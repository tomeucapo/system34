use std::error::Error;
use std::fmt;

use super::cp::{ControlProcessor, ControlStorageError, CSIPL_BOOTSTRAP_BYTES};
use super::diskette::{DisketteController, DisketteError};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IplState {
    #[default]
    NotStarted,
    /// The CSIPL bootstrap block has been copied into CP control storage.
    /// Executing it requires the Control Processor microinstruction engine.
    CsiplBootstrapLoaded { bytes: usize, words: usize },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IplError {
    Diskette(DisketteError),
    DeletedBootstrapRecord,
    InvalidBootstrapSize { expected: usize, actual: usize },
    ControlStorage(ControlStorageError),
}

impl fmt::Display for IplError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Diskette(error) => write!(f, "{error}"),
            Self::DeletedBootstrapRecord => {
                write!(f, "the CSIPL bootstrap sector is marked deleted")
            }
            Self::InvalidBootstrapSize { expected, actual } => write!(
                f,
                "the CSIPL bootstrap sector must be {expected} bytes, got {actual}"
            ),
            Self::ControlStorage(error) => write!(f, "{error}"),
        }
    }
}

impl Error for IplError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Diskette(error) => Some(error),
            Self::ControlStorage(error) => Some(error),
            _ => None,
        }
    }
}

impl From<DisketteError> for IplError {
    fn from(error: DisketteError) -> Self {
        Self::Diskette(error)
    }
}

impl From<ControlStorageError> for IplError {
    fn from(error: ControlStorageError) -> Self {
        Self::ControlStorage(error)
    }
}

/// Performs the first diskette CSIPL transfer: the 4,096-byte record at
/// physical C0/H0/R1 is copied as 2,048 big-endian words into control storage.
pub fn load_csipl_bootstrap(
    diskette: &mut DisketteController,
    cp: &mut ControlProcessor,
) -> Result<IplState, IplError> {
    let record = diskette.read_sector(0, 0, 1)?;
    if record.deleted {
        return Err(IplError::DeletedBootstrapRecord);
    }
    let data = record
        .data()
        .ok_or(IplError::Diskette(DisketteError::SectorUnavailable {
            cylinder: 0,
            head: 0,
            sector: 1,
        }))?;
    if data.len() != CSIPL_BOOTSTRAP_BYTES {
        return Err(IplError::InvalidBootstrapSize {
            expected: CSIPL_BOOTSTRAP_BYTES,
            actual: data.len(),
        });
    }

    cp.load_csipl_bootstrap(data)?;
    Ok(IplState::CsiplBootstrapLoaded {
        bytes: CSIPL_BOOTSTRAP_BYTES,
        words: CSIPL_BOOTSTRAP_BYTES / 2,
    })
}

#[cfg(test)]
mod tests {
    use super::{load_csipl_bootstrap, IplError, IplState};
    use crate::system34::{cp::ControlProcessor, diskette::DisketteController};

    fn diskette_with_boot_record(size_code: u8, record_type: u8, fill: u8) -> Vec<u8> {
        let size = 128usize << size_code;
        let mut bytes = b"IMD CSIPL fixture\x1a".to_vec();
        bytes.extend_from_slice(&[0, 0, 0, 1, size_code, 1, record_type]);
        if matches!(record_type, 1 | 3 | 5 | 7) {
            bytes.extend(std::iter::repeat_n(fill, size));
        } else if matches!(record_type, 2 | 4 | 6 | 8) {
            bytes.push(fill);
        }
        bytes
    }

    #[test]
    fn loads_first_4096_bytes_into_control_storage_words() {
        let bytes = diskette_with_boot_record(5, 1, 0xa5);
        let mut diskette = DisketteController::default();
        diskette.mount_imd(&bytes).unwrap();
        let mut cp = ControlProcessor::default();

        let result = load_csipl_bootstrap(&mut diskette, &mut cp).unwrap();

        assert_eq!(
            result,
            IplState::CsiplBootstrapLoaded {
                bytes: 4096,
                words: 2048
            }
        );
        assert_eq!(cp.control_storage[0], 0xa5a5);
        assert_eq!(cp.control_storage[2047], 0xa5a5);
        assert_eq!(cp.control_storage[2048], 0);
        assert_eq!(cp.csipl_words_loaded, 2048);
    }

    #[test]
    fn rejects_regular_128_or_512_byte_label_sectors_as_bootstrap() {
        let bytes = diskette_with_boot_record(2, 1, 0);
        let mut diskette = DisketteController::default();
        diskette.mount_imd(&bytes).unwrap();
        let mut cp = ControlProcessor::default();

        assert_eq!(
            load_csipl_bootstrap(&mut diskette, &mut cp),
            Err(IplError::InvalidBootstrapSize {
                expected: 4096,
                actual: 512
            })
        );
    }

    #[test]
    fn rejects_deleted_csipl_bootstrap_sector() {
        let bytes = diskette_with_boot_record(5, 3, 0);
        let mut diskette = DisketteController::default();
        diskette.mount_imd(&bytes).unwrap();
        let mut cp = ControlProcessor::default();

        assert_eq!(
            load_csipl_bootstrap(&mut diskette, &mut cp),
            Err(IplError::DeletedBootstrapRecord)
        );
        assert_eq!(cp.csipl_words_loaded, 0);
    }
}

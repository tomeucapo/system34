use std::error::Error;
use std::fmt;

use super::cp::ControlProcessor;
use super::disk::FixedDiskError;
use super::diskette::DisketteError;
use super::memory::{Memory, MemoryError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelTarget {
    MainStorage { byte_address: usize },
    ControlStorage { byte_address: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CycleStealProgress {
    pub transferred: usize,
    pub total: usize,
    pub complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChannelError {
    TransferAlreadyActive,
    NoTransferActive,
    Memory(MemoryError),
    Diskette(DisketteError),
    FixedDisk(FixedDiskError),
    ControlStorageOutOfRange { byte_address: usize },
}

impl fmt::Display for ChannelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TransferAlreadyActive => {
                write!(f, "an I/O cycle-steal transfer is already active")
            }
            Self::NoTransferActive => write!(f, "no I/O cycle-steal transfer is active"),
            Self::Memory(error) => write!(f, "{error}"),
            Self::Diskette(error) => write!(f, "{error}"),
            Self::FixedDisk(error) => write!(f, "{error}"),
            Self::ControlStorageOutOfRange { byte_address } => write!(
                f,
                "control-storage byte address {byte_address:#x} is out of range"
            ),
        }
    }
}

impl Error for ChannelError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Memory(error) => Some(error),
            Self::Diskette(error) => Some(error),
            Self::FixedDisk(error) => Some(error),
            _ => None,
        }
    }
}

impl From<MemoryError> for ChannelError {
    fn from(error: MemoryError) -> Self {
        Self::Memory(error)
    }
}

impl From<DisketteError> for ChannelError {
    fn from(error: DisketteError) -> Self {
        Self::Diskette(error)
    }
}

impl From<FixedDiskError> for ChannelError {
    fn from(error: FixedDiskError) -> Self {
        Self::FixedDisk(error)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IoChannel {
    pending: Option<Transfer>,
    completed_transfers: u64,
    bytes_transferred: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Transfer {
    bytes: Vec<u8>,
    target: ChannelTarget,
    transferred: usize,
}

impl IoChannel {
    pub fn start_cycle_steal(
        &mut self,
        bytes: Vec<u8>,
        target: ChannelTarget,
    ) -> Result<(), ChannelError> {
        if self.pending.is_some() {
            return Err(ChannelError::TransferAlreadyActive);
        }
        self.pending = Some(Transfer {
            bytes,
            target,
            transferred: 0,
        });
        Ok(())
    }

    /// Transfers one byte, matching the byte-wide data path used by the
    /// System/34 channel's cycle-steal service.
    pub fn cycle_steal(
        &mut self,
        memory: &mut Memory,
        cp: &mut ControlProcessor,
    ) -> Result<CycleStealProgress, ChannelError> {
        let transfer = self
            .pending
            .as_mut()
            .ok_or(ChannelError::NoTransferActive)?;
        let total = transfer.bytes.len();
        if transfer.transferred < total {
            let byte = transfer.bytes[transfer.transferred];
            let address = match transfer.target {
                ChannelTarget::MainStorage { byte_address } => byte_address + transfer.transferred,
                ChannelTarget::ControlStorage { byte_address } => {
                    byte_address + transfer.transferred
                }
            };
            match transfer.target {
                ChannelTarget::MainStorage { .. } => memory.write(address, byte)?,
                ChannelTarget::ControlStorage { .. } => {
                    write_control_storage_byte(cp, address, byte)?
                }
            }
            transfer.transferred += 1;
            self.bytes_transferred += 1;
        }

        let progress = CycleStealProgress {
            transferred: transfer.transferred,
            total,
            complete: transfer.transferred == total,
        };
        if progress.complete {
            self.pending = None;
            self.completed_transfers += 1;
        }
        Ok(progress)
    }

    pub fn transfer_progress(&self) -> Option<CycleStealProgress> {
        self.pending.as_ref().map(|transfer| CycleStealProgress {
            transferred: transfer.transferred,
            total: transfer.bytes.len(),
            complete: false,
        })
    }

    pub fn completed_transfers(&self) -> u64 {
        self.completed_transfers
    }

    pub fn bytes_transferred(&self) -> u64 {
        self.bytes_transferred
    }

    pub fn is_busy(&self) -> bool {
        self.pending.is_some()
    }
}

fn write_control_storage_byte(
    cp: &mut ControlProcessor,
    byte_address: usize,
    value: u8,
) -> Result<(), ChannelError> {
    let word_address = byte_address / 2;
    let word = cp
        .control_storage
        .get_mut(word_address)
        .ok_or(ChannelError::ControlStorageOutOfRange { byte_address })?;
    if byte_address & 1 == 0 {
        *word = (*word & 0x00ff) | ((value as u16) << 8);
    } else {
        *word = (*word & 0xff00) | value as u16;
        cp.csipl_words_loaded = cp.csipl_words_loaded.max(word_address + 1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ChannelError, ChannelTarget, IoChannel};
    use crate::system34::{cp::ControlProcessor, memory::Memory};

    #[test]
    fn cycle_steal_transfers_one_byte_per_service_and_completes() {
        let mut channel = IoChannel::default();
        let mut memory = Memory::new(1);
        let mut cp = ControlProcessor::default();
        channel
            .start_cycle_steal(
                vec![0x11, 0x22, 0x33],
                ChannelTarget::MainStorage { byte_address: 10 },
            )
            .unwrap();

        assert_eq!(
            channel.cycle_steal(&mut memory, &mut cp),
            Ok(super::CycleStealProgress {
                transferred: 1,
                total: 3,
                complete: false,
            })
        );
        assert_eq!(memory.read(10), Ok(0x11));
        assert_eq!(memory.read(11), Ok(0));
        channel.cycle_steal(&mut memory, &mut cp).unwrap();
        let final_cycle = channel.cycle_steal(&mut memory, &mut cp).unwrap();

        assert!(final_cycle.complete);
        assert!(!channel.is_busy());
        assert_eq!(memory.read(12), Ok(0x33));
        assert_eq!(channel.completed_transfers(), 1);
        assert_eq!(channel.bytes_transferred(), 3);
    }

    #[test]
    fn cycle_steal_loads_control_storage_words_in_big_endian_order() {
        let mut channel = IoChannel::default();
        let mut memory = Memory::new(1);
        let mut cp = ControlProcessor::default();
        cp.control_storage.fill(0);
        channel
            .start_cycle_steal(
                vec![0x12, 0x34, 0xab, 0xcd],
                ChannelTarget::ControlStorage { byte_address: 0 },
            )
            .unwrap();

        for _ in 0..4 {
            channel.cycle_steal(&mut memory, &mut cp).unwrap();
        }

        assert_eq!(&cp.control_storage[..2], &[0x1234, 0xabcd]);
        assert_eq!(cp.csipl_words_loaded, 2);
    }

    #[test]
    fn rejects_second_request_until_the_active_transfer_finishes() {
        let mut channel = IoChannel::default();
        channel
            .start_cycle_steal(vec![0x77], ChannelTarget::MainStorage { byte_address: 0 })
            .unwrap();
        assert_eq!(
            channel.start_cycle_steal(vec![0x88], ChannelTarget::MainStorage { byte_address: 1 },),
            Err(ChannelError::TransferAlreadyActive)
        );
    }
}

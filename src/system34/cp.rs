use std::error::Error;
use std::fmt;

pub const CONTROL_STORAGE_WORDS: usize = 16 * 1024;
pub const CSIPL_BOOTSTRAP_BYTES: usize = 4096;

/// Control Processor register state.
///
/// Execution of CP microinstructions will be added after their formats and
/// sequencing have been transcribed from the System/34 references.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlProcessor {
    pub sar: u32,
    pub sdr: u16,
    pub mor: u16,
    pub x: u16,
    pub y: u16,
    pub pcr: u16,
    pub lsr: [u8; 64],
    pub control_storage: Vec<u16>,
    pub csipl_words_loaded: usize,
    pending_interrupt: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlStorageError {
    InvalidCsiplLength { expected: usize, actual: usize },
}

impl fmt::Display for ControlStorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCsiplLength { expected, actual } => write!(
                f,
                "CSIPL bootstrap must contain {expected} bytes, got {actual}"
            ),
        }
    }
}

impl Error for ControlStorageError {}

impl Default for ControlProcessor {
    fn default() -> Self {
        Self {
            sar: 0,
            sdr: 0,
            mor: 0,
            x: 0,
            y: 0,
            pcr: 0,
            lsr: [0; 64],
            control_storage: vec![0; CONTROL_STORAGE_WORDS],
            csipl_words_loaded: 0,
            pending_interrupt: None,
        }
    }
}

impl ControlProcessor {
    /// Loads the first 2K 16-bit control-storage words transferred by CSIPL.
    pub fn load_csipl_bootstrap(&mut self, bytes: &[u8]) -> Result<(), ControlStorageError> {
        if bytes.len() != CSIPL_BOOTSTRAP_BYTES {
            return Err(ControlStorageError::InvalidCsiplLength {
                expected: CSIPL_BOOTSTRAP_BYTES,
                actual: bytes.len(),
            });
        }

        self.control_storage.fill(0);
        let (words, _) = bytes.as_chunks::<2>();
        for (word, pair) in self.control_storage[..CSIPL_BOOTSTRAP_BYTES / 2]
            .iter_mut()
            .zip(words)
        {
            *word = u16::from_be_bytes(*pair);
        }
        self.csipl_words_loaded = CSIPL_BOOTSTRAP_BYTES / 2;
        self.sar = 0;
        Ok(())
    }

    pub fn request_interrupt(&mut self, level: u8) {
        self.pending_interrupt = Some(level);
    }

    pub fn pending_interrupt(&self) -> Option<u8> {
        self.pending_interrupt
    }

    pub fn acknowledge_interrupt(&mut self) -> Option<u8> {
        self.pending_interrupt.take()
    }
}

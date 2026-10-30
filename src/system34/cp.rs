use std::error::Error;
use std::fmt;

pub const CONTROL_STORAGE_WORDS: usize = 16 * 1024;
pub const CSIPL_BOOTSTRAP_BYTES: usize = 4096;
pub const MAIN_LEVEL_MAR_LSR: usize = 0x08;

/// Control Processor register state.
///
/// Execution covers the currently decoded subset of CP microinstructions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlProcessor {
    pub sar: u16,
    pub sdr: u16,
    pub mor: u16,
    pub x: u16,
    pub y: u16,
    pub pcr: u16,
    /// Sixty-four 16-bit local storage registers (interrupt work registers,
    /// microaddress registers and control registers).
    pub lsr: [u16; 64],
    pub control_storage: Vec<u16>,
    pub csipl_words_loaded: usize,
    pending_interrupts: u8,
    io_condition: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpStepOutcome {
    pub address: u16,
    pub word: u16,
    pub branch_taken: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlStorageError {
    InvalidCsiplLength { expected: usize, actual: usize },
    InvalidMicroaddress { address: u16 },
    UnsupportedMicroinstruction { address: u16, word: u16 },
}

impl fmt::Display for ControlStorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCsiplLength { expected, actual } => write!(
                f,
                "CSIPL bootstrap must contain {expected} bytes, got {actual}"
            ),
            Self::InvalidMicroaddress { address } => write!(
                f,
                "control-processor microaddress {address:#06x} is outside control storage"
            ),
            Self::UnsupportedMicroinstruction { address, word } => write!(
                f,
                "unsupported control-processor microinstruction {word:#06x} at {address:#06x}"
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
            pending_interrupts: 0,
            io_condition: false,
        }
    }
}

impl ControlProcessor {
    pub fn microaddress(&self) -> u16 {
        self.lsr[MAIN_LEVEL_MAR_LSR]
    }

    /// Fetches the next 16-bit control-store word and advances the main-level
    /// MAR. Execution of the fetched microinstruction is a separate stage.
    pub fn fetch_microinstruction(&mut self) -> Result<(u16, u16), ControlStorageError> {
        let address = self.microaddress();
        let word = self
            .control_storage
            .get(address as usize)
            .copied()
            .ok_or(ControlStorageError::InvalidMicroaddress { address })?;
        self.mor = word;
        self.lsr[MAIN_LEVEL_MAR_LSR] = address.wrapping_add(1);
        Ok((address, word))
    }

    /// Fetches and executes the CP instructions whose formats and basic
    /// semantics are transcribed here. I/O and remaining micro-operations
    /// report `UnsupportedMicroinstruction` until their device timing/control
    /// fields are implemented.
    pub fn step_microinstruction(&mut self) -> Result<CpStepOutcome, ControlStorageError> {
        let (address, word) = self.fetch_microinstruction()?;
        let opcode = (word >> 12) as u8;
        let branch_taken = match opcode {
            0x0 => {
                self.set_microaddress(word & 0x0fff);
                Some(true)
            }
            0x1 => {
                self.lsr[MAIN_LEVEL_MAR_LSR + 1] = self.microaddress();
                self.set_microaddress(word & 0x0fff);
                Some(true)
            }
            0x2 => {
                let condition = ((word >> 8) & 0x0f) as u8;
                let taken = self.test_branch_condition(condition);
                if taken {
                    let target = if condition == 0x0f {
                        self.lsr[MAIN_LEVEL_MAR_LSR + 1]
                    } else {
                        (self.microaddress() & 0xff00) | (word & 0x00ff)
                    };
                    self.set_microaddress(target);
                }
                Some(taken)
            }
            0x3 => {
                let condition = ((word >> 8) & 0x0f) as u8;
                let taken = self.io_condition && self.test_branch_condition(condition);
                if taken {
                    let target = if condition == 0x0f {
                        self.lsr[MAIN_LEVEL_MAR_LSR + 1]
                    } else {
                        (self.microaddress() & 0xff00) | (word & 0x00ff)
                    };
                    self.set_microaddress(target);
                }
                Some(taken)
            }
            0x5 => {
                self.test_mask(word);
                None
            }
            0x6 => {
                self.logical_arithmetic(word, false)?;
                None
            }
            0x7 => {
                self.logical_arithmetic(word, true)?;
                None
            }
            0x8 => {
                self.mask_register_byte(word, false);
                None
            }
            0x9 => {
                self.mask_register_byte(word, true);
                None
            }
            0xa => {
                self.load_immediate_byte(word);
                None
            }
            0xc => {
                self.compare_immediate_byte(word);
                None
            }
            _ => {
                return Err(ControlStorageError::UnsupportedMicroinstruction { address, word });
            }
        };
        Ok(CpStepOutcome {
            address,
            word,
            branch_taken,
        })
    }

    pub fn set_io_condition(&mut self, active: bool) {
        self.io_condition = active;
    }

    fn set_microaddress(&mut self, address: u16) {
        self.lsr[MAIN_LEVEL_MAR_LSR] = address;
    }

    fn test_branch_condition(&self, condition: u8) -> bool {
        const FLAG: u16 = 1 << 0;
        const POSITIVE: u16 = 1 << 1;
        const NEGATIVE: u16 = 1 << 2;
        const ZERO: u16 = 1 << 3;
        const CARRY: u16 = 1 << 4;
        const HIGH: u16 = 1 << 5;
        const LOW: u16 = 1 << 6;
        const EQUAL: u16 = 1 << 7;

        match condition {
            0x0 => self.pcr & CARRY != 0,
            0x1 => self.pcr & HIGH != 0,
            0x2 => self.pcr & LOW != 0,
            0x3 => self.pcr & EQUAL != 0,
            0x4 => self.pcr & POSITIVE != 0,
            0x5 => self.pcr & NEGATIVE != 0,
            0x6 => self.pcr & ZERO != 0,
            0x7 => self.pcr & FLAG != 0,
            0x8 => self.pending_interrupts != 0,
            0x9 => self.pcr & HIGH == 0,
            0xa => self.pcr & LOW == 0,
            0xb => self.pcr & EQUAL == 0,
            0xc => self.pcr & POSITIVE == 0,
            0xd => self.pcr & NEGATIVE == 0,
            0xe => self.pcr & ZERO == 0,
            0xf => true,
            _ => false,
        }
    }

    fn register_byte(&self, register: usize, high: bool) -> u8 {
        let word = self.lsr[register];
        if high {
            (word >> 8) as u8
        } else {
            word as u8
        }
    }

    fn set_register_byte(&mut self, register: usize, high: bool, value: u8) {
        let word = &mut self.lsr[register];
        if high {
            *word = (*word & 0x00ff) | ((value as u16) << 8);
        } else {
            *word = (*word & 0xff00) | value as u16;
        }
    }

    fn set_result_condition(&mut self, value: u16, width: u32, carry: bool) {
        const POSITIVE: u16 = 1 << 1;
        const NEGATIVE: u16 = 1 << 2;
        const ZERO: u16 = 1 << 3;
        const CARRY: u16 = 1 << 4;
        const CONDITION_MASK: u16 = POSITIVE | NEGATIVE | ZERO | CARRY;
        let mask = if width == 8 { 0x00ff } else { 0xffff };
        let value = value & mask;
        let sign = if width == 8 { 0x0080 } else { 0x8000 };
        let condition = if value == 0 {
            ZERO
        } else if value & sign != 0 {
            NEGATIVE
        } else {
            POSITIVE
        } | if carry { CARRY } else { 0 };
        self.pcr = (self.pcr & !CONDITION_MASK) | condition;
    }

    fn compare_condition(&mut self, left: u16, right: u16) {
        const HIGH: u16 = 1 << 5;
        const LOW: u16 = 1 << 6;
        const EQUAL: u16 = 1 << 7;
        const MASK: u16 = HIGH | LOW | EQUAL;
        let condition = match left.cmp(&right) {
            std::cmp::Ordering::Less => LOW,
            std::cmp::Ordering::Equal => EQUAL,
            std::cmp::Ordering::Greater => HIGH,
        };
        self.pcr = (self.pcr & !MASK) | condition;
    }

    fn test_mask(&mut self, word: u16) {
        let high = word & 0x0800 != 0;
        let register = ((word >> 8) & 0x07) as usize;
        let mask = word as u8;
        let value = self.register_byte(register, high);
        const ZERO: u16 = 1 << 3;
        const POSITIVE: u16 = 1 << 1;
        const NEGATIVE: u16 = 1 << 2;
        const MASKED: u16 = ZERO | POSITIVE | NEGATIVE;
        let tested = value & mask;
        let condition = if tested == 0 {
            ZERO
        } else if tested == mask {
            POSITIVE
        } else {
            NEGATIVE
        };
        self.pcr = (self.pcr & !MASKED) | condition;
    }

    fn mask_register_byte(&mut self, word: u16, set_bits: bool) {
        let high = word & 0x0800 != 0;
        let register = ((word >> 8) & 0x07) as usize;
        let mask = word as u8;
        let value = self.register_byte(register, high);
        let result = if set_bits {
            value | mask
        } else {
            value & !mask
        };
        self.set_register_byte(register, high, result);
    }

    fn load_immediate_byte(&mut self, word: u16) {
        let high = word & 0x0800 != 0;
        let register = ((word >> 8) & 0x07) as usize;
        self.set_register_byte(register, high, word as u8);
    }

    fn compare_immediate_byte(&mut self, word: u16) {
        let high = word & 0x0800 != 0;
        let register = ((word >> 8) & 0x07) as usize;
        self.compare_condition(self.register_byte(register, high) as u16, word as u8 as u16);
    }

    fn logical_arithmetic(
        &mut self,
        word: u16,
        word_operation: bool,
    ) -> Result<(), ControlStorageError> {
        let high1 = word & 0x0800 != 0;
        let register1 = ((word >> 8) & 0x07) as usize;
        let function = ((word >> 4) & 0x0f) as u8;
        let high2 = word & 0x0008 != 0;
        let register2 = (word & 0x0007) as usize;

        let (left, right, width) = if word_operation {
            (self.lsr[register1], self.lsr[register2], 16)
        } else {
            (
                self.register_byte(register1, high1) as u16,
                self.register_byte(register2, high2) as u16,
                8,
            )
        };
        let (result, carry) = match function {
            0x0 => (left ^ right, false),
            0x1 => (left | right, false),
            0x2 => (left & !right, false),
            0x3 => (left & right, false),
            0x4 => (left | !right, false),
            0x5 => (left.wrapping_sub(1), left == 0),
            0x6 => {
                let sum = left as u32 + right as u32 + u32::from(self.pcr & (1 << 4) != 0);
                (sum as u16, sum > if width == 8 { 0xff } else { 0xffff })
            }
            0x7 => (left.wrapping_sub(right).wrapping_add(1), left >= right),
            0x8 => {
                let sum = left as u32 + right as u32;
                (sum as u16, sum > if width == 8 { 0xff } else { 0xffff })
            }
            0x9 => (left.wrapping_sub(right), left >= right),
            0xa => (left.wrapping_add(1), false),
            _ => {
                return Err(ControlStorageError::UnsupportedMicroinstruction {
                    address: self.microaddress().wrapping_sub(1),
                    word,
                })
            }
        };

        if word_operation {
            self.lsr[register1] = result;
        } else {
            self.set_register_byte(register1, high1, result as u8);
        }
        self.set_result_condition(result, width, carry);
        Ok(())
    }

    pub fn begin_csipl_transfer(&mut self) {
        self.control_storage.fill(0);
        self.csipl_words_loaded = 0;
        self.sar = 0;
        self.lsr[MAIN_LEVEL_MAR_LSR] = 0;
    }

    /// Loads the first 2K 16-bit control-storage words transferred by CSIPL.
    pub fn load_csipl_bootstrap(&mut self, bytes: &[u8]) -> Result<(), ControlStorageError> {
        if bytes.len() != CSIPL_BOOTSTRAP_BYTES {
            return Err(ControlStorageError::InvalidCsiplLength {
                expected: CSIPL_BOOTSTRAP_BYTES,
                actual: bytes.len(),
            });
        }

        self.begin_csipl_transfer();
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
        if level < u8::BITS as u8 {
            self.pending_interrupts |= 1 << level;
        }
    }

    pub fn pending_interrupt(&self) -> Option<u8> {
        (self.pending_interrupts != 0).then(|| self.pending_interrupts.trailing_zeros() as u8)
    }

    pub fn acknowledge_interrupt(&mut self) -> Option<u8> {
        let level = self.pending_interrupt()?;
        self.pending_interrupts &= !(1 << level);
        Some(level)
    }
}

#[cfg(test)]
mod tests {
    use super::{ControlProcessor, ControlStorageError, MAIN_LEVEL_MAR_LSR};

    #[test]
    fn fetches_a_control_word_and_advances_the_main_level_microaddress() {
        let mut cp = ControlProcessor::default();
        cp.control_storage[0] = 0x1234;
        cp.control_storage[1] = 0xabcd;

        assert_eq!(cp.fetch_microinstruction(), Ok((0, 0x1234)));
        assert_eq!(cp.mor, 0x1234);
        assert_eq!(cp.lsr[MAIN_LEVEL_MAR_LSR], 1);
        assert_eq!(cp.fetch_microinstruction(), Ok((1, 0xabcd)));
        assert_eq!(cp.lsr[MAIN_LEVEL_MAR_LSR], 2);
    }

    #[test]
    fn rejects_microaddresses_outside_control_storage() {
        let mut cp = ControlProcessor::default();
        cp.lsr[MAIN_LEVEL_MAR_LSR] = cp.control_storage.len() as u16;

        assert_eq!(
            cp.fetch_microinstruction(),
            Err(ControlStorageError::InvalidMicroaddress {
                address: cp.control_storage.len() as u16
            })
        );
    }

    #[test]
    fn executes_absolute_branch_and_branch_and_link_return() {
        let mut cp = ControlProcessor::default();
        cp.control_storage[0] = 0x1004;
        cp.control_storage[4] = 0x2f00;

        assert_eq!(
            cp.step_microinstruction(),
            Ok(super::CpStepOutcome {
                address: 0,
                word: 0x1004,
                branch_taken: Some(true),
            })
        );
        assert_eq!(cp.lsr[MAIN_LEVEL_MAR_LSR], 4);
        assert_eq!(cp.lsr[MAIN_LEVEL_MAR_LSR + 1], 1);

        let outcome = cp.step_microinstruction().unwrap();
        assert_eq!(outcome.branch_taken, Some(true));
        assert_eq!(cp.lsr[MAIN_LEVEL_MAR_LSR], 1);
    }

    #[test]
    fn executes_immediate_load_mask_and_logical_arithmetic() {
        let mut cp = ControlProcessor::default();
        cp.control_storage[0] = 0xa20f; // LI low byte of LSR 2, immediate 0x0f
        cp.control_storage[1] = 0x520f; // TM low byte of LSR 2, mask 0x0f
        cp.control_storage[2] = 0x8201; // SBF bit 0 of low byte of LSR 2
        cp.control_storage[3] = 0x6281; // LA1: add low bytes of LSR 2 and 1
        cp.lsr[1] = 2;

        cp.step_microinstruction().unwrap();
        assert_eq!(cp.lsr[2], 0x000f);

        cp.step_microinstruction().unwrap();
        assert_ne!(cp.pcr & (1 << 1), 0); // TM reports all selected bits on

        cp.step_microinstruction().unwrap();
        assert_eq!(cp.lsr[2], 0x000e);

        cp.step_microinstruction().unwrap();
        assert_eq!(cp.lsr[2], 16);
    }

    #[test]
    fn reports_an_unsupported_microinstruction_with_its_location() {
        let mut cp = ControlProcessor::default();
        cp.control_storage[0] = 0xb000;

        assert_eq!(
            cp.step_microinstruction(),
            Err(ControlStorageError::UnsupportedMicroinstruction {
                address: 0,
                word: 0xb000,
            })
        );
    }
}

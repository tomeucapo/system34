use std::error::Error;
use std::fmt;
use std::ops::Range;

use super::isa::{self, DecodeError, DecodedInstruction, Operation};
use super::memory::{Memory, MemoryError};

const CC_EQUAL: u16 = 0x01;
const CC_LOW: u16 = 0x02;
const CC_HIGH: u16 = 0x04;
const DECIMAL_OVERFLOW: u16 = 0x08;
const TEST_FALSE: u16 = 0x10;
const BINARY_OVERFLOW: u16 = 0x20;
const CONDITION_CODE_MASK: u16 = CC_EQUAL | CC_LOW | CC_HIGH;

/// Main Storage Processor architectural and working registers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MspRegisters {
    pub msar: u32,
    pub index_registers: [u16; 2],
    pub y: u8,
    pub opreg: u8,
    pub q_backup: u8,
    pub q: u8,
    /// The System/34 program status register, including its status byte.
    pub psr: u16,
    pub sbr: [u8; 4],
    pub ccr: u8,
    pub acr: u32,
    pub iar: u32,
    pub arr: u32,
    pub program_mode: u8,
    pub lsr: u8,
}

/// Result of advancing the MSP once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepOutcome {
    Executed { address: u32, opcode: u8 },
    SupervisorCall { address: u32, interrupt_level: u8 },
    WaitingForControlProcessor,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MspError {
    Memory(MemoryError),
    UnsupportedOpcode { address: u32, opcode: u8 },
    ProgramCounterOverflow,
    InvalidAddressingMode { address: u32, opcode: u8 },
    InvalidRegisterCode { opcode: u8, q: u8 },
    InvalidParameter { opcode: u8, parameter: u8 },
    InvalidField { address: u32, length: usize },
    InvalidDecimalDigit { address: u32, value: u8 },
    PrivilegeViolation { address: u32, opcode: u8 },
    ProgramCheck { address: u32, opcode: u8 },
}

impl fmt::Display for MspError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Memory(error) => write!(f, "{error}"),
            Self::UnsupportedOpcode { address, opcode } => write!(
                f,
                "unsupported MSP opcode {opcode:#04x} at address {address:#07x}"
            ),
            Self::ProgramCounterOverflow => write!(f, "MSP instruction address overflow"),
            Self::InvalidAddressingMode { address, opcode } => write!(
                f,
                "invalid addressing mode in MSP opcode {opcode:#04x} at address {address:#07x}"
            ),
            Self::InvalidRegisterCode { opcode, q } => write!(
                f,
                "invalid register selection {q:#04x} for MSP opcode {opcode:#04x}"
            ),
            Self::InvalidParameter { opcode, parameter } => write!(
                f,
                "invalid parameter {parameter:#04x} for MSP opcode {opcode:#04x}"
            ),
            Self::InvalidField { address, length } => write!(
                f,
                "invalid {length}-byte MSP field ending at address {address:#07x}"
            ),
            Self::InvalidDecimalDigit { address, value } => write!(
                f,
                "invalid zoned-decimal byte {value:#04x} at address {address:#07x}"
            ),
            Self::PrivilegeViolation { address, opcode } => write!(
                f,
                "privileged MSP opcode {opcode:#04x} used in nonprivileged mode at {address:#07x}"
            ),
            Self::ProgramCheck { address, opcode } => write!(
                f,
                "MSP program check for opcode {opcode:#04x} at address {address:#07x}"
            ),
        }
    }
}

impl Error for MspError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Memory(error) => Some(error),
            _ => None,
        }
    }
}

impl From<MemoryError> for MspError {
    fn from(error: MemoryError) -> Self {
        Self::Memory(error)
    }
}

impl From<DecodeError> for MspError {
    fn from(error: DecodeError) -> Self {
        match error {
            DecodeError::Memory(error) => Self::Memory(error),
            DecodeError::UnsupportedOpcode { address, opcode } => {
                Self::UnsupportedOpcode { address, opcode }
            }
            DecodeError::ProgramCounterOverflow => Self::ProgramCounterOverflow,
            DecodeError::InvalidAddressingMode { address, opcode } => {
                Self::InvalidAddressingMode { address, opcode }
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Msp {
    pub registers: MspRegisters,
    waiting_for_control_processor: bool,
}

impl Msp {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self, entry_point: u32) {
        self.registers = MspRegisters {
            iar: entry_point,
            ..MspRegisters::default()
        };
        self.waiting_for_control_processor = false;
    }

    pub fn resume_from_control_processor(&mut self) {
        self.waiting_for_control_processor = false;
    }

    /// Fetches, decodes, and executes one base System/34 MSP instruction.
    pub fn step(&mut self, memory: &mut Memory) -> Result<StepOutcome, MspError> {
        if self.waiting_for_control_processor {
            return Ok(StepOutcome::WaitingForControlProcessor);
        }

        let instruction = isa::decode(memory, &self.registers, self.registers.iar)?;
        self.registers.opreg = instruction.opcode;
        self.registers.q = instruction.q;
        self.registers.q_backup = instruction.q;
        self.registers.iar = instruction.next_address;
        self.execute(memory, &instruction)
    }

    fn execute(
        &mut self,
        memory: &mut Memory,
        instruction: &DecodedInstruction,
    ) -> Result<StepOutcome, MspError> {
        let address = instruction.address;
        let opcode = instruction.opcode;
        let q = instruction.q;

        match instruction.operation {
            Operation::ZeroAndAddZoned => self.zero_and_add_zoned(memory, instruction)?,
            Operation::AddZoned => self.decimal_arithmetic(memory, instruction, false)?,
            Operation::SubtractZoned => self.decimal_arithmetic(memory, instruction, true)?,
            Operation::MoveHexCharacter => self.move_hex_character(memory, instruction)?,
            Operation::MoveCharacters => self.move_characters(memory, instruction)?,
            Operation::Edit => self.edit(memory, instruction)?,
            Operation::InsertAndTestCharacters => self.insert_and_test(memory, instruction)?,
            Operation::CompareLogicalCharacters => {
                self.compare_logical_characters(memory, instruction)?
            }
            Operation::AddLogicalCharacters => {
                self.logical_arithmetic(memory, instruction, false)?
            }
            Operation::SubtractLogicalCharacters => {
                self.logical_arithmetic(memory, instruction, true)?
            }
            Operation::MoveImmediate => {
                let target = required_address(instruction.operand1, instruction)?;
                memory.write(target as usize, q)?;
            }
            Operation::CompareLogicalImmediate => {
                let target = required_address(instruction.operand1, instruction)?;
                let value = memory.read(target as usize)?;
                self.set_comparison(value.cmp(&q));
            }
            Operation::SetBitsOnMasked => {
                let target = required_address(instruction.operand1, instruction)?;
                let value = memory.read(target as usize)?;
                memory.write(target as usize, value | q)?;
            }
            Operation::SetBitsOffMasked => {
                let target = required_address(instruction.operand1, instruction)?;
                let value = memory.read(target as usize)?;
                memory.write(target as usize, value & !q)?;
            }
            Operation::TestBitsOnMasked => {
                let target = required_address(instruction.operand1, instruction)?;
                let value = memory.read(target as usize)?;
                if value & q != q {
                    self.registers.psr |= TEST_FALSE;
                }
            }
            Operation::TestBitsOffMasked => {
                let target = required_address(instruction.operand1, instruction)?;
                let value = memory.read(target as usize)?;
                if value & q != 0 {
                    self.registers.psr |= TEST_FALSE;
                }
            }
            Operation::StoreRegister => {
                let target = required_address(instruction.operand1, instruction)?;
                if let Some(value) = self.read_register(q, opcode, false)? {
                    write_field(memory, target, &value.to_be_bytes())?;
                }
            }
            Operation::LoadRegister => {
                if q != 0 {
                    let source = required_address(instruction.operand1, instruction)?;
                    let value =
                        u16::from_be_bytes(read_field(memory, source, 2)?.try_into().unwrap());
                    self.write_register(q, value, opcode)?;
                }
            }
            Operation::AddToRegister => self.add_to_register(memory, instruction)?,
            Operation::LoadAddress => {
                let target = required_address(instruction.operand2, instruction)?;
                let destination = match q {
                    0x01 | 0x03 => 0,
                    0x00 | 0x02 => 1,
                    _ => return Err(MspError::InvalidRegisterCode { opcode, q }),
                };
                self.registers.index_registers[destination] = target as u16;
            }
            Operation::BranchOnCondition => {
                let target = required_address(instruction.operand2, instruction)?;
                if self.test_branch_condition(q) {
                    self.registers.arr = self.registers.iar;
                    self.registers.iar = target;
                }
            }
            Operation::JumpOnCondition => {
                if self.test_branch_condition(q) {
                    let displacement = instruction.parameter.unwrap_or_default() as u32;
                    self.registers.iar = (self.registers.iar + displacement) & 0x1ffff;
                }
            }
            Operation::SupervisorCall => {
                self.waiting_for_control_processor = true;
                return Ok(StepOutcome::SupervisorCall {
                    address,
                    interrupt_level: 5,
                });
            }
            Operation::Transfer => {
                return Err(MspError::ProgramCheck { address, opcode });
            }
            Operation::LoadProgramModeRegister => {
                if self.registers.program_mode & 0x80 != 0 {
                    return Err(MspError::PrivilegeViolation { address, opcode });
                }
                self.registers.program_mode = instruction.parameter.unwrap_or_default() & 0xf1;
            }
        }

        Ok(StepOutcome::Executed { address, opcode })
    }

    fn zero_and_add_zoned(
        &mut self,
        memory: &mut Memory,
        instruction: &DecodedInstruction,
    ) -> Result<(), MspError> {
        let target = required_address(instruction.operand1, instruction)?;
        let source = required_address(instruction.operand2, instruction)?;
        let length2 = (instruction.q as usize & 0x0f) + 1;
        let length1 = (instruction.q as usize >> 4) + length2;
        validate_field(memory, target, length1)?;
        validate_field(memory, source, length2)?;

        let source_bytes = read_field(memory, source, length2)?;
        let negative = is_negative_zoned(*source_bytes.last().unwrap());
        let zero = source_bytes.iter().all(|byte| byte & 0x0f == 0);

        for offset in 0..length1 {
            let destination = target - offset as u32;
            let mut value = if offset < length2 {
                memory.read((source - offset as u32) as usize)?
            } else {
                0xf0
            };
            if offset == 0 {
                value = (value & 0x0f) | if negative && !zero { 0xd0 } else { 0xf0 };
            } else {
                value = (value & 0x0f) | 0xf0;
            }
            memory.write(destination as usize, value)?;
        }

        self.set_signed_condition(zero, negative && !zero);
        self.registers.arr = target;
        Ok(())
    }

    fn decimal_arithmetic(
        &mut self,
        memory: &mut Memory,
        instruction: &DecodedInstruction,
        subtract: bool,
    ) -> Result<(), MspError> {
        let target = required_address(instruction.operand1, instruction)?;
        let source = required_address(instruction.operand2, instruction)?;
        let length2 = (instruction.q as usize & 0x0f) + 1;
        let length1 = (instruction.q as usize >> 4) + length2;
        let left_bytes = read_field(memory, target, length1)?;
        let right_bytes = read_field(memory, source, length2)?;
        let (left_digits, left_negative) = decode_zoned(&left_bytes, target)?;
        let (right_digits, right_negative) = decode_zoned(&right_bytes, source)?;
        let (digits, negative, overflow) = signed_decimal_add(
            left_digits,
            left_negative,
            right_digits,
            right_negative ^ subtract,
            length1,
        );
        let result_zero = digits.iter().all(|digit| *digit == 0);
        let mut output = Vec::with_capacity(length1);
        for position in (0..length1).rev() {
            let digit = digits.get(position).copied().unwrap_or(0) & 0x0f;
            let zone = if position == 0 && negative && !result_zero {
                0xd0
            } else {
                0xf0
            };
            output.push(zone | digit);
        }
        write_field(memory, target, &output)?;
        self.set_signed_condition(result_zero, negative && !result_zero);
        if overflow {
            self.registers.psr |= DECIMAL_OVERFLOW;
        }
        self.registers.arr = target;
        Ok(())
    }

    fn move_hex_character(
        &mut self,
        memory: &mut Memory,
        instruction: &DecodedInstruction,
    ) -> Result<(), MspError> {
        let destination = required_address(instruction.operand1, instruction)?;
        let source = required_address(instruction.operand2, instruction)?;
        let source_byte = memory.read(source as usize)?;
        let destination_byte = memory.read(destination as usize)?;
        let result = match instruction.q {
            0x00 => (destination_byte & 0x0f) | (source_byte & 0xf0),
            0x01 => (destination_byte & 0x0f) | ((source_byte & 0x0f) << 4),
            0x02 => (destination_byte & 0xf0) | (source_byte >> 4),
            0x03 => (destination_byte & 0xf0) | (source_byte & 0x0f),
            parameter => {
                return Err(MspError::InvalidParameter {
                    opcode: instruction.opcode,
                    parameter,
                })
            }
        };
        memory.write(destination as usize, result)?;
        Ok(())
    }

    fn move_characters(
        &mut self,
        memory: &mut Memory,
        instruction: &DecodedInstruction,
    ) -> Result<(), MspError> {
        let destination = required_address(instruction.operand1, instruction)?;
        let source = required_address(instruction.operand2, instruction)?;
        let length = instruction.q as usize + 1;
        validate_field(memory, destination, length)?;
        validate_field(memory, source, length)?;

        // The hardware moves from the rightmost byte toward the left; retaining
        // that order also preserves its documented behavior for overlapping fields.
        for offset in 0..length {
            let value = memory.read((source - offset as u32) as usize)?;
            memory.write((destination - offset as u32) as usize, value)?;
        }
        Ok(())
    }

    fn edit(
        &mut self,
        memory: &mut Memory,
        instruction: &DecodedInstruction,
    ) -> Result<(), MspError> {
        let destination = required_address(instruction.operand1, instruction)?;
        let source = required_address(instruction.operand2, instruction)?;
        let length = instruction.q as usize + 1;
        let destination_bytes = read_field(memory, destination, length)?;
        let source_length = destination_bytes
            .iter()
            .filter(|byte| **byte == 0x20)
            .count();
        if source_length != 0 {
            validate_field(memory, source, source_length)?;
        }
        let source_bytes = if source_length == 0 {
            Vec::new()
        } else {
            read_field(memory, source, source_length)?
        };
        let (digits, negative) = if source_bytes.is_empty() {
            (vec![0], false)
        } else {
            decode_zoned(&source_bytes, source)?
        };
        let zero = digits.iter().all(|digit| *digit == 0);
        let rightmost_was_blank = destination_bytes.last() == Some(&0x20);
        let equal_was_set = self.registers.psr & CC_EQUAL != 0;
        let mut source_offset = 0u32;

        for offset in 0..length {
            let destination_address = destination - offset as u32;
            if memory.read(destination_address as usize)? == 0x20 {
                let mut value = memory.read((source - source_offset) as usize)?;
                value = (value & 0x0f) | 0xf0;
                memory.write(destination_address as usize, value)?;
                source_offset += 1;
            }
        }

        if equal_was_set || rightmost_was_blank || !zero {
            self.set_signed_condition(zero, negative && !zero);
        }
        Ok(())
    }

    fn insert_and_test(
        &mut self,
        memory: &mut Memory,
        instruction: &DecodedInstruction,
    ) -> Result<(), MspError> {
        let leftmost = required_address(instruction.operand1, instruction)?;
        let fill_address = required_address(instruction.operand2, instruction)?;
        let length = instruction.q as usize + 1;
        let end = leftmost
            .checked_add(length as u32 - 1)
            .ok_or(MspError::ProgramCounterOverflow)?;
        validate_field(memory, end, length)?;
        let fill = memory.read(fill_address as usize)?;
        let mut significant = None;

        for offset in 0..length {
            let address = leftmost + offset as u32;
            let value = memory.read(address as usize)?;
            if (1..=9).contains(&(value & 0x0f)) {
                significant = Some(address);
                break;
            }
            memory.write(address as usize, fill)?;
        }

        self.registers.arr = significant.unwrap_or(end + 1);
        Ok(())
    }

    fn compare_logical_characters(
        &mut self,
        memory: &Memory,
        instruction: &DecodedInstruction,
    ) -> Result<(), MspError> {
        let left = read_field(
            memory,
            required_address(instruction.operand1, instruction)?,
            instruction.q as usize + 1,
        )?;
        let right = read_field(
            memory,
            required_address(instruction.operand2, instruction)?,
            instruction.q as usize + 1,
        )?;
        self.set_comparison(left.cmp(&right));
        Ok(())
    }

    fn logical_arithmetic(
        &mut self,
        memory: &mut Memory,
        instruction: &DecodedInstruction,
        subtract: bool,
    ) -> Result<(), MspError> {
        let target = required_address(instruction.operand1, instruction)?;
        let source = required_address(instruction.operand2, instruction)?;
        let length = instruction.q as usize + 1;
        let left = read_field(memory, target, length)?;
        let right = read_field(memory, source, length)?;
        let mut result = vec![0; length];
        let mut carry_or_borrow = 0i16;

        if subtract {
            for index in (0..length).rev() {
                let difference = left[index] as i16 - right[index] as i16 - carry_or_borrow;
                if difference < 0 {
                    result[index] = (difference + 256) as u8;
                    carry_or_borrow = 1;
                } else {
                    result[index] = difference as u8;
                    carry_or_borrow = 0;
                }
            }
            self.set_comparison(left.cmp(&right));
        } else {
            for index in (0..length).rev() {
                let sum = left[index] as u16 + right[index] as u16 + carry_or_borrow as u16;
                result[index] = sum as u8;
                carry_or_borrow = (sum >> 8) as i16;
            }
            let result_zero = result.iter().all(|byte| *byte == 0);
            self.set_signed_condition(result_zero, false);
            if carry_or_borrow != 0 {
                self.registers.psr |= BINARY_OVERFLOW;
                if !result_zero {
                    self.set_condition_code(CC_HIGH);
                }
            } else if !result_zero {
                self.set_condition_code(CC_LOW);
            }
        }

        write_field(memory, target, &result)?;
        Ok(())
    }

    fn add_to_register(
        &mut self,
        memory: &Memory,
        instruction: &DecodedInstruction,
    ) -> Result<(), MspError> {
        let Some(old_value) = self.read_register(instruction.q, instruction.opcode, true)? else {
            return Ok(());
        };
        let address = required_address(instruction.operand1, instruction)?;
        let operand = u16::from_be_bytes(read_field(memory, address, 2)?.try_into().unwrap());
        let (result, carry) = old_value.overflowing_add(operand);
        self.write_register(instruction.q, result, instruction.opcode)?;
        let zero = result == 0;
        self.set_signed_condition(zero, false);
        if carry {
            self.registers.psr |= BINARY_OVERFLOW;
            if !zero {
                self.set_condition_code(CC_HIGH);
            }
        } else if !zero {
            self.set_condition_code(CC_LOW);
        }
        self.registers.arr = address;
        Ok(())
    }

    fn read_register(&self, q: u8, opcode: u8, for_add: bool) -> Result<Option<u16>, MspError> {
        let value = match q {
            0x00 => return Ok(None),
            0x01 => self.registers.index_registers[0],
            0x02 => self.registers.index_registers[1],
            0x04 if for_add => 0x0400 | (self.registers.psr & 0x00ff),
            0x04 => self.registers.psr,
            0x08 => self.registers.arr as u16,
            0x10 | 0x20 => self.registers.iar as u16,
            _ => return Err(MspError::InvalidRegisterCode { opcode, q }),
        };
        Ok(Some(value))
    }

    fn write_register(&mut self, q: u8, value: u16, opcode: u8) -> Result<(), MspError> {
        match q {
            0x00 => {}
            0x01 => self.registers.index_registers[0] = value,
            0x02 => self.registers.index_registers[1] = value,
            0x04 => self.load_psr(value),
            0x08 => self.registers.arr = value as u32,
            0x10 | 0x20 => self.registers.iar = value as u32,
            _ => return Err(MspError::InvalidRegisterCode { opcode, q }),
        }
        Ok(())
    }

    fn load_psr(&mut self, mut value: u16) {
        if value & CC_EQUAL != 0 {
            value &= !(CC_LOW | CC_HIGH);
        } else if value & CC_HIGH != 0 {
            value &= !(CC_LOW | CC_EQUAL);
        } else if value & CC_LOW != 0 {
            value &= !(CC_HIGH | CC_EQUAL);
        }
        self.registers.psr = value;
    }

    fn set_condition_code(&mut self, code: u16) {
        self.registers.psr = (self.registers.psr & !CONDITION_CODE_MASK) | code;
    }

    fn set_signed_condition(&mut self, zero: bool, negative: bool) {
        let code = if zero {
            CC_EQUAL
        } else if negative {
            CC_LOW
        } else {
            CC_HIGH
        };
        self.set_condition_code(code);
    }

    fn set_comparison(&mut self, ordering: std::cmp::Ordering) {
        let code = match ordering {
            std::cmp::Ordering::Equal => CC_EQUAL,
            std::cmp::Ordering::Less => CC_LOW,
            std::cmp::Ordering::Greater => CC_HIGH,
        };
        self.set_condition_code(code);
    }

    fn test_branch_condition(&mut self, q: u8) -> bool {
        let mask = (q & 0x3f) as u16;
        let condition_true = q & 0x80 != 0;
        let matched = self.registers.psr & mask != 0;
        let branch = if condition_true { matched } else { !matched };
        self.registers.psr &= !(mask & (DECIMAL_OVERFLOW | TEST_FALSE));
        branch
    }
}

fn required_address(
    address: Option<u32>,
    instruction: &DecodedInstruction,
) -> Result<u32, MspError> {
    address.ok_or(MspError::InvalidAddressingMode {
        address: instruction.address,
        opcode: instruction.opcode,
    })
}

fn field_range(memory: &Memory, rightmost: u32, length: usize) -> Result<Range<usize>, MspError> {
    if length == 0 {
        return Err(MspError::InvalidField {
            address: rightmost,
            length,
        });
    }
    let end = rightmost as usize;
    if end >= memory.len() {
        memory.read(end)?;
    }
    let after_end = end.checked_add(1).ok_or(MspError::InvalidField {
        address: rightmost,
        length,
    })?;
    let start = after_end
        .checked_sub(length)
        .ok_or(MspError::InvalidField {
            address: rightmost,
            length,
        })?;
    Ok(start..after_end)
}

fn validate_field(memory: &Memory, rightmost: u32, length: usize) -> Result<(), MspError> {
    field_range(memory, rightmost, length).map(|_| ())
}

fn read_field(memory: &Memory, rightmost: u32, length: usize) -> Result<Vec<u8>, MspError> {
    let range = field_range(memory, rightmost, length)?;
    range
        .map(|address| memory.read(address).map_err(MspError::from))
        .collect()
}

fn write_field(memory: &mut Memory, rightmost: u32, bytes: &[u8]) -> Result<(), MspError> {
    let range = field_range(memory, rightmost, bytes.len())?;
    for (address, value) in range.zip(bytes.iter().copied()) {
        memory.write(address, value)?;
    }
    Ok(())
}

fn is_negative_zoned(byte: u8) -> bool {
    matches!(byte >> 4, 0x0b | 0x0d)
}

fn decode_zoned(bytes: &[u8], rightmost: u32) -> Result<(Vec<u8>, bool), MspError> {
    let mut digits = Vec::with_capacity(bytes.len());
    for (index, byte) in bytes.iter().rev().copied().enumerate() {
        let digit = byte & 0x0f;
        if digit > 9 {
            return Err(MspError::InvalidDecimalDigit {
                address: rightmost - index as u32,
                value: byte,
            });
        }
        digits.push(digit);
    }
    let negative = bytes
        .last()
        .copied()
        .map(is_negative_zoned)
        .unwrap_or(false);
    Ok((digits, negative))
}

fn trim_decimal(digits: &mut Vec<u8>) {
    while digits.len() > 1 && digits.last() == Some(&0) {
        digits.pop();
    }
    if digits.is_empty() {
        digits.push(0);
    }
}

fn compare_magnitude(left: &[u8], right: &[u8]) -> std::cmp::Ordering {
    let mut left = left.to_vec();
    let mut right = right.to_vec();
    trim_decimal(&mut left);
    trim_decimal(&mut right);
    left.len()
        .cmp(&right.len())
        .then_with(|| left.iter().rev().cmp(right.iter().rev()))
}

fn add_magnitude(left: &[u8], right: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(left.len().max(right.len()) + 1);
    let mut carry = 0u8;
    for index in 0..left.len().max(right.len()) {
        let sum =
            left.get(index).copied().unwrap_or(0) + right.get(index).copied().unwrap_or(0) + carry;
        result.push(sum % 10);
        carry = sum / 10;
    }
    if carry != 0 {
        result.push(carry);
    }
    result
}

fn subtract_magnitude(larger: &[u8], smaller: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(larger.len());
    let mut borrow = 0i8;
    for index in 0..larger.len() {
        let mut difference = larger.get(index).copied().unwrap_or(0) as i8
            - smaller.get(index).copied().unwrap_or(0) as i8
            - borrow;
        if difference < 0 {
            difference += 10;
            borrow = 1;
        } else {
            borrow = 0;
        }
        result.push(difference as u8);
    }
    trim_decimal(&mut result);
    result
}

fn signed_decimal_add(
    left: Vec<u8>,
    left_negative: bool,
    right: Vec<u8>,
    right_negative: bool,
    result_length: usize,
) -> (Vec<u8>, bool, bool) {
    let (mut result, negative) = if left_negative == right_negative {
        (add_magnitude(&left, &right), left_negative)
    } else {
        match compare_magnitude(&left, &right) {
            std::cmp::Ordering::Greater => (subtract_magnitude(&left, &right), left_negative),
            std::cmp::Ordering::Less => (subtract_magnitude(&right, &left), right_negative),
            std::cmp::Ordering::Equal => (vec![0], false),
        }
    };
    trim_decimal(&mut result);
    let overflow =
        result.len() > result_length && result[result_length..].iter().any(|digit| *digit != 0);
    result.resize(result_length, 0);
    result.truncate(result_length);
    let is_zero = result.iter().all(|digit| *digit == 0);
    (result, negative && !is_zero, overflow)
}

#[cfg(test)]
mod tests {
    use super::{Msp, MspError, StepOutcome, BINARY_OVERFLOW, CC_EQUAL, CC_HIGH, CC_LOW};
    use crate::system34::memory::Memory;

    fn run_one(program: &[u8], memory_size_kib: usize) -> (Msp, Memory, StepOutcome) {
        let mut memory = Memory::new(memory_size_kib);
        memory.load(0, program).unwrap();
        let mut msp = Msp::default();
        let outcome = msp.step(&mut memory).unwrap();
        (msp, memory, outcome)
    }

    #[test]
    fn reset_sets_entry_point_and_clears_registers() {
        let mut msp = Msp::default();
        msp.registers.index_registers[0] = 0x1234;

        msp.reset(0x200);

        assert_eq!(msp.registers.iar, 0x200);
        assert_eq!(msp.registers.index_registers[0], 0);
    }

    #[test]
    fn executes_move_logical_immediate() {
        let (msp, memory, _) = run_one(&[0x3c, 0xa5, 0x02, 0x00], 1);

        assert_eq!(memory.read(0x0200), Ok(0xa5));
        assert_eq!(msp.registers.iar, 4);
    }

    #[test]
    fn executes_zoned_move_with_operand_length_difference() {
        let mut memory = Memory::new(1);
        memory
            .load(0, &[0x04, 0x10, 0x01, 0x00, 0x01, 0x10])
            .unwrap();
        memory.write(0x0110, 0xd5).unwrap();
        let mut msp = Msp::default();

        msp.step(&mut memory).unwrap();

        assert_eq!(memory.read(0x00ff), Ok(0xf0));
        assert_eq!(memory.read(0x0100), Ok(0xd5));
        assert_eq!(msp.registers.psr & CC_LOW, CC_LOW);
    }

    #[test]
    fn adds_logical_fields_and_sets_carry_and_zero() {
        let mut memory = Memory::new(1);
        memory
            .load(0, &[0x0e, 0x00, 0x01, 0x00, 0x01, 0x01])
            .unwrap();
        memory.write(0x0100, 0xfe).unwrap();
        memory.write(0x0101, 0x02).unwrap();
        let mut msp = Msp::default();
        msp.step(&mut memory).unwrap();

        assert_eq!(memory.read(0x0100), Ok(0x00));
        assert_eq!(
            msp.registers.psr & (CC_EQUAL | BINARY_OVERFLOW),
            CC_EQUAL | BINARY_OVERFLOW
        );
    }

    #[test]
    fn subtracts_logical_field_with_borrow() {
        let mut memory = Memory::new(1);
        memory
            .load(0, &[0x0f, 0x00, 0x01, 0x00, 0x01, 0x01])
            .unwrap();
        memory.write(0x0100, 2).unwrap();
        memory.write(0x0101, 3).unwrap();
        let mut msp = Msp::default();

        msp.step(&mut memory).unwrap();

        assert_eq!(memory.read(0x0100), Ok(0xff));
        assert_eq!(msp.registers.psr & super::CC_LOW, super::CC_LOW);
    }

    #[test]
    fn logical_moves_compare_and_nibble_move_work() {
        let mut memory = Memory::new(1);
        memory
            .load(0, &[0x0c, 0x01, 0x01, 0x00, 0x01, 0x10])
            .unwrap();
        memory.write(0x010f, 0x12).unwrap();
        memory.write(0x0110, 0x34).unwrap();
        let mut msp = Msp::default();
        msp.step(&mut memory).unwrap();
        assert_eq!(memory.read(0x00ff), Ok(0x12));
        assert_eq!(memory.read(0x0100), Ok(0x34));

        memory
            .load(0, &[0x0d, 0x01, 0x01, 0x00, 0x01, 0x00])
            .unwrap();
        msp.registers.iar = 0;
        msp.step(&mut memory).unwrap();
        assert_eq!(msp.registers.psr & CC_EQUAL, CC_EQUAL);

        memory
            .load(0, &[0x08, 0x01, 0x01, 0x00, 0x01, 0x10])
            .unwrap();
        memory.write(0x0100, 0x12).unwrap();
        memory.write(0x0110, 0xab).unwrap();
        msp.registers.iar = 0;
        msp.step(&mut memory).unwrap();
        assert_eq!(memory.read(0x0100), Ok(0xb2));
    }

    #[test]
    fn masked_operations_set_clear_and_test_bits() {
        let mut memory = Memory::new(1);
        let mut msp = Msp::default();

        memory.load(0, &[0x3a, 0x0f, 0x01, 0x00]).unwrap();
        memory.write(0x0100, 0xa0).unwrap();
        msp.step(&mut memory).unwrap();
        assert_eq!(memory.read(0x0100), Ok(0xaf));

        memory.load(0, &[0x3b, 0x30, 0x01, 0x00]).unwrap();
        memory.write(0x0100, 0xff).unwrap();
        msp.registers.iar = 0;
        msp.step(&mut memory).unwrap();
        assert_eq!(memory.read(0x0100), Ok(0xcf));

        memory.load(0, &[0x38, 0x0c, 0x01, 0x00]).unwrap();
        memory.write(0x0100, 0xf0).unwrap();
        msp.registers.iar = 0;
        msp.step(&mut memory).unwrap();
        assert_ne!(msp.registers.psr & super::TEST_FALSE, 0);

        memory.load(0, &[0x39, 0x03, 0x01, 0x00]).unwrap();
        memory.write(0x0100, 0xf0).unwrap();
        msp.registers.iar = 0;
        msp.registers.psr = 0;
        msp.step(&mut memory).unwrap();
        assert_eq!(msp.registers.psr & super::TEST_FALSE, 0);
    }

    #[test]
    fn register_load_store_add_and_load_address_work() {
        let mut memory = Memory::new(1);
        let mut msp = Msp::default();
        memory.load(0, &[0x35, 0x01, 0x01, 0x02]).unwrap();
        memory.write(0x0101, 0x12).unwrap();
        memory.write(0x0102, 0x34).unwrap();
        msp.step(&mut memory).unwrap();
        assert_eq!(msp.registers.index_registers[0], 0x1234);

        memory.load(0, &[0x34, 0x01, 0x01, 0x12]).unwrap();
        msp.registers.iar = 0;
        msp.step(&mut memory).unwrap();
        assert_eq!(memory.read(0x0111), Ok(0x12));
        assert_eq!(memory.read(0x0112), Ok(0x34));

        memory.load(0, &[0x36, 0x02, 0x01, 0x02]).unwrap();
        memory.write(0x0101, 0x00).unwrap();
        memory.write(0x0102, 0x02).unwrap();
        msp.registers.index_registers[1] = 1;
        msp.registers.iar = 0;
        msp.step(&mut memory).unwrap();
        assert_eq!(msp.registers.index_registers[1], 3);

        memory.load(0, &[0xc2, 0x01, 0x01, 0x23]).unwrap();
        msp.registers.iar = 0;
        msp.step(&mut memory).unwrap();
        assert_eq!(msp.registers.index_registers[0], 0x0123);
    }

    #[test]
    fn edit_and_insert_test_characters_update_fields_and_arr() {
        let mut memory = Memory::new(1);
        memory
            .load(0, &[0x0a, 0x01, 0x01, 0x02, 0x01, 0x12])
            .unwrap();
        memory.write(0x0101, 0x20).unwrap();
        memory.write(0x0102, 0x21).unwrap();
        memory.write(0x0111, 0xf2).unwrap();
        memory.write(0x0112, 0xf1).unwrap();
        let mut msp = Msp::default();
        msp.step(&mut memory).unwrap();
        assert_eq!(memory.read(0x0101), Ok(0xf1));
        assert_eq!(memory.read(0x0102), Ok(0x21));

        memory
            .load(0, &[0x0b, 0x02, 0x02, 0x00, 0x02, 0x10])
            .unwrap();
        memory.write(0x0200, 0xf0).unwrap();
        memory.write(0x0201, 0xf0).unwrap();
        memory.write(0x0202, 0xf3).unwrap();
        memory.write(0x0210, 0x40).unwrap();
        msp.registers.iar = 0;
        msp.step(&mut memory).unwrap();
        assert_eq!(memory.read(0x0200), Ok(0x40));
        assert_eq!(memory.read(0x0201), Ok(0x40));
        assert_eq!(msp.registers.arr, 0x0202);
    }

    #[test]
    fn branch_on_condition_branches_and_saves_sequential_address() {
        let (mut msp, _, _) = run_one(&[0xc0, 0x81, 0x00, 0x20], 1);
        msp.registers.psr = CC_EQUAL;
        // Re-execute the same instruction after setting its condition.
        msp.registers.iar = 0;
        let mut memory = Memory::new(1);
        memory.load(0, &[0xc0, 0x81, 0x00, 0x20]).unwrap();

        msp.step(&mut memory).unwrap();

        assert_eq!(msp.registers.iar, 0x20);
        assert_eq!(msp.registers.arr, 4);
    }

    #[test]
    fn jump_on_condition_adds_the_unsigned_displacement_to_iar() {
        let mut memory = Memory::new(1);
        memory.load(0, &[0xf2, 0x81, 0x10]).unwrap();
        let mut msp = Msp::default();
        msp.registers.psr = CC_EQUAL;

        msp.step(&mut memory).unwrap();

        assert_eq!(msp.registers.iar, 19);
        assert_eq!(msp.registers.arr, 0);
    }

    #[test]
    fn load_program_mode_register_updates_defined_bits() {
        let mut memory = Memory::new(1);
        memory.load(0, &[0xf6, 0x00, 0x51]).unwrap();
        let mut msp = Msp::default();

        msp.step(&mut memory).unwrap();

        assert_eq!(msp.registers.program_mode, 0x51);
    }

    #[test]
    fn add_zoned_decimal_sets_high_condition() {
        let mut memory = Memory::new(1);
        memory
            .load(0, &[0x06, 0x00, 0x01, 0x00, 0x01, 0x01])
            .unwrap();
        memory.write(0x0100, 0xf2).unwrap();
        memory.write(0x0101, 0xf3).unwrap();
        let mut msp = Msp::default();

        msp.step(&mut memory).unwrap();

        assert_eq!(memory.read(0x0100), Ok(0xf5));
        assert_eq!(memory.read(0x0101), Ok(0xf3));
        assert_eq!(msp.registers.psr & (CC_EQUAL | CC_HIGH), CC_HIGH);
    }

    #[test]
    fn subtract_zoned_decimal_produces_negative_zoned_result() {
        let mut memory = Memory::new(1);
        memory
            .load(0, &[0x07, 0x00, 0x01, 0x00, 0x01, 0x01])
            .unwrap();
        memory.write(0x0100, 0xf2).unwrap();
        memory.write(0x0101, 0xf8).unwrap();
        let mut msp = Msp::default();

        msp.step(&mut memory).unwrap();

        assert_eq!(memory.read(0x0100), Ok(0xd6));
        assert_eq!(msp.registers.psr & CC_LOW, CC_LOW);
    }

    #[test]
    fn compare_immediate_sets_equal_condition() {
        let mut memory = Memory::new(1);
        memory.load(0, &[0x3d, 0x42, 0x01, 0x00]).unwrap();
        memory.write(0x0100, 0x42).unwrap();
        let mut msp = Msp::default();

        msp.step(&mut memory).unwrap();

        assert_eq!(msp.registers.psr & CC_EQUAL, CC_EQUAL);
    }

    #[test]
    fn svc_stops_the_msp_until_control_processor_resumes_it() {
        let (mut msp, mut memory, outcome) = run_one(&[0xf4, 0x00, 0x00], 1);

        assert_eq!(
            outcome,
            StepOutcome::SupervisorCall {
                address: 0,
                interrupt_level: 5
            }
        );
        assert_eq!(
            msp.step(&mut memory),
            Ok(StepOutcome::WaitingForControlProcessor)
        );
        msp.resume_from_control_processor();
    }

    #[test]
    fn transfer_instruction_causes_a_program_check() {
        let mut memory = Memory::new(1);
        memory.load(0, &[0xf5, 0x00, 0x00]).unwrap();
        let mut msp = Msp::default();

        assert_eq!(
            msp.step(&mut memory),
            Err(MspError::ProgramCheck {
                address: 0,
                opcode: 0xf5,
            })
        );
    }

    #[test]
    fn load_program_mode_register_rejects_nonprivileged_caller() {
        let mut memory = Memory::new(1);
        memory.load(0, &[0xf6, 0x00, 0x10]).unwrap();
        let mut msp = Msp::default();
        msp.registers.program_mode = 0x80;

        assert!(matches!(
            msp.step(&mut memory),
            Err(MspError::PrivilegeViolation { .. })
        ));
    }
}

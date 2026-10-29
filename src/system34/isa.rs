use std::error::Error;
use std::fmt;

use super::memory::{Memory, MemoryError};
use super::msp::MspRegisters;

/// Operation codes implemented by the System/34 Main Storage Processor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    ZeroAndAddZoned,
    AddZoned,
    SubtractZoned,
    MoveHexCharacter,
    MoveCharacters,
    Edit,
    InsertAndTestCharacters,
    CompareLogicalCharacters,
    AddLogicalCharacters,
    SubtractLogicalCharacters,
    MoveImmediate,
    CompareLogicalImmediate,
    SetBitsOnMasked,
    SetBitsOffMasked,
    TestBitsOnMasked,
    TestBitsOffMasked,
    StoreRegister,
    LoadRegister,
    AddToRegister,
    BranchOnCondition,
    LoadAddress,
    SupervisorCall,
    Transfer,
    LoadProgramModeRegister,
    JumpOnCondition,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedInstruction {
    pub address: u32,
    pub opcode: u8,
    pub operation: Operation,
    pub q: u8,
    pub operand1: Option<u32>,
    pub operand2: Option<u32>,
    /// The third byte of command instructions (or the effective byte operand
    /// for single-byte operations such as MVI).
    pub parameter: Option<u8>,
    pub next_address: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    Memory(MemoryError),
    UnsupportedOpcode { address: u32, opcode: u8 },
    ProgramCounterOverflow,
    InvalidAddressingMode { address: u32, opcode: u8 },
}

impl fmt::Display for DecodeError {
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
        }
    }
}

impl Error for DecodeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Memory(error) => Some(error),
            _ => None,
        }
    }
}

impl From<MemoryError> for DecodeError {
    fn from(error: MemoryError) -> Self {
        Self::Memory(error)
    }
}

#[derive(Clone, Copy)]
enum Format {
    TwoAddress,
    OneAddressOperand1,
    OneAddressOperand2,
    Command,
}

fn classify(opcode: u8, address: u32) -> Result<(Operation, Format), DecodeError> {
    let high = opcode >> 4;
    let low = opcode & 0x0f;

    let result = if high == 0x0f {
        let operation = match low {
            0x02 => Operation::JumpOnCondition,
            0x04 => Operation::SupervisorCall,
            0x05 => Operation::Transfer,
            0x06 => Operation::LoadProgramModeRegister,
            _ => return Err(DecodeError::UnsupportedOpcode { address, opcode }),
        };
        (operation, Format::Command)
    } else if matches!(high, 0x03 | 0x07 | 0x0b) {
        let operation = match low {
            0x04 => Operation::StoreRegister,
            0x05 => Operation::LoadRegister,
            0x06 => Operation::AddToRegister,
            0x08 => Operation::TestBitsOnMasked,
            0x09 => Operation::TestBitsOffMasked,
            0x0a => Operation::SetBitsOnMasked,
            0x0b => Operation::SetBitsOffMasked,
            0x0c => Operation::MoveImmediate,
            0x0d => Operation::CompareLogicalImmediate,
            _ => return Err(DecodeError::UnsupportedOpcode { address, opcode }),
        };
        (operation, Format::OneAddressOperand1)
    } else if matches!(high, 0x0c..=0x0e) {
        let operation = match low {
            0x00 => Operation::BranchOnCondition,
            0x02 => Operation::LoadAddress,
            _ => return Err(DecodeError::UnsupportedOpcode { address, opcode }),
        };
        (operation, Format::OneAddressOperand2)
    } else if matches!(
        high,
        0x00 | 0x01 | 0x02 | 0x04 | 0x05 | 0x06 | 0x08 | 0x09 | 0x0a
    ) {
        let operation = match low {
            0x04 => Operation::ZeroAndAddZoned,
            0x06 => Operation::AddZoned,
            0x07 => Operation::SubtractZoned,
            0x08 => Operation::MoveHexCharacter,
            0x0a => Operation::Edit,
            0x0b => Operation::InsertAndTestCharacters,
            0x0c => Operation::MoveCharacters,
            0x0d => Operation::CompareLogicalCharacters,
            0x0e => Operation::AddLogicalCharacters,
            0x0f => Operation::SubtractLogicalCharacters,
            _ => return Err(DecodeError::UnsupportedOpcode { address, opcode }),
        };
        (operation, Format::TwoAddress)
    } else {
        return Err(DecodeError::UnsupportedOpcode { address, opcode });
    };

    Ok(result)
}

fn read_byte(memory: &Memory, cursor: &mut u32) -> Result<u8, DecodeError> {
    let value = memory.read(*cursor as usize)?;
    *cursor = cursor
        .checked_add(1)
        .ok_or(DecodeError::ProgramCounterOverflow)?;
    Ok(value)
}

fn mode_address(
    memory: &Memory,
    cursor: &mut u32,
    mode: u8,
    registers: &MspRegisters,
    instruction_address: u32,
    opcode: u8,
) -> Result<Option<u32>, DecodeError> {
    match mode {
        0 => {
            let high = read_byte(memory, cursor)?;
            let low = read_byte(memory, cursor)?;
            Ok(Some(u16::from_be_bytes([high, low]) as u32))
        }
        1 | 2 => {
            let displacement = read_byte(memory, cursor)? as u32;
            Ok(Some(
                registers.index_registers[(mode - 1) as usize] as u32 + displacement,
            ))
        }
        3 => Ok(None),
        _ => Err(DecodeError::InvalidAddressingMode {
            address: instruction_address,
            opcode,
        }),
    }
}

/// Decodes an instruction and resolves direct/indexed addresses. Direct
/// addresses and operand addresses denote the rightmost byte of each field;
/// ITC's operand-one address is interpreted by its executor as the leftmost.
pub fn decode(
    memory: &Memory,
    registers: &MspRegisters,
    address: u32,
) -> Result<DecodedInstruction, DecodeError> {
    let opcode = memory.read(address as usize)?;
    let (operation, format) = classify(opcode, address)?;
    let mut cursor = address
        .checked_add(1)
        .ok_or(DecodeError::ProgramCounterOverflow)?;
    let q = read_byte(memory, &mut cursor)?;
    let mut operand1 = None;
    let mut operand2 = None;
    let mut parameter = None;

    match format {
        Format::Command => {
            parameter = Some(read_byte(memory, &mut cursor)?);
        }
        Format::TwoAddress => {
            let high = opcode >> 4;
            operand1 = mode_address(memory, &mut cursor, high >> 2, registers, address, opcode)?;
            operand2 = mode_address(memory, &mut cursor, high & 0x03, registers, address, opcode)?;
        }
        Format::OneAddressOperand1 => {
            operand1 = mode_address(
                memory,
                &mut cursor,
                (opcode >> 4) >> 2,
                registers,
                address,
                opcode,
            )?;
        }
        Format::OneAddressOperand2 => {
            operand2 = mode_address(
                memory,
                &mut cursor,
                (opcode >> 4) & 0x03,
                registers,
                address,
                opcode,
            )?;
        }
    }

    Ok(DecodedInstruction {
        address,
        opcode,
        operation,
        q,
        operand1,
        operand2,
        parameter,
        next_address: cursor,
    })
}

#[cfg(test)]
mod tests {
    use super::{decode, Operation};
    use crate::system34::{memory::Memory, msp::MspRegisters};

    #[test]
    fn decodes_direct_two_operand_instruction() {
        let mut memory = Memory::new(1);
        memory
            .load(0, &[0x0d, 0x03, 0x01, 0x02, 0x03, 0x04])
            .unwrap();

        let instruction = decode(&memory, &MspRegisters::default(), 0).unwrap();

        assert_eq!(instruction.operation, Operation::CompareLogicalCharacters);
        assert_eq!(instruction.q, 3);
        assert_eq!(instruction.operand1, Some(0x0102));
        assert_eq!(instruction.operand2, Some(0x0304));
        assert_eq!(instruction.next_address, 6);
    }

    #[test]
    fn decodes_indexed_operands_from_opcode_nibbles() {
        let mut memory = Memory::new(1);
        memory.load(0, &[0x66, 0x00, 0x10, 0x20]).unwrap();
        let registers = MspRegisters {
            index_registers: [0x100, 0x200],
            ..MspRegisters::default()
        };

        let instruction = decode(&memory, &registers, 0).unwrap();

        assert_eq!(instruction.operation, Operation::AddZoned);
        assert_eq!(instruction.operand1, Some(0x110));
        assert_eq!(instruction.operand2, Some(0x220));
    }

    #[test]
    fn decodes_command_instruction_as_three_bytes() {
        let mut memory = Memory::new(1);
        memory.load(0, &[0xf6, 0xaa, 0x51]).unwrap();

        let instruction = decode(&memory, &MspRegisters::default(), 0).unwrap();

        assert_eq!(instruction.operation, Operation::LoadProgramModeRegister);
        assert_eq!(instruction.parameter, Some(0x51));
        assert_eq!(instruction.next_address, 3);
    }

    #[test]
    fn recognizes_the_complete_base_msp_opcode_set() {
        let cases = [
            (0x04, Operation::ZeroAndAddZoned, 6),
            (0x06, Operation::AddZoned, 6),
            (0x07, Operation::SubtractZoned, 6),
            (0x08, Operation::MoveHexCharacter, 6),
            (0x0a, Operation::Edit, 6),
            (0x0b, Operation::InsertAndTestCharacters, 6),
            (0x0c, Operation::MoveCharacters, 6),
            (0x0d, Operation::CompareLogicalCharacters, 6),
            (0x0e, Operation::AddLogicalCharacters, 6),
            (0x0f, Operation::SubtractLogicalCharacters, 6),
            (0x3c, Operation::MoveImmediate, 4),
            (0x3d, Operation::CompareLogicalImmediate, 4),
            (0x3a, Operation::SetBitsOnMasked, 4),
            (0x3b, Operation::SetBitsOffMasked, 4),
            (0x38, Operation::TestBitsOnMasked, 4),
            (0x39, Operation::TestBitsOffMasked, 4),
            (0x34, Operation::StoreRegister, 4),
            (0x35, Operation::LoadRegister, 4),
            (0x36, Operation::AddToRegister, 4),
            (0xc0, Operation::BranchOnCondition, 4),
            (0xc2, Operation::LoadAddress, 4),
            (0xf2, Operation::JumpOnCondition, 3),
            (0xf4, Operation::SupervisorCall, 3),
            (0xf5, Operation::Transfer, 3),
            (0xf6, Operation::LoadProgramModeRegister, 3),
        ];

        for (opcode, expected_operation, expected_length) in cases {
            let mut memory = Memory::new(1);
            memory.write(0, opcode).unwrap();
            let instruction = decode(&memory, &MspRegisters::default(), 0).unwrap();

            assert_eq!(
                instruction.operation, expected_operation,
                "opcode {opcode:#04x}"
            );
            assert_eq!(
                instruction.next_address, expected_length,
                "opcode {opcode:#04x}"
            );
        }
    }
}

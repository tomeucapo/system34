pub mod cp;
pub mod diskette;
pub mod ipl;
pub mod isa;
pub mod memory;
pub mod msp;
pub mod terminal;

use self::cp::ControlProcessor;
use self::diskette::{DisketteController, DisketteError};
use self::ipl::{IplError, IplState};
use self::memory::Memory;
use self::msp::{Msp, MspError, StepOutcome};
use self::terminal::OperatorConsole;

/// A configured IBM System/34 machine.
#[derive(Clone, Debug)]
pub struct System34 {
    memory: Memory,
    msp: Msp,
    cp: ControlProcessor,
    diskette: DisketteController,
    ipl_state: IplState,
    operator_console: OperatorConsole,
}

impl System34 {
    pub fn new(memory: Memory) -> Self {
        Self {
            memory,
            msp: Msp::new(),
            cp: ControlProcessor::default(),
            diskette: DisketteController::default(),
            ipl_state: IplState::NotStarted,
            operator_console: OperatorConsole::default(),
        }
    }

    pub fn memory(&self) -> &Memory {
        &self.memory
    }

    pub fn memory_mut(&mut self) -> &mut Memory {
        &mut self.memory
    }

    pub fn msp(&self) -> &Msp {
        &self.msp
    }

    pub fn msp_mut(&mut self) -> &mut Msp {
        &mut self.msp
    }

    pub fn cp(&self) -> &ControlProcessor {
        &self.cp
    }

    pub fn operator_console(&self) -> &OperatorConsole {
        &self.operator_console
    }

    pub fn operator_console_mut(&mut self) -> &mut OperatorConsole {
        &mut self.operator_console
    }

    pub fn diskette_controller(&self) -> &DisketteController {
        &self.diskette
    }

    pub fn diskette_controller_mut(&mut self) -> &mut DisketteController {
        &mut self.diskette
    }

    pub fn mount_diskette_imd(&mut self, bytes: &[u8]) -> Result<(), DisketteError> {
        self.diskette.mount_imd(bytes)
    }

    pub fn mount_diskette_imd_file(
        &mut self,
        path: impl AsRef<std::path::Path>,
    ) -> Result<(), DisketteError> {
        self.diskette.mount_imd_file(path)
    }

    pub fn ipl_state(&self) -> IplState {
        self.ipl_state
    }

    /// Performs the first documented CSIPL diskette transfer. The CP currently
    /// receives the bootstrap words; executing those microinstructions is a
    /// later step in the emulator.
    pub fn load_csipl_bootstrap_from_diskette(&mut self) -> Result<IplState, IplError> {
        let state = ipl::load_csipl_bootstrap(&mut self.diskette, &mut self.cp)?;
        self.ipl_state = state;
        Ok(state)
    }

    pub fn reset(&mut self, entry_point: u32) {
        self.msp.reset(entry_point);
        self.cp = ControlProcessor::default();
        self.ipl_state = IplState::NotStarted;
        self.operator_console = OperatorConsole::default();
    }

    /// Executes one MSP step.
    pub fn step(&mut self) -> Result<StepOutcome, MspError> {
        let outcome = self.msp.step(&mut self.memory)?;
        if let StepOutcome::SupervisorCall {
            interrupt_level, ..
        } = outcome
        {
            self.cp.request_interrupt(interrupt_level);
        }
        Ok(outcome)
    }

    /// Runs at most `max_steps` instructions and returns the number attempted.
    pub fn run_for(&mut self, max_steps: usize) -> Result<usize, MspError> {
        let mut attempted = 0;
        while attempted < max_steps {
            let outcome = self.step()?;
            attempted += 1;
            if !matches!(outcome, StepOutcome::Executed { .. }) {
                break;
            }
        }

        Ok(attempted)
    }
}

#[cfg(test)]
mod tests {
    use super::{ipl::IplState, memory::Memory, msp::StepOutcome, System34};

    #[test]
    fn machine_executes_a_valid_noop_register_instruction() {
        let mut memory = Memory::new(1);
        memory.load(0, &[0x34, 0x00, 0x00, 0x00]).unwrap();
        let mut machine = System34::new(memory);

        assert!(matches!(machine.step(), Ok(StepOutcome::Executed { .. })));
    }

    #[test]
    fn run_for_with_zero_steps_does_not_execute() {
        let mut machine = System34::new(Memory::new(1));

        assert_eq!(machine.run_for(0), Ok(0));
        assert_eq!(machine.msp().registers.iar, 0);
    }

    #[test]
    fn machine_loads_the_first_csipl_block_from_imd() {
        let mut image = b"IMD fixture\x1a".to_vec();
        image.extend_from_slice(&[0, 0, 0, 1, 5, 1, 1]);
        image.extend(std::iter::repeat_n(0xa5, 4096));
        let mut machine = System34::new(Memory::new(48));
        machine.mount_diskette_imd(&image).unwrap();

        assert_eq!(
            machine.load_csipl_bootstrap_from_diskette(),
            Ok(IplState::CsiplBootstrapLoaded {
                bytes: 4096,
                words: 2048,
            })
        );
        assert_eq!(machine.cp().control_storage[0], 0xa5a5);
    }
}

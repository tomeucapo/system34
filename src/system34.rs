pub mod cp;
pub mod isa;
pub mod memory;
pub mod msp;

use self::cp::ControlProcessor;
use self::memory::Memory;
use self::msp::{Msp, MspError, StepOutcome};

/// A configured IBM System/34 machine.
#[derive(Clone, Debug)]
pub struct System34 {
    memory: Memory,
    msp: Msp,
    cp: ControlProcessor,
}

impl System34 {
    pub fn new(memory: Memory) -> Self {
        Self {
            memory,
            msp: Msp::new(),
            cp: ControlProcessor::default(),
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

    pub fn reset(&mut self, entry_point: u32) {
        self.msp.reset(entry_point);
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
    use super::{memory::Memory, msp::StepOutcome, System34};

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
}

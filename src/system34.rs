pub mod cp;
pub mod channel;
pub mod disk;
pub mod diskette;
pub mod ipl;
pub mod isa;
pub mod memory;
pub mod msp;
pub mod terminal;
pub mod twinax;

use self::cp::ControlProcessor;
use self::channel::{ChannelTarget, CycleStealProgress, IoChannel};
use self::disk::{DiskGeometry, FixedDiskController, FixedDiskError};
use self::diskette::{DisketteController, DisketteError};
use self::ipl::{IplError, IplState};
use self::memory::Memory;
use self::msp::{Msp, MspError, StepOutcome};
use self::terminal::{OperatorConsole, TerminalKey};
use self::twinax::VirtualTwinaxBus;

/// A configured IBM System/34 machine.
#[derive(Clone, Debug)]
pub struct System34 {
    memory: Memory,
    msp: Msp,
    cp: ControlProcessor,
    channel: IoChannel,
    fixed_disk: FixedDiskController,
    diskette: DisketteController,
    ipl_state: IplState,
    operator_console: OperatorConsole,
    twinax_bus: VirtualTwinaxBus,
}

impl System34 {
    pub fn new(memory: Memory) -> Self {
        Self {
            memory,
            msp: Msp::new(),
            cp: ControlProcessor::default(),
            channel: IoChannel::default(),
            fixed_disk: FixedDiskController::default(),
            diskette: DisketteController::default(),
            ipl_state: IplState::NotStarted,
            operator_console: OperatorConsole::default(),
            twinax_bus: VirtualTwinaxBus::default(),
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

    pub fn io_channel(&self) -> &IoChannel {
        &self.channel
    }

    /// Performs one byte-wide cycle-steal service for the active channel transfer.
    pub fn cycle_steal(&mut self) -> Result<CycleStealProgress, channel::ChannelError> {
        let progress = self.channel.cycle_steal(&mut self.memory, &mut self.cp)?;
        if matches!(self.ipl_state, IplState::CsiplTransferInProgress { .. }) {
            self.ipl_state = if progress.complete {
                IplState::CsiplBootstrapLoaded {
                    bytes: progress.total,
                    words: progress.total / 2,
                }
            } else {
                IplState::CsiplTransferInProgress {
                    transferred: progress.transferred,
                    total: progress.total,
                }
            };
        }
        Ok(progress)
    }

    pub fn fixed_disk_controller(&self) -> &FixedDiskController {
        &self.fixed_disk
    }

    pub fn fixed_disk_controller_mut(&mut self) -> &mut FixedDiskController {
        &mut self.fixed_disk
    }

    pub fn mount_fixed_disk_raw_file(
        &mut self,
        path: impl AsRef<std::path::Path>,
        geometry: DiskGeometry,
    ) -> Result<(), FixedDiskError> {
        self.fixed_disk.mount_raw_file(path, geometry)
    }

    pub fn operator_console(&self) -> &OperatorConsole {
        &self.operator_console
    }

    pub fn operator_console_mut(&mut self) -> &mut OperatorConsole {
        &mut self.operator_console
    }

    pub fn virtual_twinax_bus(&self) -> &VirtualTwinaxBus {
        &self.twinax_bus
    }

    pub fn virtual_twinax_bus_mut(&mut self) -> &mut VirtualTwinaxBus {
        &mut self.twinax_bus
    }

    /// Simulates a host data transfer to the attached operator display.
    pub fn write_operator_display(&mut self, text: &str) -> bool {
        if !self.twinax_bus.send_to_station(text.as_bytes()) {
            return false;
        }
        self.operator_console.write_text(text);
        true
    }

    /// Simulates keyboard data returned by the operator station to the host.
    /// A successful keyboard transfer requests the workstation interrupt.
    pub fn receive_operator_keys(&mut self, keys: &[TerminalKey]) -> bool {
        if keys.is_empty() || !self.twinax_bus.receive_from_station(keys) {
            return false;
        }
        for key in keys.iter().copied() {
            self.operator_console.queue_key(key);
            if key == TerminalKey::Attention {
                self.twinax_bus.signal_attention();
            }
        }
        self.cp.request_interrupt(4);
        true
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
        self.begin_csipl_bootstrap_from_diskette()?;
        while self.channel.is_busy() {
            self.cycle_steal()?;
        }
        Ok(self.ipl_state)
    }

    /// Starts a byte-wise cycle-steal transfer of the diagnostic CSIPL block.
    pub fn begin_csipl_bootstrap_from_diskette(&mut self) -> Result<IplState, IplError> {
        let bytes = ipl::read_csipl_bootstrap(&mut self.diskette)?;
        self.channel.start_cycle_steal(
            bytes,
            ChannelTarget::ControlStorage { byte_address: 0 },
        )?;
        self.cp.begin_csipl_transfer();
        let total = self
            .channel
            .transfer_progress()
            .map_or(0, |progress| progress.total);
        self.ipl_state = IplState::CsiplTransferInProgress {
            transferred: 0,
            total,
        };
        Ok(self.ipl_state)
    }

    pub fn reset(&mut self, entry_point: u32) {
        self.msp.reset(entry_point);
        self.cp = ControlProcessor::default();
        self.channel = IoChannel::default();
        self.ipl_state = IplState::NotStarted;
        self.operator_console = OperatorConsole::default();
        self.twinax_bus = VirtualTwinaxBus::default();
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
    use super::{
        ipl::IplState, memory::Memory, msp::StepOutcome, terminal::TerminalKey, System34,
    };

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

    #[test]
    fn operator_console_io_is_attached_to_the_virtual_bus_and_cp_interrupts() {
        let mut machine = System34::new(Memory::new(48));

        assert!(machine.write_operator_display("HELLO"));
        assert_eq!(&machine.operator_console().screen()[0][..5], "HELLO");
        assert_eq!(machine.virtual_twinax_bus().host_bytes_sent(), 5);

        assert!(machine.receive_operator_keys(&[TerminalKey::Character('A')]));
        assert_eq!(
            machine.operator_console_mut().read_key(),
            Some(TerminalKey::Character('A'))
        );
        assert_eq!(machine.cp().pending_interrupt(), Some(4));
        assert_eq!(machine.virtual_twinax_bus().keyboard_events_received(), 1);

        machine.virtual_twinax_bus_mut().set_online(false);
        assert!(!machine.write_operator_display("offline"));
        assert!(!machine.receive_operator_keys(&[TerminalKey::Enter]));
    }

    #[test]
    fn csipl_bootstrap_moves_one_byte_per_cycle_steal_and_updates_state() {
        let mut image = b"IMD fixture\x1a".to_vec();
        image.extend_from_slice(&[0, 0, 0, 1, 5, 1, 1]);
        image.extend(std::iter::repeat_n(0x5a, 4096));
        let mut machine = System34::new(Memory::new(48));
        machine.mount_diskette_imd(&image).unwrap();

        assert_eq!(
            machine.begin_csipl_bootstrap_from_diskette(),
            Ok(IplState::CsiplTransferInProgress {
                transferred: 0,
                total: 4096,
            })
        );
        assert_eq!(machine.cycle_steal().unwrap().transferred, 1);
        assert_eq!(machine.cycle_steal().unwrap().transferred, 2);
        assert_eq!(machine.cp().csipl_words_loaded, 1);
        assert_eq!(
            machine.ipl_state(),
            IplState::CsiplTransferInProgress {
                transferred: 2,
                total: 4096,
            }
        );

        while machine.io_channel().is_busy() {
            machine.cycle_steal().unwrap();
        }
        assert_eq!(
            machine.ipl_state(),
            IplState::CsiplBootstrapLoaded {
                bytes: 4096,
                words: 2048,
            }
        );
        assert_eq!(machine.cp().control_storage[2047], 0x5a5a);
    }
}

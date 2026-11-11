pub mod channel;
pub mod cp;
pub mod disk;
pub mod diskette;
pub mod ipl;
pub mod isa;
pub mod memory;
pub mod msp;
pub mod terminal;
pub mod twinax;

use self::channel::{ChannelTarget, CycleStealProgress, IoChannel};
use self::cp::ControlProcessor;
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

    pub fn cp_mut(&mut self) -> &mut ControlProcessor {
        &mut self.cp
    }

    /// Fetches one control-store word through the CP's main-level MAR without
    /// executing it, for diagnostics and control-store inspection.
    pub fn fetch_control_store_word(&mut self) -> Result<(u16, u16), cp::ControlStorageError> {
        self.cp.fetch_microinstruction()
    }

    /// Fetches and executes one supported CP microinstruction.
    pub fn step_control_processor(&mut self) -> Result<cp::CpStepOutcome, cp::ControlStorageError> {
        self.cp.step_microinstruction()
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
        } else if let IplState::MainStorageTransferInProgress {
            total,
            load_address,
            entry_point,
            ..
        } = self.ipl_state
        {
            self.ipl_state = if progress.complete {
                self.msp.reset(entry_point);
                IplState::MainStorageProgramLoaded {
                    bytes: total,
                    load_address,
                    entry_point,
                }
            } else {
                IplState::MainStorageTransferInProgress {
                    transferred: progress.transferred,
                    total,
                    load_address,
                    entry_point,
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

    /// Starts a diskette-to-main-storage read serviced one byte per
    /// `cycle_steal` call.
    pub fn start_diskette_read(
        &mut self,
        cylinder: u8,
        head: u8,
        sector: u8,
        destination: usize,
    ) -> Result<(), channel::ChannelError> {
        let record = self.diskette.read_sector(cylinder, head, sector)?;
        let bytes = record
            .data()
            .ok_or(DisketteError::SectorUnavailable {
                cylinder,
                head,
                sector,
            })?
            .to_vec();
        self.channel.start_cycle_steal(
            bytes,
            ChannelTarget::MainStorage {
                byte_address: destination,
            },
        )
    }

    /// Starts a fixed-disk-to-main-storage read serviced one byte per
    /// `cycle_steal` call.
    pub fn start_fixed_disk_read(
        &mut self,
        cylinder: u16,
        head: u8,
        sector: u8,
        destination: usize,
    ) -> Result<(), channel::ChannelError> {
        let bytes = self
            .fixed_disk
            .read_sector(cylinder, head, sector)?
            .to_vec();
        self.channel.start_cycle_steal(
            bytes,
            ChannelTarget::MainStorage {
                byte_address: destination,
            },
        )
    }

    /// Starts an explicitly addressed raw program transfer from the fixed disk
    /// into main storage. This is a low-level loader primitive; the SSP directory,
    /// module header and normal CP-driven MSIPL sequence are not decoded here.
    pub fn begin_main_storage_program_load(
        &mut self,
        cylinder: u16,
        head: u8,
        sector: u8,
        length: usize,
        load_address: usize,
        entry_point: u32,
    ) -> Result<IplState, IplError> {
        let end = load_address
            .checked_add(length)
            .filter(|&end| end <= self.memory.len());
        if length == 0 || end.is_none() {
            return Err(IplError::InvalidMainStorageRange {
                load_address,
                length,
                storage_size: self.memory.len(),
            });
        }

        let bytes = self
            .fixed_disk
            .read_bytes_from(cylinder, head, sector, length)?
            .to_vec();
        self.channel.start_cycle_steal(
            bytes,
            ChannelTarget::MainStorage {
                byte_address: load_address,
            },
        )?;
        self.ipl_state = IplState::MainStorageTransferInProgress {
            transferred: 0,
            total: length,
            load_address,
            entry_point,
        };
        Ok(self.ipl_state)
    }

    /// Completes an explicitly addressed raw fixed-disk program transfer.
    /// This is useful for testing programs after MSIPL has selected an image.
    pub fn load_main_storage_program_from_fixed_disk(
        &mut self,
        cylinder: u16,
        head: u8,
        sector: u8,
        length: usize,
        load_address: usize,
        entry_point: u32,
    ) -> Result<IplState, IplError> {
        self.begin_main_storage_program_load(
            cylinder,
            head,
            sector,
            length,
            load_address,
            entry_point,
        )?;
        while self.channel.is_busy() {
            self.cycle_steal()?;
        }
        Ok(self.ipl_state)
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
        self.channel
            .start_cycle_steal(bytes, ChannelTarget::ControlStorage { byte_address: 0 })?;
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
    use super::{ipl::IplState, memory::Memory, msp::StepOutcome, terminal::TerminalKey, System34};

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
        assert_eq!(machine.fetch_control_store_word(), Ok((0, 0xa5a5)));
        assert_eq!(machine.cp().microaddress(), 1);
    }

    #[test]
    fn operator_console_io_is_attached_to_the_virtual_bus_and_cp_interrupts() {
        let mut machine = System34::new(Memory::new(48));

        assert!(machine.write_operator_display("HELLO"));
        assert_eq!(
            &machine.operator_console().screen()[0][..5],
            &['H', 'E', 'L', 'L', 'O']
        );
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

    #[test]
    fn diskette_reads_use_the_channel_cycle_steal_path() {
        let mut image = b"IMD channel fixture\x1a".to_vec();
        image.extend_from_slice(&[0, 1, 0, 1, 0, 1, 1]);
        image.extend(std::iter::repeat_n(0x3c, 128));
        let mut machine = System34::new(Memory::new(48));
        machine.mount_diskette_imd(&image).unwrap();

        machine.start_diskette_read(1, 0, 1, 0x100).unwrap();
        assert_eq!(machine.memory().read(0x100), Ok(0));
        machine.cycle_steal().unwrap();
        assert_eq!(machine.memory().read(0x100), Ok(0x3c));
        while machine.io_channel().is_busy() {
            machine.cycle_steal().unwrap();
        }
        assert_eq!(machine.memory().read(0x17f), Ok(0x3c));
    }

    #[test]
    fn fixed_disk_reads_use_the_channel_cycle_steal_path() {
        use super::disk::{DiskGeometry, FixedDiskImage};

        let geometry = DiskGeometry::new(1, 1, 2, 128).unwrap();
        let mut image = FixedDiskImage::blank(geometry).unwrap();
        image.write_sector(0, 0, 1, &[0xa7; 128]).unwrap();
        let mut machine = System34::new(Memory::new(48));
        machine.fixed_disk_controller_mut().mount(image);

        machine.start_fixed_disk_read(0, 0, 1, 0x200).unwrap();
        while machine.io_channel().is_busy() {
            machine.cycle_steal().unwrap();
        }
        assert_eq!(machine.memory().read(0x200), Ok(0xa7));
        assert_eq!(machine.memory().read(0x27f), Ok(0xa7));
    }

    #[test]
    fn raw_fixed_disk_program_load_places_bytes_and_sets_the_msp_entry_point() {
        use super::disk::{DiskGeometry, FixedDiskImage};

        let geometry = DiskGeometry::new(1, 1, 2, 128).unwrap();
        let mut image = FixedDiskImage::blank(geometry).unwrap();
        let mut boot_sector = [0; 128];
        boot_sector[..4].copy_from_slice(&[0x34, 0, 0, 0]);
        image.write_sector(0, 0, 0, &boot_sector).unwrap();
        let mut machine = System34::new(Memory::new(48));
        machine.fixed_disk_controller_mut().mount(image);

        assert_eq!(
            machine.load_main_storage_program_from_fixed_disk(0, 0, 0, 4, 0, 0),
            Ok(IplState::MainStorageProgramLoaded {
                bytes: 4,
                load_address: 0,
                entry_point: 0,
            })
        );
        assert_eq!(machine.memory().read(0), Ok(0x34));
        assert_eq!(machine.msp().registers.iar, 0);
        assert_eq!(machine.run_for(1), Ok(1));
    }
}

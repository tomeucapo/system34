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
    pending_interrupt: Option<u8>,
}

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
            pending_interrupt: None,
        }
    }
}

impl ControlProcessor {
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

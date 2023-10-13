// Control Processor 

pub struct ControlProcessor
{
    sar: usize,             // Storage Address Register
    sdr: u16,               // Storage Data Register
    mor: u16,               // Micro operation Register
    x: u16,
    y: u16,
    pcr: u16,               // Processor Condition Register       
    lsr:[char;64],          // Local Storage Register

    storage:[u16; 16384]    // Main memory
}

impl ControlProcessor 
{
    pub fn new() -> ControlProcessor
    {
        ControlProcessor { sar: 0, sdr: 0, mor: 0, x: 0, y: 0, pcr: 0, lsr: ['\0'; 64], storage: [0; 16384] }
    }

    fn execute_instruction(&mut self, instr: u16) 
    {
        let opcode = (instr >> 12) & 0x000F;
    }

    pub fn run()
    {

    }
}

// Main Storage Processor 

struct LPMR 
{
    opcode: u8,
    qbyte: u8,
    rbyte: u8
}

pub struct Msp
{
    msar: usize,            // Storage Address Register
 
    x: u16,                 // X-Register (X-Low X-High)
    y: u8,                  // Y-Register
    opreg: u8,              // Operation Register
    qbackup: u8,            // Q-Backup Register
    q: u8,                  // Q-Register
    psr: u8,                // Program Status Register
    sbr: [u8; 4],           // Status Byte Registers
    ccr: u8,                // Configuration Control Register (8-bit)
    acr: u32,               // Address Compare Register (17-bit) 

    memory: Memory,
}

impl Msp 
{
    pub fn new(memSize: usize) -> Msp
    {
        Msp { 
            msar: 0, 
            acr: 0, 
            ccr:0, 
            opreg: 0, 
            x: 0,
            y: 0,
            q: 0,
            memory: Memory::new(memSize) 
        }
    }

    fn execute_instruction(&mut self, instr: u16) 
    {
       
    }

    pub fn run()
    {

    }
}
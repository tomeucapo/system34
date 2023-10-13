// Main Storage Processor 

struct LPMR 
{
    opcode: u8,
    qbyte: u8,
    rbyte: u8
}

pub struct MainStorageProc
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

    storage:[u8; 32768]    // Main storage
}

impl MainStorageProc 
{
    pub fn new() -> MainStorageProc
    {
        MainStorageProc { msar: 0, lpmr: LPMR { opcode: '\0', qbyte: '\0', rbyte: '\0' }, storage: ['\0'; 32768] }
    }

    fn execute_instruction(&mut self, instr: u16) 
    {
       
    }

    pub fn run()
    {

    }
}
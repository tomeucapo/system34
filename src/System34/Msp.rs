// Main Storage Processor 

//use std::{cell::RefCell, rc::Rc};
use super::memory;

pub struct msp
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
    iar: usize,             // Instruction Address Register
    lsr: u8,            
    memory: memory::memory
}

impl msp 
{
    pub fn new(mem: memory::memory) -> msp
    {
        msp { 
            msar: 0,
            iar: 0, 
            lsr: 0,
            acr: 0, 
            ccr:0, 
            opreg: 0, 
            x: 0,
            y: 0,
            psr: 0,
            q: 0,
            sbr: [0; 4],
            memory: mem,
            qbackup: 0 
        }
    }

    
    fn execute_instruction(&mut self)
    {   
           /* 
            match self.opreg {
                0xc2 =>
                0xd2 =>
                0xe2 =>
                _ => Some();
            }
            */
    }

    pub fn run(&mut self)
    {
        while true 
        {
            self.msar = self.iar;            
            self.iar += 1;
            self.opreg = memory::memory::read(&self.memory, self.msar);
            self.lsr = self.opreg;
            self.msar = self.iar;
            self.iar += 1;
            self.q = memory::memory::read(&self.memory, self.msar);
            self.qbackup = self.q;
            //self.q = self.lsr;
            self.y = 1;
            match self.opreg {
                0xc2 | 0xd2 | 0xe2 | 0xc0 | 0xd0 | 0xe0 | 0xf2 => self.execute_instruction(),
                _ => {
                    self.msar = self.iar;
                    self.iar += 1;
                    // Executable instruction?

                }
            }
        }
    }

}

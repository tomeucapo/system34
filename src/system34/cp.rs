// Control Processor 

//use std::{cell::RefCell, rc::Rc};
//use crate::System34::Msp::Msp;

pub struct cp
{
    sar: usize,             // Storage Address Register
    sdr: u16,               // Storage Data Register
    mor: u16,               // Micro operation Register
    x: u16,
    y: u16,
    pcr: u16,               // Processor Condition Register       
    lsr:[u8; 64]            // Local Storage Register
}


impl cp
{
    pub fn new() -> cp
    {
        cp { sar: 0, sdr: 0, mor: 0, x: 0, y: 0, pcr: 0, lsr: [0; 64] }
    }

    fn execute_instruction(&mut self, instr: u16) 
    {
        let opcode = (instr >> 12) & 0x000F;
    }

     fn run()
    {

    }
}

mod msp;
mod cp;
pub mod memory;

pub struct system34
{
    msp: msp::msp,
    csp: cp::cp
}

impl system34
{
    pub fn new(mem: memory::memory) -> system34
    {
        system34 { 
            msp: msp::msp::new(mem.into()),
            csp: cp::cp::new()
        }
    }

    pub fn run(&mut self)
    {
        msp::msp::run(&mut self.msp);
    }
}
mod Msp;
mod Cp;

struct System34
{
    msp: Msp,
    csp: Cp
}

impl System34
{
    fn new(memSize: usize) -> System34
    {
        let msp = Msp::new(memSize);
        let csp = Cp::new(msp);
        System34 { msp, csp }
    }
}
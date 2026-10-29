# IBM System/34 Emulator

Rust emulator project for the IBM System/34, based on IBM's technical
documentation available from [Bitsavers](https://bitsavers.org/pdf/ibm/system34/).

## Current status

The project implements decoding and functional execution for the 25 base
Main Storage Processor (MSP) machine instructions, with single-step execution,
registers, indexed/direct addressing, status conditions, and bounded main
storage. `SVC` raises a modeled Control Processor interrupt request and suspends
the MSP; `XFER` reports the program check specified by IBM's reference.

Control Processor microinstruction execution, cycle-accurate timing, address
translation/protection, I/O channels and optional scientific microinstructions
are not implemented yet. Unsupported opcodes and invalid data accesses are
reported with structured errors.

Implementation references:

- [System/34 Functions Reference, SA21-9243](https://bitsavers.org/pdf/ibm/system34/SA21-9243-4_IBM_System_34_Functions_Reference_5th_ed_198209.pdf)
- [Basic Assembler and Macro Processor Reference, SC21-7705-3](https://bitsavers.org/pdf/ibm/system34/SC21-7705-3_IBM_System_34_Basic_Assembler_and_Macro_Processor_Reference_Manual_4th_ed_198201.pdf)

Run the current tests with:

```sh
cargo test
```

# IBM System/34 Emulator

Rust emulator project for the IBM System/34, based on IBM's technical
documentation available from [Bitsavers](https://bitsavers.org/pdf/ibm/system34/).

## Current status

The project implements decoding and functional execution for the 25 base
Main Storage Processor (MSP) machine instructions, with single-step execution,
registers, indexed/direct addressing, status conditions, and bounded main
storage. `SVC` raises a modeled Control Processor interrupt request and suspends
the MSP; `XFER` reports the program check specified by IBM's reference.

Diskette images in ImageDisk (`.IMD`) format can be mounted and read by physical
CHS or System/34 sequential sector address. The parser preserves deleted-record,
CRC-error and unavailable-sector markers. The first CSIPL step loads the 4096
byte C0/H0/R1 bootstrap record into the first 2048 Control Storage words.
Use `cargo run -- path/to/diskette.IMD` to inspect an image's IMD comment,
track count, and sector count.

The machine also exposes a local 5251 operator-console model: the default
Model 11 screen is 24x80, Model 1 is 12x80, and each console has a cursor,
screen buffer, and queued keyboard events. `TerminalEmulator` renders that
screen with ANSI escape sequences and translates host terminal input (text,
Enter, editing/control keys, and cursor keys) into console key events. The
console is available through `System34::operator_console()` and
`System34::operator_console_mut()`.

Try the standalone local console and type a few lines with:

```sh
cargo run -- --terminal-demo
```

Enter `/quit` to leave the demo. It exercises the 5251 screen, ANSI renderer,
and keyboard-input decoder without requiring an IPL image.

The CSIPL bootstrap is loaded but not executed yet: Control Processor
microinstructions and the channel/cycle-steal I/O protocol are still needed to
continue CSIPL, perform MSIPL, and connect running programs to the operator
console. The local ANSI view models the console display and keyboard; twinax
electrical/protocol emulation is not implemented. Cycle-accurate diskette
timing, address translation/protection, and optional scientific
microinstructions are also not implemented. Unsupported opcodes and invalid
data accesses return structured errors.

Implementation references:

- [System/34 Functions Reference, SA21-9243](https://bitsavers.org/pdf/ibm/system34/SA21-9243-4_IBM_System_34_Functions_Reference_5th_ed_198209.pdf)
- [Basic Assembler and Macro Processor Reference, SC21-7705-3](https://bitsavers.org/pdf/ibm/system34/SC21-7705-3_IBM_System_34_Basic_Assembler_and_Macro_Processor_Reference_Manual_4th_ed_198201.pdf)
- [System Unit Theory Diagrams, diskette and control-processor sections](https://bitsavers.org/pdf/ibm/system34/fe/SY31-0458-3_System_34_5340_System_Unit_Theory_Diagrams_Manual_Jul79/)
- [System Support Program Logic Manual, LY21-0050](https://bitsavers.org/pdf/ibm/system34/plm/LY21-0050-0_System_34_System_Support_Program_Logic_Manual_System_Dec77.pdf)

Run the current tests with:

```sh
cargo test
```

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

Check several diskette images for a structurally valid CSIPL bootstrap record
at C0/H0/R1 with:

```sh
cargo run -- --inspect-csipl image1.IMD image2.IMD
```

This verifies the record is present, not deleted, has no CRC error, and is
4,096 bytes. If an IMD image marks that sector as unavailable, its data payload
was not captured; that alone does not establish whether the original disk had
CSIPL data there. This distinction matters for diagnostic diskettes: IBM's
System/34 theory manual documents 4,096-byte FM records on track 00. The
bootstrap is not yet executed by the emulator.

The machine also exposes a local 5251 operator-console model: the default
Model 11 screen is 24x80, Model 1 is 12x80, and each console has a cursor,
screen buffer, and queued keyboard events. `TerminalEmulator` renders that
screen with ANSI escape sequences and translates host terminal input (text,
Enter, editing/control keys, and cursor keys) into console key events. The
console is available through `System34::operator_console()` and
`System34::operator_console_mut()`.

Launch the native graphical 5251 window with:

```sh
cargo run --bin system34-terminal
```

The window draws the 24x80 CRT using the `3270font` typeface, rasterized for the
screen, with a scanline effect. That font is derived from the x3270 bitmap
font, itself transcribed from an IBM 3270-series terminal. Its upstream BSD
license and attribution are in `assets/`. The virtual Twinax panel simulates
station online/selection, polling, host-to-screen transfers, keyboard returns,
attention and check indicators. This is a logical workstation-link simulation;
it does not model physical Twinax signaling or timing. Controls in the window
can send display text and keyboard events in either direction.

Try the standalone local console and type a few lines with:

```sh
cargo run -- --terminal-demo
```

Enter `/quit` to leave the demo. It exercises the 5251 screen, ANSI renderer,
and keyboard-input decoder without requiring an IPL image.

The CSIPL bootstrap is loaded but not executed yet: Control Processor
microinstructions and the channel/cycle-steal I/O protocol are still needed to
continue CSIPL, perform MSIPL, and connect running programs to the operator
console. The virtual Twinax bus is not yet wired to the Control Processor's
workstation attachment. Cycle-accurate Twinax/diskette timing, address
translation/protection, and optional scientific microinstructions are also
not implemented. Unsupported opcodes and invalid data accesses return
structured errors.

Implementation references:

- [System/34 Functions Reference, SA21-9243](https://bitsavers.org/pdf/ibm/system34/SA21-9243-4_IBM_System_34_Functions_Reference_5th_ed_198209.pdf)
- [Basic Assembler and Macro Processor Reference, SC21-7705-3](https://bitsavers.org/pdf/ibm/system34/SC21-7705-3_IBM_System_34_Basic_Assembler_and_Macro_Processor_Reference_Manual_4th_ed_198201.pdf)
- [System Unit Theory Diagrams, diskette and control-processor sections](https://bitsavers.org/pdf/ibm/system34/fe/SY31-0458-3_System_34_5340_System_Unit_Theory_Diagrams_Manual_Jul79/)
- [System/34 Theory Diagrams, Diskette 1 / Diskette 2D formats, including diagnostic 4,096-byte records](https://bitsavers.org/pdf/ibm/system34/fe/SY-31-0458-6_IBM_System_34_Theory_Diagrams_Manual_Volume_1_198201/08_33FD_53FD_Diskette_Attachment.pdf)
- [System Support Program Logic Manual, LY21-0050](https://bitsavers.org/pdf/ibm/system34/plm/LY21-0050-0_System_34_System_Support_Program_Logic_Manual_System_Dec77.pdf)

Run the current tests with:

```sh
cargo test
```

# Fanticon Cartridge Projects

This document defines the v0.1 project, assembly, build, launch, and debugger
workflow. Raw `.BIN` builds remain available for individual source files; a
project build produces a validated `.FCN` cartridge.

## Project manifest

A cartridge project is a directory containing `FANTICON.CFG`. The manifest is
plain ASCII text with one case-insensitive `KEY=VALUE` assignment per line.
Blank lines and lines beginning with `;` are ignored.

```text
TITLE=MY GAME
ID=0123456789ABCDEF
MAIN=MAIN.ASM
OUTPUT=MYGAME.FCN
SAVE_BANKS=0
MACHINE=1.0
AUTHOR=YOUR NAME
DESCRIPTION=A SHORT DESCRIPTION FOR WEB EXPORTS
ICON=ICON.PNG
WEB_BACKGROUND=#08080C
WEB_FOREGROUND=#EEEEEE
```

| Key | Rule |
| --- | --- |
| `TITLE` | 1-22 printable ASCII characters; spaces allowed |
| `ID` | Exactly 16 hexadecimal digits and not zero |
| `MAIN` | 8.3 path to the root assembly source |
| `OUTPUT` | 8.3 `.FCN` output filename |
| `SAVE_BANKS` | 0-4 battery-backed 16 KiB banks |
| `MACHINE` | Required Fanticon hardware version; `1.0` for v0.1 |
| `AUTHOR` | Optional 1-64 character export author |
| `DESCRIPTION` | Optional 1-160 character export description |
| `ICON` | Optional 8.3 PNG path used by PWA, Windows, Linux, and macOS packages |
| `WEB_BACKGROUND` | Optional HTML shell color in `#RRGGBB`; defaults to `#08080C` |
| `WEB_FOREGROUND` | Optional HTML shell color in `#RRGGBB`; defaults to `#EEEEEE` |

Unknown keys are build errors in manifest version 1. `NEW` creates the manifest
and generates a cryptographically random, nonzero 64-bit ID once. Ordinary
builds never change it. Copying a project intentionally copies its save identity;
a future `NEWID` command may explicitly detach a copy.

## ROM sections

Cartridge assembly adds two explicit section directives to the macro assembler:

```asm
        BANK  0
        ORG   $8000
        ; switchable code or data

        FIXED
        ORG   $C100
RESET   ; always-visible startup code

        ORG   $FFFA
        DA    NMI,RESET,IRQ
```

`BANK 0-255` selects a 16 KiB switchable image whose CPU addresses are
`$8000-$BFFF`. `FIXED` selects the 16 KiB logical image for `$C000-$FFFF`; the
hidden I/O-page bytes `$C000-$C0FF` remain `$FF`. Every unwritten ROM byte is
`$FF`.

Code or data crossing its selected section, overlapping earlier output, writing
the hidden I/O page, selecting an invalid bank, or omitting any CPU vector is a
build error. RESET must target fixed ROM or main RAM. Reset code cannot assume a
switchable bank other than bank 0.

`REQUIRE_FIXED` asserts that `FIXED` is the currently selected section. It emits
no bytes. Under `BANK n`, or before any section has been selected, it produces a
build error at its source line. The standard `EMIT_VRAM_COPY` and
`EMIT_PAD_SCROLL` macros in `FANTICON.INC` contain this assertion, preventing a
generated procedure from accidentally landing in switchable ROM. Raw `.BIN`
assembly has no cartridge sections, so `REQUIRE_FIXED` is a no-op there.

## Bank-aware symbols

Every label records its CPU address and section identity. Label names remain
globally unique across the root source and every included file.

```asm
        LDA   #BANKOF(LEVEL2)
        STA   BANK_NUMBER
        JMP   LEVEL2
```

`BANKOF(label)` returns 0-255 for a switchable-ROM label. It is an error for
fixed-ROM, RAM, or absolute numeric expressions. Referring to a banked label
returns its `$8000-$BFFF` CPU address but never inserts a bank switch. Direct
branches or jumps between different switchable banks produce a warning.

The build fails on unresolved symbols, unsupported relocation, or bank overflow.
Its symbol output includes section and bank alongside every address so the
debugger can distinguish labels that occupy the same CPU address in different
banks.

## Packing

The packager writes the fixed image followed by every bank from zero through the
highest bank referenced by source. Skipped banks and unwritten bytes are `$FF`;
trailing banks above the highest reference are omitted. It then writes counts,
machine requirements, title, ID, flags, and CRCs into the 64-byte `.FCN` header.

Using banks 0 and 3 therefore stores four banks. This preserves stable hardware
bank numbers without padding every cartridge to 4 MiB.

## Commands and launch behavior

| Action | Behavior |
| --- | --- |
| `BUILD` | Build `FANTICON.CFG` in the current directory |
| `RUN` | Build and launch the current project |
| `RUN GAME.FCN` | Validate and launch an existing cartridge |
| Editor **Build & Run** | Build the open project and launch on success |
| `fanticon-app GAME.FCN` | Launch a cartridge directly from the host command line |

Build or validation errors leave Fanticon in Editor mode and use the existing
diagnostic dialog. A game launched from Editor mode returns to the terminal when
Escape is pressed, after flushing and ejecting its cartridge. When Fanticon is
launched directly with a cartridge, Escape does nothing.

Controller 1 defaults to arrow keys, Z for A, X for B, Space for Select, and
Enter for Start. Controller 2 has no default keyboard binding and uses a second
gamepad unless the host is configured later.

## Required v0.1 debugger

The development debugger provides:

- CPU instruction breakpoints;
- memory-read and memory-write watchpoints;
- single-instruction and single-CPU-cycle stepping;
- CPU register, stack, current bank, IRQ, raster, and APU inspection;
- raster-position breakpoints; and
- complete VM pause without state advancement.

Deterministic input recording and replay files are deferred beyond v0.1.

### Editor debugger controls

Build & Run starts an editor-owned debug session. The VM normally runs at full
speed and returns to the editor automatically when a breakpoint, watchpoint,
raster breakpoint, or CPU jam is encountered. The VM remains completely frozen
while the editor is visible; rendering the debugger does not advance CPU,
video, timer, input, or audio state.

| Key | Debug action |
| --- | --- |
| `F5` | Build & Run when stopped; continue when paused |
| `F6` | Pause a running editor-owned game |
| `Shift+F5` | Stop the debug session |
| `F9` | Toggle a source breakpoint on the current line |
| `F10` | Step over `JSR`, or step one instruction otherwise |
| `F11` | Step one 6502 instruction |
| `Shift+F11` | Run until the current subroutine returns |
| `Ctrl/Cmd+F11` | Step one CPU/bus cycle |

The two-character source gutter shows `@` for a breakpoint and an arrow for the
currently paused source line. Breakpoints placed on labels or other
non-emitting lines resolve to the next emitted line in that source file.

The **Debug** menu also creates 16-bit memory-read and memory-write watchpoints,
and raster breakpoints entered as `LINE,DOT`. Debugger raster stops occur at CPU
cycle boundaries, which are every two video dots in machine 1.0.

The paused panel reports CPU registers and flags, stack bytes, memory around the
program counter, current mapped bank, IRQ state, raster position, every APU
channel's control/timer state, current audio output, stop reason, and the eight
most recently entered instructions.

### Source-map behavior

The assembler records an address and byte length for every emitting source
line, including lines originating in `PUT` files and macro expansions. Cartridge
entries additionally record `FIXED` or the exact `BANK` number. These maps stay
with the active editor build and are not stored inside the `.FCN`, so launching
an existing cartridge directly remains independent of development metadata.

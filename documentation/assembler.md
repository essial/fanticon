# Fanticon 6502 Macro Assembler

Fanticon includes a native two-pass assembler for writing programs for its NMOS
6502 VM. It accepts Merlin-inspired source, expands includes and macros, resolves
labels, and writes a raw `.BIN` file. The binary contains only emitted bytes: the
`ORG` address is reported by the build but is not stored as a header.

## Building

At the Fanticon command prompt, build a source file with either command:

```text
BUILD GAME.ASM
ASM GAME.ASM
```

The default output replaces the source extension with `.BIN`. Supply a second
8.3 path to choose it explicitly:

```text
BUILD GAME.ASM CART.BIN
```

In the editor, open or save an `.ASM` or `.INC` file and use **Build > Assemble**,
F5, or Ctrl+B. Command+B is also accepted on macOS. The editor assembles the
current buffer, including unsaved changes. The editor displays a progress dialog
while assembling, followed by a success dialog containing the output name,
origin, and byte count.

On failure, no new binary is written. A build-error dialog shows the diagnostic,
the editor moves to its source line, and affected lines are colored red. Use F4
or **Build > Next Error** to move forward;
Shift+F4 or **Build > Prev Error** moves backward. Editing clears the now-stale
diagnostics. The command prompt prints every diagnostic as:

```text
source:line:column message
```

## Source layout

Source is case-insensitive, while string contents are preserved. The editor uses
Merlin-style fields: label at column 1, operation at column 10, operand at column
16, and comment at column 27. Whitespace-separated source is also accepted.
A semicolon starts a comment outside a quoted string. A line beginning with `*`
or `;` is a full-line comment.

```asm
         ORG   $8000
START    LDX   #0
LOOP     LDA   MESSAGE,X
         BEQ   DONE
         INX
         BNE   LOOP
DONE     RTS
MESSAGE  ASC   "HELLO"
         DFB   0
         END
```

Labels are global, case-insensitive symbols. A label may end in a colon. Branch
operands are target addresses; the assembler calculates and validates the signed
-128 through +127 displacement. Known values in `$00` through `$FF` use a zero
page encoding when the instruction provides one. Forward operands conservatively
use absolute encoding, so instruction sizes remain stable between passes.

## Numbers and expressions

The assembler accepts decimal integers, `$` or `0x` hexadecimal, `%` binary, and
single-character literals. `*` is the address at the current source line.

Unary operators are `+`, `-`, `~`, `<` (low byte), and `>` (high byte). Binary
operators, from higher to lower precedence, are `* /`, `+ -`, `<< >>`, `&`, `^`,
and `|`. Parentheses override precedence.

```asm
SCREEN   EQU   $2000
COUNT    EQU   16
         LDA   #<SCREEN
         LDY   #>SCREEN
         DFB   'A',%10101010,COUNT-1
         DW    SCREEN,SCREEN+40
```

`EQU` and directives that determine layout must be resolvable when encountered;
define those constants before use. Ordinary instruction and data operands may
refer to labels defined later.

## Addressing modes

All official NMOS 6502 instructions and their valid addressing modes are
supported. Write operands in standard 6502 form:

| Mode | Example |
| --- | --- |
| Implied | `RTS` |
| Accumulator | `ASL A` |
| Immediate | `LDA #$20` |
| Zero page or absolute | `LDA ADDRESS` |
| Indexed | `LDA ADDRESS,X` or `LDX ADDRESS,Y` |
| Indirect jump | `JMP (VECTOR)` |
| Indexed indirect | `LDA (POINTER,X)` |
| Indirect indexed | `LDA (POINTER),Y` |
| Relative | `BNE LABEL` |

The assembler rejects an addressing mode that the selected instruction does not
implement. Undocumented opcodes are not currently emitted by this assembler,
even though Fanticon's CPU can execute them.

## Directives

| Directive | Purpose |
| --- | --- |
| `ORG expression` | Set the address for following output |
| `name EQU expression` / `EQ` | Define a constant |
| `DFB`, `DB`, `BYTE` | Emit comma-separated bytes or strings |
| `DW`, `DA`, `WORD` | Emit comma-separated 16-bit values, low byte first |
| `ASC`, `TEXT` | Emit quoted ASCII strings or byte expressions |
| `HEX` | Emit hexadecimal byte pairs, with spaces or commas allowed |
| `DS expression` | Reserve and zero-fill a number of bytes |
| `PUT`, `USE`, `INCLUDE` | Assemble another text file at this location |
| `END` | Mark the logical end of source |

Multiple `ORG` regions are allowed only in increasing address order. Gaps are
zero-filled in the raw output. Moving backward over output is an error. Includes
use Fanticon paths and may be nested up to 16 levels.

## Merlin-style macros

Define a macro by placing its name in the label field and `MAC` in the operation
field. End it with `EOM` or `<<<`. Parameters `]1` through `]8` are substituted
when the macro expands.

```asm
LOADIMM  MAC
         LDA   #]1
         STA   ]2
         EOM

         ORG   $8000
         PMC   LOADIMM;$42;$20
```

`PMC name;arg1;arg2` and `>>> name;arg1;arg2` invoke a macro. A macro name may
also be used directly in the operation field. Macro expansion may nest up to 32
levels. Duplicate macro names, unterminated definitions, invalid expanded source,
and unknown macro calls are reported with their source location.

## Output and limits

Output may span at most the 6502's 64 KiB address space. Byte and immediate
values must fit 8 bits, word and address values must fit 16 bits, and branch
targets must be in range. The symbol table and origin are retained by the Rust
assembler API, while the Fanticon build command writes only the program bytes.

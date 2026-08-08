# Fanticon 6502 Macro Assembler

Fanticon supports both raw `.BIN` assembly and project cartridge builds. The
`FANTICON.CFG`, `BANK`, `FIXED`, and `BANKOF` extensions are described in the
[Fanticon Cartridge Projects](cartridge-projects.md) guide.

Fanticon includes a native two-pass assembler for writing programs for its NMOS
6502 VM. It accepts Merlin-inspired source, expands includes and macros, resolves
labels, and writes a raw `.BIN` file. The binary contains only emitted bytes: the
`ORG` address is reported by the build but is not stored as a header.

## Building

At the Fanticon command prompt, assemble a raw source file with `ASM`:

```text
ASM GAME.ASM
```

The default output replaces the source extension with `.BIN`. Supply a second
8.3 path to choose it explicitly:

```text
ASM GAME.ASM CART.BIN
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

With a `FANTICON.CFG` in the current directory, `BUILD` packages the complete
project as a validated `.FCN`; `RUN` builds and launches it. **Build > Build &
Run** or Shift+F5 performs the same operation from the editor. `RUN GAME.FCN`
validates and launches an existing image.

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
`|`, and finally the comparisons `<`, `>`, `<=`, `>=`, `==`, and `!=`.
Comparisons produce 1 when true and 0 when false. Parentheses override precedence.

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
implement. It also supports Fanticon's stable NMOS 6502 undocumented mnemonics:
`SLO`, `RLA`, `SRE`, `RRA`, `SAX`, `LAX`, `DCP`, `ISC`/`ISB`, `ANC`, `ALR`,
`ARR`, `XAA`, `AXS`/`SBX`, `AHX`, `SHY`, `SHX`, `TAS`, `LAS`, and `KIL`/`JAM`.
Addressed undocumented `NOP` forms are accepted as well.

Some undocumented operations have several opcode bytes with identical syntax.
The assembler emits one stable canonical byte for each form: `$02` for
`KIL`/`JAM`, `$0B` for `ANC #`, and `$80`, `$04`, `$14`, `$0C`, or `$1C` for
the addressed `NOP` modes. Use `DFB` or `HEX` when software needs a particular
duplicate byte. These instructions are NMOS-specific and may behave differently
or be reassigned on later 6502-family CPUs.

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
| `IF` / `DO`, `ELSE`, `ENDIF` / `FIN` | Select source at assembly time |
| `REPEAT` / `LUP`, `ENDREP` / `--^` | Repeat source at assembly time |
| `name PROC`, `ENDPROC` | Scope dot-prefixed local labels to a procedure |
| `name DUM origin`, `DEND` | Define a symbol-only data layout |
| `END` | Mark the logical end of source |

Multiple `ORG` regions are allowed only in increasing address order. Gaps are
zero-filled in the raw output. Moving backward over output is an error. Includes
use Fanticon paths and may be nested up to 16 levels.

## Modern macros

Define a macro by placing its name in the label field and `MAC` in the operation
field. End it with `EOM` or `<<<`. Legacy definitions with no parameter list keep
supporting positional parameters `]1` through `]8`:

```asm
LOADIMM  MAC
         LDA   #]1
         STA   ]2
         EOM

         ORG   $8000
         PMC   LOADIMM;$42;$20
```

`PMC name;arg1;arg2` and `>>> name;arg1;arg2` invoke a macro. A macro name may
also be used directly in the operation field. `PMC` is recommended for
semicolon-separated calls because it makes the argument list unambiguous to the
editor.

### Named parameters and defaults

Place a semicolon- or comma-separated parameter list after `MAC`. Named macros
accept up to 32 parameters. A trailing parameter may provide a default with
`NAME=value`; every later parameter must also have a default. Named parameters
are referenced as `]NAME`. Their positional aliases (`]1`, `]2`, and so on) also
remain available.

```asm
STORE    MAC   VALUE;DEST=$20
         LDA   #]VALUE
         STA   ]DEST
         EOM

         PMC   STORE;$42       ; uses default destination $20
         PMC   STORE;$18;$30
```

An omitted or empty argument uses its default. Missing required arguments,
extra arguments, duplicate parameter names, and required parameters following a
default are errors at the invocation or definition line.

### Hygienic local labels

A label beginning with `@` inside a macro is private to that expansion. Fanticon
rewrites its definition and every reference to a unique internal symbol, including
inside nested macro calls. The same macro can therefore be invoked repeatedly
without caller-supplied label suffixes.

```asm
WAITX    MAC   COUNT
         LDX   #]COUNT
@LOOP    DEX
         BNE   @LOOP
         EOM

         PMC   WAITX;8
         PMC   WAITX;16
```

Macro expansion may nest up to 32 levels. Substitution does not alter quoted
strings or comments. Duplicate macro names, unterminated definitions, invalid
expanded source, and unknown macro calls are reported with their source location.

## Compile-time conditionals

`IF expression` (or Merlin-compatible `DO expression`) includes its block when
the expression is nonzero. `ELSE` selects the other branch and `ENDIF` (or `FIN`)
closes the block. Blocks may nest. Expressions can use constants defined by an
earlier `EQU`; an unresolved condition is an error.

```asm
DEBUG    EQU   1

         IF    DEBUG & $01
         LDA   #$E0
         ELSE
         LDA   #0
         ENDIF
```

Use the assembler's bitwise `&` and `|` operators rather than C-style `&&` and
`||`.

## Compile-time repetition

`REPEAT count;INDEX` (or `LUP count;INDEX`) expands its body `count` times and
ends with `ENDREP` (or `--^`). `]INDEX` is the zero-based iteration number;
`]#` is a short alias. Repeat blocks may nest, use different index names, and
contain macros or conditionals. The count must be resolved when encountered and
must be from 0 through 65,535.

```asm
         REPEAT 8;PAGE
         STA   $A000+]PAGE*$100,X
         ENDREP
```

## Procedure scopes

`name PROC` defines the procedure's ordinary global entry label and opens a
scope. Within it, labels beginning with `.` are qualified by the procedure name.
`ENDPROC` closes the scope. Procedures cannot nest, and the directives emit no
automatic prologue, epilogue, register saves, or `RTS`.

```asm
UPDATE   PROC
.LOOP    DEX
         BNE   .LOOP
         RTS
         ENDPROC
```

The example defines `UPDATE` and `UPDATE.LOOP`. A second procedure may use its
own `.LOOP` without a collision.

## Symbol-only data layouts

`name DUM origin` opens a dummy layout that assigns addresses without emitting
bytes. Each entry is a labeled `DS size`, `EQU`, or `EQ`. A named layout prefixes
every field and defines `name.SIZE` at `DEND`; omit the name for traditional
global dummy labels. Sizes and constants must resolve when encountered.

```asm
PLAYER   DUM   0
X        DS    2
Y        DS    1
FLAGS    DS    1
         DEND

HERO     DS    PLAYER.SIZE
         LDA   HERO+PLAYER.Y
```

This defines `PLAYER.X=0`, `PLAYER.Y=2`, `PLAYER.FLAGS=3`, and
`PLAYER.SIZE=4`, then allocates one instance at `HERO`.

## Output and limits

Output may span at most the 6502's 64 KiB address space. Byte and immediate
values must fit 8 bits, word and address values must fit 16 bits, and branch
targets must be in range. The symbol table and origin are retained by the Rust
assembler API, while the Fanticon build command writes only the program bytes.

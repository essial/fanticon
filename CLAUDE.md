# Fanticon — writing games as an agent

Fanticon is a 6502 fantasy console (NES-shaped: 320×200 indexed video, tile/
sprite/bitmap graphics, four-voice audio). Games are written in 6502
assembly against a fixed hardware contract and packaged as `.FCN`
cartridges. This file is a condensed, code-writing-oriented reference —
read it instead of the full `documentation/` set for day-to-day game work.
It does not cover the editor UI, exporting, or engine internals; see
[Other docs](#other-docs) for those.

## Fast build/check loop

The interactive app (`NEW PROJECT` / `BUILD` / `RUN`) is the only way a
human iterates, but there's also a headless CLI for agent iteration —
assemble a project, print diagnostics, exit nonzero on error, with no GUI:

```sh
cargo run --bin fanticon-check -- path/to/project --check-only   # assemble only, no .FCN written
cargo run --bin fanticon-check -- path/to/project                # assemble and write the .FCN
cargo run --bin fanticon-check -- path/to/file.asm --check-only  # raw (non-project) source
```

A project is any directory containing `fanticon.cfg`. Errors print as
`source:line:column: message`, one per line — the same source names and
positions the in-app editor would show. Use `--check-only` in a tight
edit/check loop; drop it once you want the actual `.FCN` on disk. This
tool only assembles — it doesn't run the game, so it can't catch runtime
logic bugs (see [Verifying gameplay](#verifying-gameplay)).

## Minimal project

A cartridge project is a directory with `fanticon.cfg`:

```
TITLE=MY GAME
ID=0123456789ABCDEF
MAIN=MAIN.ASM
OUTPUT=MYGAME.FCN
SAVE_BANKS=0
MACHINE=1.0
```

`TITLE` is 1-22 printable ASCII characters. `ID` is 16 nonzero hex digits
(generate any random nonzero value; the app's `NEW` command normally does
this). `MAIN`/`OUTPUT` are 8.3 paths. `SAVE_BANKS` is 0-4 (battery-backed
16 KiB banks). `MACHINE` must be `1.0`. Unknown keys are a build error.
Optional keys: `AUTHOR`, `DESCRIPTION`, `ICON`, `WEB_BACKGROUND`,
`WEB_FOREGROUND` — see [cartridge-projects.md](documentation/cartridge-projects.md).

Smallest valid `MAIN.ASM`:

```asm
         INCLUDE FANTICON.INC

         FIXED
         ORG   FIXED_ROM
RESET    LDA   #RGB332_RED
         STA   BACKDROP_COLOR
LOOP     JMP   LOOP
NMI      RTI
IRQ      RTI
         ORG   VECTOR_NMI
         DA    NMI,RESET,IRQ
```

`INCLUDE FANTICON.INC` should be the first line of every `MAIN.ASM` — it's
a reserved, built-in include (works with no file on disk) supplying every
hardware name and macro referenced below. `FIXED` selects the
$C000-$FFFF fixed-ROM image; `BANK 0-255` selects a switchable 16 KiB
image at $8000-$BFFF for larger games. RESET must target fixed ROM or
main RAM. Every unwritten ROM byte is `$FF`, and the hidden I/O page
`$C000-$C0FF` inside `FIXED` can never be written from source.

## Assembler cheatsheet

Two-pass Merlin-style macro assembler ([full spec](documentation/assembler.md)).

- `ORG expr` — set output address; multiple ORGs must move forward only,
  gaps zero-fill.
- `name EQU expr` — constant; must resolve where it's defined (no forward
  refs for layout-affecting values).
- `DFB`/`DB`/`BYTE`, `DW`/`DA`/`WORD` (16-bit, low byte first — this is
  the directive the vector table uses), `ASC`/`TEXT` (quoted strings),
  `HEX` (hex byte pairs), `DS expr` (reserve + zero-fill).
- `INCLUDE path` — nested to 16 levels.
- `IF expr` / `ELSE` / `FIN` (or `ENDIF`) — compile-time conditional.
- `REPEAT count;INDEX` / `ENDREP` — compile-time repeat; `]INDEX` is the
  0-based iteration number inside the block.
- `name MAC PARAM1;PARAM2=default` … `EOM` — define a macro. Named,
  optionally-defaulted parameters (defaults must trail, up to 32),
  referenced in the body as `]PARAM`. Invoke with `PMC name;arg1;arg2`.
- `@LABEL` inside a macro body — hygienic private label, rewritten to a
  unique symbol per expansion (nests to 32 levels). Real example from
  `fanticon.inc`: `@COPY LDA ]SOURCE,X ... BNE @COPY`.
- `name PROC` … `ENDPROC` — scope for `.LABEL` dot-prefixed local labels,
  qualified externally as `ProcName.Label`.
- `name DUM origin` … `DEND` — symbol-only struct-like layout, also
  defines `name.SIZE`.
- `REQUIRE_FIXED` — build error unless the current section is `FIXED`;
  no-op outside cartridge projects. `EMIT_VRAM_COPY`/`EMIT_PAD_SCROLL`
  (below) both start with it.
- `BANKOF(label)` — resolves to 0-255 for a switchable-ROM label only;
  never inserts a bank switch itself. Direct branches/jumps across
  different banks produce a warning — switch via `BANK_NUMBER` first.
- Addressing: `LDA #$20` immediate, `LDA ADDR` zp/absolute (auto-picked),
  `LDA ADDR,X` / `LDX ADDR,Y` indexed, `JMP (VECTOR)` indirect,
  `LDA (PTR,X)` / `LDA (PTR),Y` indexed indirect / indirect indexed,
  `BNE LABEL` relative (range-checked). Numbers: `$`/`0x` hex, `%`
  binary, `'A'` char literal, `*` = current address.
- All 256 opcodes assemble, including the stable NMOS undocumented ones
  (`SLO`, `RLA`, `LAX`, `SAX`, `DCP`, `ISC`, `ANC`, `ALR`, `ARR`, `XAA`,
  `AXS`, `AHX`, `SHY`, `SHX`, `TAS`, `LAS`, `KIL`).

## FANTICON.INC reference

Everything below is a symbol or macro from the built-in include — don't
redefine these names. Full source: [code-assets/fanticon.inc](code-assets/fanticon.inc).

**Core addresses**: `MAIN_RAM=$0000`…`$7FFF`, `BANK_WINDOW=$8000`,
`IO_PAGE=$C000`, `FIXED_ROM=$C100`, `VECTOR_NMI=$FFFA`,
`VECTOR_RESET=$FFFC`, `VECTOR_IRQ=$FFFE`. `BANK_KIND=$C000` /
`BANK_NUMBER=$C001` select the banked window (`BANK_CARTRIDGE=0`,
`BANK_WORK_RAM=1`, `BANK_VRAM=2`, `BANK_SAVE_RAM=3`).

**Video/VRAM**: `VIDEO_MODE=$C010` (`VIDEO_BLANK`/`VIDEO_TILEMAP`/
`VIDEO_BITMAP` = 0/1/2), `VIDEO_CONTROL` layer mask (`VIDEO_BG=$01`,
`VIDEO_SPRITES=$02`, `VIDEO_ALL=$03`), `VIDEO_STATUS=$C01E`
(`STATUS_VBLANK=$01`, `STATUS_HBLANK=$02`, `STATUS_SPROVER=$04`). VRAM
(bank-relative): `VRAM_TILE_DATA=$0000` (8 KiB, 32 bytes/tile, 4bpp
packed, high nibble = left pixel), `VRAM_TILE_MAP=$2000` (64×32 tile
indices, one byte/cell, `y*64+x`), `VRAM_TILE_ATTR=$2800` (64×32
attribute bytes), `VRAM_SPRITES=$3000` (32 × 8-byte records),
`VRAM_BITMAP=$4000`. Mapped through the CPU window when VRAM bank 0 is
selected: `VRAM_TILE_CPU=$8000`, `VRAM_MAP_CPU=$A000`,
`VRAM_ATTR_CPU=$A800`, `VRAM_SPR_CPU=$B000`. Scroll: `SCROLL_X_LOW/HIGH`,
`SCROLL_Y_LOW/HIGH` are signed 16-bit, wrap modulo 512×256 (the tile map
is a 512×256px ring buffer behind a fixed 320×200 viewport). Palette:
`PALETTE_INDEX=$C01B` / `PALETTE_DATA=$C01C` (auto-increments both
directions, wraps at 255), `BITMAP_PALETTE=$C01D`. Only three RGB332
component constants ship: `RGB332_RED=$E0`, `RGB332_GREEN=$1C`,
`RGB332_BLUE=$03` — there's no full color table, compose or `SET_COLOR`
your own palette.

**Tile attribute byte**: bit6 `TILE_PRIORITY` (foreground), bit5
`TILE_VFLIP`, bit4 `TILE_HFLIP`, bits3-0 `TILE_PAL_MASK` (palette bank).

**Sprite record** (8 bytes, offsets `SPR_X_LOW/X_FLAGS/Y/TILE/ATTR` =
0/1/2/3/4, bytes 5-7 reserved): byte0 X low; byte1 bit0 = X bit8 (9-bit
X), bit1 = behind-background; byte2 = signed Y; byte3 = base tile number
(16×16 sprites need 4 consecutive tiles and the number must be divisible
by 4); byte4 bits0-3 palette bank, bit4 `SPR_HFLIP`, bit5 `SPR_VFLIP`,
bit6 = 16×16 size, bit7 `SPR_ENABLE`. Hardware limit: 32 records total, 8
drawn per scanline — a 9th candidate on one line is dropped silently and
sets `STATUS_SPROVER`; lower record numbers win ties.

**Audio**: `PULSE1_CONTROL=$C030`…`AUDIO_MASTER=$C040`, `AUDIO_ENABLE=$80`,
duty constants `PULSE_DUTY_12_5/25/50/75`. See [audio.md](documentation/audio.md)
for the full four-voice register set.

**Controllers**: `PAD0_STATE=$C050` (held), `PAD0_PRESSED=$C051` (edges;
read clears the bits it returns), `PAD1_STATE=$C052`, `PAD1_PRESSED=$C053`.
Bits: `PAD_UP=$01`, `PAD_DOWN=$02`, `PAD_LEFT=$04`, `PAD_RIGHT=$08`,
`PAD_A=$10`, `PAD_B=$20`, `PAD_SELECT=$40`, `PAD_START=$80`. Sampled once
per frame; a disconnected pad reads all-released.

**Timers**: `TIMER0_RELOADL=$C060`…`TIMER1_COUNTH=$C06C`,
`TIMER_ENABLE=$01`, `TIMER_REPEAT=$02`.

**Interrupts**: `IRQ_PENDING=$C002` / `IRQ_ENABLE=$C003`, bit0
`IRQ_VBLANK`, bit1 `IRQ_RASTER`, bit2/3 the two timers. Multiple sources
OR onto one line-level IRQ — a handler must check and ack every source it
enabled.

**Macros** (`PMC name;arg1;arg2` to invoke): `SET_BANK KIND;NUMBER`,
`ACK_IRQ MASK`, `SET_IRQS MASK`, `SET_VIDEO MODE;LAYERS=VIDEO_BG`,
`SET_BITMAP PALETTE;LAYERS=VIDEO_BG`, `SET_BACKDROP COLOR`,
`SET_SCROLL X;Y`, `SET_RASTER X;Y`, `SET_COLOR INDEX;COLOR`,
`UPLOAD_TILE INDEX;SOURCE` (copies a 32-byte pattern, clobbers A/X),
`FILL_TILEMAP TILE;ATTR`, `SET_SPRITE INDEX;X;Y;TILE;ATTR;FLAGS=0`,
`SET_TONE`, `SET_NOISE`, `SET_AUDIO_MASTER VOLUME;ENABLE=AUDIO_ENABLE`,
`SILENCE_AUDIO`, `START_TIMER`/`STOP_TIMER`, `READ_FRAME16 DEST`,
`READ_TIMER16`, `WAIT_VBLANK` (spins until `VIDEO_STATUS` shows
`STATUS_VBLANK`), `WAIT_NEXT_VBLANK` (waits for the next rising edge),
`PUSH_BANK`/`POP_BANK`, `PUSH_AXY`/`POP_YXA`, `STORE16`/`COPY16 DEST;VALUE`,
`ADD16`/`SUB16 ADDRESS;VALUE`, `INC16`/`DEC16 ADDRESS`. There's no hardware
entropy source (the noise channel's LFSR is audio-only) — `SEED_RANDOM SEED`
/ `NEXT_RANDOM SEED` run an 8-bit Galois LFSR (tap `$1D`, maximal period
255) over a caller-owned RAM byte instead: `SEED_RANDOM` mixes `FRAME_LOW`
with `PAD0_STATE` and guarantees a nonzero seed, `NEXT_RANDOM` advances one
step and leaves the new byte in A. Deterministic for a given input
recording — call `NEXT_RANDOM` once per frame from the VBlank handler. Two
emitters generate a whole named subroutine (call once from `FIXED`, then
`JSR NAME`): `EMIT_VRAM_COPY NAME;SRC;DST;LEN;BUF` and
`EMIT_PAD_SCROLL NAME;X;Y;PAD=PAD0_STATE`.

## Game loop pattern

**Vsync comes from IRQ, not NMI.** Fanticon's hardware never drives the
NMI line in v0.1 — every demo's `NMI` handler is just `RTI` to satisfy the
vector table. Frame timing and input are driven by `IRQ_VBLANK`. The
standard shape, taken directly from the tile-scrolling demo
(`code-assets/demos/tiles/main.asm`):

```asm
         INCLUDE FANTICON.INC
         FIXED
         ORG   $C100
RESET    SEI
         ; one-time setup: select VRAM bank, upload tile patterns,
         ; fill the tile map, set VIDEO_MODE / VIDEO_CONTROL, ...
         LDA   #IRQ_VBLANK
         STA   IRQ_ENABLE
         CLI
IDLE     JMP   IDLE            ; all real work happens in IRQ below

IRQ      PHA
         ; read PAD0_STATE, update game state, write sprite/scroll
         ; registers directly — this runs once per frame
         PMC   ACK_IRQ;IRQ_VBLANK
         PLA
         RTI
NMI      RTI

         ORG   $FFFA
         DA    NMI,RESET,IRQ
```

Push/restore X and Y too (`PUSH_AXY`/`POP_YXA`) if the handler touches
them. Sprite records are read fresh at the start of each scanline, so
writes made inside the vblank IRQ land cleanly before the next frame
renders.

## Hard rules (build errors, not warnings, unless noted)

- Code/data that crosses its selected section, overlaps earlier output,
  writes the hidden I/O page, selects an invalid bank, or omits any CPU
  vector — build error.
- `RESET` must target fixed ROM or main RAM, never a switchable bank
  other than bank 0.
- `ORG` must move forward only; moving backward over already-emitted
  bytes is an error.
- `EQU` and other layout-affecting constants must resolve at the point
  they're defined — no forward references for those (ordinary instruction
  operands may still forward-reference labels).
- `REQUIRE_FIXED` fails the build if the current section isn't `FIXED`.
- `BANKOF(label)` is only valid for switchable-ROM labels — fixed-ROM,
  RAM, or absolute-numeric labels are an error.
- Direct branch/jump across two different `BANK` sections — warning, not
  a hard error, but switch via `BANK_NUMBER` + an indirect/JSR-through
  pattern instead of relying on it.
- Macro calls: missing a required argument, passing extra arguments,
  duplicate parameter names, or a required parameter after a defaulted
  one — all build errors.
- Unknown `fanticon.cfg` keys are a build error.
- `INCLUDE FANTICON.INC` is safe to repeat (from main source and from a
  child include) — it only expands once per assembly.
- **Silent, not a build error:** a label sharing a line with a `PMC` macro
  invocation (`LOOP PMC NEXT_RANDOM;$20`) is dropped — the macro expands,
  but the label is never defined, so a later reference to it fails with an
  unrelated-looking "unresolved expression" error somewhere else. Always
  put a label that a macro call needs on its own line above the call.

## Verifying gameplay

`fanticon-check` only proves the source assembles — it can't tell you a
sprite moves the right way or a collision check is correct. To see the
game actually run, use the interactive app (`cargo run --release`, then
`RUN` from the console, or `cargo run --release -- /path/to/GAME.FCN`).
There's no headless frame-stepping harness for cartridge logic today;
`cargo bench --no-default-features --bench vm` runs fixed demo cartridges
for performance measurement, not gameplay assertions.

## Other docs

Full topic-by-topic references live under `documentation/`: [6502.md](documentation/6502.md)
(CPU/addressing/cycles), [video.md](documentation/video.md) (display
modes, sprites, raster effects), [audio.md](documentation/audio.md)
(voices, waveforms), [assembler.md](documentation/assembler.md) (complete
syntax/diagnostics), [system-architecture.md](documentation/system-architecture.md)
(clock, memory, I/O, interrupts), [memory-map.md](documentation/memory-map.md)
(address tables), [cartridge-projects.md](documentation/cartridge-projects.md)
(manifests, banking, debugger workflow). `code-assets/demos/` has eight
complete example cartridges (raster, wave, tiles, sprites, graphics,
bitmap, audio, music) — read one before inventing a pattern from scratch.

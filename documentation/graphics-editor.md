# Fanticon Graphics Editor

Fanticon graphics are edited as `.GFX` documents in the normal IDE tab strip.
The global hardware palette is stored separately as a `.PAL` document so every
image and graphics set can reference the same colors. Both formats are ASCII,
valid assembler input, and editable visually or as source with `Ctrl/Cmd+G`.

Choose **File > New Graphics** or press `Ctrl/Cmd+Shift+N`. A new graphics file
references `GAME.PAL`; if it does not exist in the current folder, the editor
immediately creates a default DB16 `GAME.PAL`. Choose **File > New Palette** to
create and open `GAME.PAL` directly. If it already exists, the command opens the
existing resource instead of replacing it. Existing `.GFX` and `.PAL` files also
open visually from the project browser or Open dialog.

## The resident graphics set

A `.GFX` file describes graphics that can be resident together in the VM. In
tilemap mode it contains exactly:

| Block | Size | Hardware destination |
| --- | ---: | --- |
| Patterns | 8,192 bytes | VRAM `$0000-$1FFF` |
| Tile numbers | 2,048 bytes | VRAM `$2000-$27FF` |
| Tile attributes | 2,048 bytes | VRAM `$2800-$2FFF` |

That is 12,288 bytes and fits in one 16 KiB cartridge bank. The referenced
`.PAL` contains the separate 256-byte global palette. Pattern bytes use the VM's
native format: the high nibble is the left pixel and the low nibble is the right
pixel, so runtime conversion is unnecessary.

Games may store multiple `.GFX` sets in cartridge ROM. Loading another set
replaces the resident patterns and map. A game can also copy only the ranges it
needs—for example, retain common player patterns `$00-$3F` while replacing
level patterns `$80-$FF`.

### How Pattern, Map, and Sprite fit together

There is one shared library of 256 reusable 8×8 patterns:

1. **Pattern** edits one reusable 8×8 image.
2. **Map** places those patterns into the 64×32 circular background.
3. **16×16 Sprite** edits four consecutive patterns together as a sprite
   composite.

Changing a pattern changes every map cell and sprite that references it. An 8×8
hardware sprite uses one pattern. A 16×16 sprite uses `N,N+1` above `N+2,N+3`,
where `N` is divisible by four. Sprite view edits that artwork; runtime position,
size, palette bank, flips, priority, and enable state live in the 32 sprite
records and are controlled by game code.

Pattern pixels store color numbers 0-15. The map cell or sprite record chooses
one of the 16 palette banks. The same pattern can therefore appear with several
color schemes. Color 0 is transparent for sprites but opaque in the background.

The editor view and active background mode are separate. Opening Pattern or
Sprite view does not change the background. Selecting Map chooses tilemap
background mode; selecting Bitmap chooses bitmap background mode. Hardware
sprites and their patterns remain usable over either background.

The visual views are:

- `1` — one shared 8×8 pattern and the complete 16×16 pattern sheet.
- `2` — a pannable 40×25 view of the 64×32 background map. Arrow keys pan it.
- `3` — a 16×16 sprite composite made from four shared patterns.
- `4` — the referenced 256-color palette.
- `5` — a packed 320×200 bitmap background.

Drawing tools are `P` Pencil, `F` connected Fill, and `I` Eyedropper. Pattern
view supports `H`/`V` flips, `R` clockwise rotation, Delete, arrow-key pattern
selection, and Undo/Cut/Copy/Paste. Map view uses `H`, `V`, and `Q` for placement
flips and foreground priority.

In Map view, the arrow keys move the editor viewport one tile and wrap at all
four map edges. This exposes every cell and makes the circular seam directly
editable. The viewport origin shown in the status bar is the hardware cell at
its upper-left corner; it does not alter data when the asset is saved.

## Shared palettes

The VM has one global palette of 256 RGB332 entries, arranged as 16 banks of 16.
Every `.GFX` referring to the same `.PAL` sees the same colors. Open tabs update
together, and saving a graphics document also saves its palette resource.

New `GAME.PAL` resources use DawnBringer DB16 in every bank. DB16 color 0 maps to
RGB332 `$00` (black), avoiding the red cast caused by quantizing its original
very dark purple into a two-bit blue channel.

Palette view offers `DB16`, `PICO-8`, `C64`, and `EGA`. Click a preset or press
`N`/`Shift+N` to replace only the selected bank. Use `R`, `G`, or `B` to increase
an RGB332 component and Shift with the same key to decrease it. Preset and
component edits are undoable.

A typical project assignment is:

```text
bank 0  UI and text
bank 1  player
bank 2  enemies
bank 3  current level
bank 8  title bitmap
```

Bitmap pixels contain only color numbers 0-15 and use the bank selected by the
`BITMAP_PALETTE` register. Sprite records still select their own banks.

## ASCII formats

New graphics files use version 3 and reference their palette:

```asm
;@FANTICON-GFX 3
;@PALETTE-FILE GAME.PAL
;@MODE TILEMAP
```

The `;@TILES`, `;@MAP`, and `;@ATTRIBUTES` markers delimit hardware-native
blocks. Saving `WORLD.GFX` creates `WORLD_CHR`, `WORLD_MAP`, and `WORLD_ATR`.

A palette begins with:

```asm
;@FANTICON-PAL 1
;@PALETTE
GAME_PAL
```

Legacy version 1 and 2 files remain readable. Their 40×25 maps are placed at the
top-left of the new 64×32 map and the added cells are initialized to zero.

Both resources are ordinary assembler input. Include a shared palette once,
followed by the graphics sets the cartridge needs:

```asm
         FIXED
         ORG   $D000
         PUT   GAME.PAL
         PUT   WORLD.GFX
```

## Loading a tilemap set

Write `GAME_PAL` through the auto-incrementing palette port:

```asm
BANKKIND EQU   $C000
BANKNUM  EQU   $C001
PALINDEX EQU   $C01B
PALDATA  EQU   $C01C

         LDX   #0
         STX   PALINDEX
PALLOOP  LDA   GAME_PAL,X
         STA   PALDATA
         INX
         BNE   PALLOOP
```

Copy `WORLD_CHR` to VRAM `$0000`, `WORLD_MAP` to `$2000`, and `WORLD_ATR` to
`$2800`. Their exact sizes are `$2000`, `$0800`, and `$0800`. Then select video
mode 1 and enable the background and sprite layers as required.

## Loading a bitmap set

A bitmap set occupies three consecutive cartridge banks. For start bank 7:

| Cartridge bank | Label | Destination |
| ---: | --- | --- |
| 7 | `TITLE_BM0` | First bitmap portion in VRAM bank 1 |
| 8 | `TITLE_BM1` | Remaining bitmap portion in VRAM bank 2 |
| 9 | `TITLE_CHR` | Resident sprite patterns in VRAM bank 0 |

The editor limits the first bank to 0-253 so all three fit. Include a bitmap
`.GFX` before the program's `FIXED` section, then explicitly restore `FIXED` for
following code. Load its referenced `.PAL`, select the desired
`BITMAP_PALETTE`, choose video mode 2, and enable sprites as needed.

Banked ROM and VRAM share the CPU window, so loaders stage each page through
main RAM: map ROM, copy into RAM, map VRAM, copy out, and repeat.

## Version-control behavior

Visual saves produce canonical uppercase hexadecimal lines. Returning from
ASCII view validates markers and exact block sizes. Malformed data remains in
source view with a readable error rather than being silently truncated or
padded. Palette references are explicit text, so palette sharing and changes
remain visible in source control.

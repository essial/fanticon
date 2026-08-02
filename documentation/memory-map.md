# Fanticon Memory Map

This is the quick-reference memory map for game code running inside the Fanticon
VM. All addresses are hexadecimal. Multi-byte registers and values are
little-endian.

## CPU address space

```text
$FFFF  ┌──────────────────────────────┐
       │ CPU vectors                  │  NMI $FFFA, RESET $FFFC,
$FFFA  ├──────────────────────────────┤  IRQ/BRK $FFFE
       │                              │
       │ Fixed cartridge ROM          │  Always visible
       │                              │
$C100  ├──────────────────────────────┤
$C0FF  │ Memory-mapped I/O            │  System, video, audio,
$C000  ├──────────────────────────────┤  input, and timers
$BFFF  │                              │
       │ 16 KiB bank window           │  Cartridge ROM, work RAM,
       │                              │  video RAM, or save RAM
$8000  ├──────────────────────────────┤
$7FFF  │                              │
       │ 32 KiB main RAM              │  Always visible
       │                              │
$0200  ├──────────────────────────────┤
$01FF  │ 6502 hardware stack          │
$0100  ├──────────────────────────────┤
$00FF  │ Zero page                    │
$0000  └──────────────────────────────┘
```

| Range | Size | Read | Write |
| --- | ---: | --- | --- |
| `$0000-$7FFF` | 32 KiB | Main RAM | Main RAM |
| `$8000-$BFFF` | 16 KiB | Selected bank | Selected bank, unless ROM |
| `$C000-$C0FF` | 256 B | Device registers | Device registers |
| `$C100-$FFFF` | 16,128 B | Fixed cartridge ROM | Ignored |

Unmapped and reserved reads return `$FF`. Unmapped, reserved, and ROM writes are
ignored.

## Bank window: `$8000-$BFFF`

Write `BANK_KIND` and `BANK_NUMBER` to choose the visible 16 KiB bank.

| Register | Address | Purpose |
| --- | --- | --- |
| `BANK_KIND` | `$C000` | Select the backing memory type |
| `BANK_NUMBER` | `$C001` | Select one bank of that type |

| `BANK_KIND` | Valid `BANK_NUMBER` | Window contains |
| ---: | ---: | --- |
| `$00` | 0-255 | Cartridge ROM, up to 4 MiB |
| `$01` | 0-3 | Volatile work RAM, 64 KiB total |
| `$02` | 0-2 | Video RAM, 48 KiB total |
| `$03` | 0-3 | Battery-backed save RAM, up to 64 KiB total |
| Other | — | Unmapped memory |

Reset selects cartridge bank 0. Switch banks while executing from main RAM or
fixed ROM, never from the bank window itself.

The cartridge header declares how many save-RAM banks exist. Banks beyond that
count are unmapped. On native hosts the contents are stored in a sibling `.SAV`
file as specified by the [Fanticon Cartridge Format](cartridge-format.md).

To access an offset in a banked memory region:

```text
bank number = offset / $4000
CPU address = $8000 + (offset AND $3FFF)
```

## I/O page overview

```text
$C0FF  ┌──────────────────────────────┐
$C070  │ Reserved                     │
$C06F  ├──────────────────────────────┤
$C060  │ Two interval timers          │
$C05F  ├──────────────────────────────┤
$C050  │ Controller input             │
$C04F  ├──────────────────────────────┤
$C030  │ Audio processing unit        │
$C02F  ├──────────────────────────────┤
$C010  │ Video control and palette    │
$C00F  ├──────────────────────────────┤
$C000  │ Banking and interrupts       │
       └──────────────────────────────┘
```

Access abbreviations:

- **R**: read only
- **W**: write only
- **R/W**: readable and writable
- **W1C**: write a one to clear that bit

## System and interrupt registers

| Address | Name | Access | Description |
| --- | --- | --- | --- |
| `$C000` | `BANK_KIND` | R/W | `$00` cartridge, `$01` work RAM, `$02` VRAM, `$03` save RAM |
| `$C001` | `BANK_NUMBER` | R/W | Bank within the selected kind |
| `$C002` | `IRQ_PENDING` | R/W1C | Pending interrupt sources |
| `$C003` | `IRQ_ENABLE` | R/W | Sources allowed to assert IRQ |
| `$C004` | `FRAME_LOW` | R | Frame counter low byte; latches high byte |
| `$C005` | `FRAME_HIGH` | R | Latched frame counter high byte |
| `$C006` | `MACHINE_MAJOR` | R | Fanticon hardware major version, currently 1 |
| `$C007` | `MACHINE_MINOR` | R | Fanticon hardware minor version, currently 0 |
| `$C008-$C00F` | — | — | Reserved |

`IRQ_PENDING` and `IRQ_ENABLE` bits:

| Bit | Mask | Source |
| ---: | ---: | --- |
| 0 | `$01` | Vertical blank begins |
| 1 | `$02` | Raster X/Y comparison matches |
| 2 | `$04` | Timer 0 reaches zero |
| 3 | `$08` | Timer 1 reaches zero |
| 4-7 | — | Reserved |

Read `FRAME_LOW` before `FRAME_HIGH` to get one coherent frame-counter value.

## Video registers

| Address | Name | Access | Description |
| --- | --- | --- | --- |
| `$C010` | `VIDEO_MODE` | R/W | 0 blank, 1 tilemap, 2 packed bitmap |
| `$C011` | `VIDEO_CONTROL` | R/W | Bit 0 background enable, bit 1 sprites enable |
| `$C012` | `BACKDROP_COLOR` | R/W | 8-bit palette index behind a disabled background |
| `$C013` | `SCROLL_X_LOW` | R/W | Signed tilemap X scroll bits 0-7 |
| `$C014` | `SCROLL_X_HIGH` | R/W | Signed tilemap X scroll bits 8-15 |
| `$C015` | `SCROLL_Y_LOW` | R/W | Signed tilemap Y scroll bits 0-7 |
| `$C016` | `SCROLL_Y_HIGH` | R/W | Signed tilemap Y scroll bits 8-15 |
| `$C017` | `RASTER_X_LOW` | R/W | Raster compare dot bits 0-7 |
| `$C018` | `RASTER_X_HIGH` | R/W | Raster compare dot bit 8 |
| `$C019` | `RASTER_Y_LOW` | R/W | Raster compare line bits 0-7 |
| `$C01A` | `RASTER_Y_HIGH` | R/W | Raster compare line bit 8 |
| `$C01B` | `PALETTE_INDEX` | R/W | Select one of 256 palette entries |
| `$C01C` | `PALETTE_DATA` | R/W | RGB332 data; index increments after every read or write |
| `$C01D` | `BITMAP_PALETTE` | R/W | Bitmap palette bank in bits 0-3 |
| `$C01E` | `VIDEO_STATUS` | R | VBlank, HBlank, and sprite-overflow status |
| `$C01F-$C02F` | — | — | Reserved |

`SCROLL_X` and `SCROLL_Y` are signed 16-bit two's-complement offsets. Tilemap
fetches wrap them modulo 512 and 256 respectively, so decrementing zero to
`$FFFF` scrolls left or up by exactly one pixel.

`VIDEO_CONTROL` masks:

| Mask | Meaning |
| ---: | --- |
| `$01` | Enable tilemap or bitmap background |
| `$02` | Enable hardware sprites |

Scroll X and Y are independent and wrap modulo 512 and 256. Increasing a scroll
coordinate moves the viewport right or down; decreasing it moves left or up.
When the background is disabled, `BACKDROP_COLOR` fills it. Sprites can remain
visible over the backdrop.

`VIDEO_STATUS` uses bit 0 for live VBlank, bit 1 for live HBlank, and bit 2 for
sprite overflow latched during the current frame. Bits 3-7 read zero. Reading
does not clear anything. HBlank covers dots 320-399, VBlank covers lines 200-261,
and overflow clears at line 0, dot 0.

The raster comparator triggers once per visit to its exact X/Y target. Clearing
the IRQ while the beam remains on that coordinate does not retrigger it. A new
target ahead of the beam may trigger in the current frame; a passed target waits
for the next frame. Reset selects unreachable target `(511,511)`.

Palette entry `N` resets to RGB332 value `N`. Components expand with rounding:
`R=round(R3*255/7)`, `G=round(G3*255/7)`, and `B=round(B2*255/3)`. Palette,
backdrop, scroll, layer-enable, and mode writes affect the next pixel fetched.
Scroll registers are ignored in bitmap mode.

## Video RAM

Select `BANK_KIND=$02`. Banks 0-2 expose VRAM offsets `$0000-$BFFF`.

### Tile mode VRAM

| VRAM offset | Bank | Size | Description |
| --- | ---: | ---: | --- |
| `$0000-$1FFF` | 0 | 8 KiB | 256 packed 4-bpp, 8×8 tile patterns |
| `$2000-$27FF` | 0 | 2,048 B | 64×32 tile-number map |
| `$2800-$2FFF` | 0 | 2,048 B | 64×32 tile-attribute map |
| `$3000-$30FF` | 0 | 256 B | 32 eight-byte sprite records |
| `$3100-$3FFF` | 0 | — | Reserved/scratch VRAM |

Useful formulas:

```text
tile pattern offset = tile_number × 32
tilemap offset       = tile_y × 64 + tile_x
tile number byte     = $2000 + tilemap offset
tile attribute byte  = $2800 + tilemap offset
sprite record        = $3000 + sprite_number × 8
```

Each pattern byte contains two pixels. The high nibble is the left pixel; the
low nibble is the right pixel.

Tile attribute byte:

```text
7       6       5       4       3 2 1 0
RES   PRIORITY V-FLIP  H-FLIP   PALETTE
```

### Sprite record

| Offset | Name | Meaning |
| ---: | --- | --- |
| 0 | `X_LOW` | X position bits 0-7 |
| 1 | `X_FLAGS` | Bit 0 X bit 8; bit 1 behind background |
| 2 | `Y` | Y position |
| 3 | `TILE` | First pattern tile |
| 4 | `ATTR` | Palette, flips, size, enable |
| 5-7 | — | Reserved |

Sprite `ATTR`:

```text
7       6       5       4       3 2 1 0
ENABLE  16×16   V-FLIP  H-FLIP   PALETTE
```

Sprite color 0 is transparent. Lower record numbers appear above higher record
numbers. For nonzero background pixels, a tile's foreground bit covers every
sprite; otherwise a sprite's behind-background flag places it below the
background. Background color 0 never hides a sprite.

Coordinates `$1F0-$1FF` in X mean -16 through -1, and `$F0-$FF` in Y mean -16
through -1. Sprites clip at all four edges and never wrap. A 16×16 sprite uses
tiles `N,N+1` on top and `N+2,N+3` below; `N` must be divisible by four, and
flips apply to the complete composite.

The first eight enabled sprites whose vertical range intersects a scanline are
evaluated. Horizontally off-screen or fully transparent candidates still count.
Later candidates are skipped on that line and latch overflow until frame start.
Sprite records are captured at scanline start, so mid-line record writes affect
the following scanline.

### Bitmap mode VRAM

| VRAM offset | Bank | Size | Description |
| --- | ---: | ---: | --- |
| `$4000-$7FFF` | 1 | 16 KiB | First bitmap section |
| `$8000-$BCFF` | 2 | 15,616 B | Remaining bitmap pixels |
| `$BD00-$BFFF` | 2 | 768 B | Reserved |

```text
VRAM byte offset = $4000 + Y × 160 + (X / 2)

even X pixel = high nibble
odd X pixel  = low nibble
```

The 4-bit pixel combines with `BITMAP_PALETTE` to select one of the 256 palette
entries. Sprite patterns and records remain in bank 0 and therefore work in
bitmap mode without aliasing bitmap pixels.

## Audio registers

The APU has two pulse channels, one triangle channel, and one noise channel.
For waveform sequences, exact divider behavior, the noise table, and mixing, see
the [Fanticon Audio Programmer's Reference](audio.md).

### Pulse 1 and Pulse 2

| Channel | Control | Timer low | Timer high | Phase reset |
| --- | --- | --- | --- | --- |
| Pulse 1 | `$C030` | `$C031` | `$C032` | `$C033` |
| Pulse 2 | `$C034` | `$C035` | `$C036` | `$C037` |

Pulse control:

```text
7       6 5       4       3 2 1 0
ENABLE  DUTY      RES     VOLUME
```

Duty values select 12.5%, 25%, 50%, or 75%. Timer high uses bits 0-2. Writing
the phase-reset register resets that channel's waveform phase.

### Triangle

| Address | Name | Description |
| --- | --- | --- |
| `$C038` | `TRI_CONTROL` | Bit 7 enables the triangle |
| `$C039` | `TRI_TIMER_LOW` | Timer bits 0-7 |
| `$C03A` | `TRI_TIMER_HIGH` | Timer bits 8-10 |
| `$C03B` | `TRI_PHASE_RESET` | Write any value to reset phase |

### Noise

| Address | Name | Description |
| --- | --- | --- |
| `$C03C` | `NOISE_CONTROL` | Enable, short mode, and 4-bit volume |
| `$C03D` | `NOISE_PERIOD` | Period-table index in bits 0-3 |
| `$C03E` | `NOISE_RESET` | Write any value to reseed the LFSR |
| `$C03F` | — | Reserved |

Noise control uses bit 7 for enable, bit 6 for short mode, and bits 0-3 for
volume.

| Address | Name | Description |
| --- | --- | --- |
| `$C040` | `AUDIO_MASTER` | Master enable and 4-bit master volume |
| `$C041-$C04F` | — | Reserved |

`AUDIO_MASTER` uses bit 7 for master enable and bits 0-3 for master volume.
Pulse and triangle phase-reset registers and `NOISE_RESET` always read `$00`.

## Controller registers

| Address | Name | Description |
| --- | --- | --- |
| `$C050` | `PAD0_STATE` | Currently held controller-1 buttons |
| `$C051` | `PAD0_PRESSED` | Newly pressed buttons; read clears returned bits |
| `$C052` | `PAD1_STATE` | Currently held controller-2 buttons |
| `$C053` | `PAD1_PRESSED` | Newly pressed buttons; read clears returned bits |
| `$C054-$C05F` | — | Reserved |

Controller bits:

```text
7       6       5       4       3       2       1       0
START   SELECT  B       A       RIGHT   LEFT    DOWN    UP
```

A set bit means pressed.

Inputs are sampled at line 0, dot 0. `PAD_PRESSED` accumulates newly pressed
edges until read, then clears all returned bits. A same-cycle edge is latched
after the read and cannot be lost. Opposite directions may coexist. A disconnected
controller reads all released.

## Timer registers

| Timer | Reload low | Reload high | Control | Count low | Count high |
| --- | --- | --- | --- | --- | --- |
| Timer 0 | `$C060` | `$C061` | `$C062` | `$C063` | `$C064` |
| Timer 1 | `$C068` | `$C069` | `$C06A` | `$C06B` | `$C06C` |

Timer control:

| Mask | Meaning |
| ---: | --- |
| `$01` | Enable counter |
| `$02` | Automatically reload after reaching zero |

`$C065-$C067`, `$C06D-$C06F`, and the complete `$C070-$C0FF` range are reserved.
Both timers decrement once per CPU cycle. Reading count low latches count high
until high is read. Reload writes do not alter a running count. An enable 0-to-1
transition loads reload and counting begins on the following CPU cycle. A reload
value of zero means 65,536 cycles. Automatic mode fires every exact reload
interval and reloads immediately; one-shot mode stops at zero. Disabling
preserves current count, while re-enabling restarts from reload.

## CPU vectors

| Address | Vector |
| --- | --- |
| `$FFFA-$FFFB` | NMI, reserved by v0.1 hardware |
| `$FFFC-$FFFD` | RESET entry point |
| `$FFFE-$FFFF` | IRQ and BRK entry point |

The cartridge's fixed ROM must provide all three vectors even when it does not
use NMI or IRQ.

For timing rules and device behavior, see the complete
[Fanticon System Architecture](system-architecture.md).

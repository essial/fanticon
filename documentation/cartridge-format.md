# Fanticon Cartridge and Save Format

This document defines Fanticon cartridge (`.FCN`) and battery-backed save
(`.SAV`) files. Both formats are little-endian and independently versioned.

## Capacity

A cartridge contains:

- one fixed 16 KiB ROM image;
- 0-256 switchable 16 KiB ROM banks; and
- a declaration for 0-4 battery-backed 16 KiB RAM banks.

The maximum banked ROM is therefore 4 MiB. Including the fixed image and 64-byte
header, the maximum `.FCN` file is `$404040` bytes (4,210,752 bytes). This is the
largest cartridge addressable by the existing 8-bit `BANK_NUMBER` register and
does not require a second mapper register.

Battery-backed RAM may be 0, 16, 32, 48, or 64 KiB. It is selected with
`BANK_KIND=$03` and `BANK_NUMBER=0-3`. A cartridge can expose fewer than four
banks; selecting an absent bank returns `$FF` and ignores writes.

## Cartridge file layout

```text
offset $000000  64-byte FCN header
offset $000040  16 KiB fixed ROM image
offset $004040  banked ROM bank 0
offset $008040  banked ROM bank 1
                ...
```

Banks are stored uncompressed in ascending order. This permits validation,
memory mapping, and direct seeking without a decompressor or section directory.
The file must have exactly the size implied by its header; trailing bytes are an
error.

### FCN header

| Offset | Size | Type | Field | Meaning |
| ---: | ---: | --- | --- | --- |
| `$00` | 8 | bytes | `MAGIC` | ASCII `FANTICON` |
| `$08` | 1 | `u8` | `FORMAT_MAJOR` | `$01` |
| `$09` | 1 | `u8` | `FORMAT_MINOR` | `$00` |
| `$0A` | 2 | `u16` | `HEADER_SIZE` | `$0040` |
| `$0C` | 4 | `u32` | `FLAGS` | Feature flags |
| `$10` | 2 | `u16` | `ROM_BANKS` | Switchable banks, 0-256 |
| `$12` | 1 | `u8` | `SAVE_BANKS` | Battery RAM banks, 0-4 |
| `$13` | 1 | `u8` | `MAPPER` | `$00` for the v0.1 mapper |
| `$14` | 4 | `u32` | `FIXED_CRC32` | CRC-32 of the fixed ROM image |
| `$18` | 4 | `u32` | `BANKED_CRC32` | CRC-32 of all banked ROM bytes |
| `$1C` | 8 | `u64` | `CARTRIDGE_ID` | Stable, nonzero game identifier |
| `$24` | 22 | bytes | `TITLE` | Printable ASCII, NUL padded |
| `$3A` | 1 | `u8` | `MACHINE_MAJOR` | Required hardware major version |
| `$3B` | 1 | `u8` | `MACHINE_MINOR` | Minimum required hardware minor version |
| `$3C` | 4 | `u32` | `HEADER_CRC32` | CRC-32 of header bytes `$00-$3B` |

All CRC fields use CRC-32/ISO-HDLC: polynomial `$04C11DB7`, initial value
`$FFFFFFFF`, reflected input and output, and final XOR `$FFFFFFFF`.
`BANKED_CRC32` is zero when `ROM_BANKS` is zero.

`CARTRIDGE_ID` is generated once when a project is created and retained across
builds and releases. It associates a save with the game without invalidating
that save whenever ROM code changes. Zero is not a valid ID.

Titles may contain spaces but are limited to 22 printable ASCII characters.
Hardware major versions must match exactly. A loader accepts a cartridge only
when its own minor version is at least `MACHINE_MINOR`. v0.1 cartridges require
machine version 1.0.

`FLAGS` currently defines only bit 0:

| Mask | Name | Meaning |
| ---: | --- | --- |
| `$00000001` | `BATTERY_RAM` | Cartridge uses its declared save RAM |

Bit 0 must be set exactly when `SAVE_BANKS` is nonzero. All other flag bits must
be zero in format 1.0. Unknown flags, mapper values, or major versions are
rejected. A loader also rejects a minor version newer than it explicitly
supports.

### Fixed ROM image

The fixed image is a complete 16 KiB logical image for `$C000-$FFFF`. Its first
256 bytes correspond to `$C000-$C0FF`, which the I/O page hides; packagers fill
those bytes with `$FF`. Storing a full power-of-two image keeps vector offsets and
linker layout simple:

| CPU address | Fixed-image offset |
| --- | ---: |
| `$C100` | `$0100` |
| `$FFFA` NMI vector | `$3FFA` |
| `$FFFC` RESET vector | `$3FFC` |
| `$FFFE` IRQ/BRK vector | `$3FFE` |

The fixed-image vector bytes must be present. RESET must point into fixed ROM or
main RAM; a cartridge cannot depend on a switchable bank before reset code has
selected it.

## Save file naming

On native platforms, save RAM is stored beside the cartridge with the same base
name and a `.SAV` extension:

```text
ASTRO.FCN  ->  ASTRO.SAV
```

The name is based on the loaded cartridge path, not its internal title. The save
file is host-managed and is never directly visible in the VM filesystem. Browser
builds store equivalent data in persistent browser storage keyed by
`CARTRIDGE_ID`; an export operation produces the same `.SAV` bytes.

## Save file layout

```text
offset $0000  32-byte SAV header
offset $0020  battery-backed RAM bytes
```

### SAV header

| Offset | Size | Type | Field | Meaning |
| ---: | ---: | --- | --- | --- |
| `$00` | 8 | bytes | `MAGIC` | Bytes `FCNSAVE` followed by `$1A` |
| `$08` | 1 | `u8` | `FORMAT_MAJOR` | `$01` |
| `$09` | 1 | `u8` | `FORMAT_MINOR` | `$00` |
| `$0A` | 2 | `u16` | `HEADER_SIZE` | `$0020` |
| `$0C` | 8 | `u64` | `CARTRIDGE_ID` | Must match the loaded cartridge |
| `$14` | 4 | `u32` | `RAM_SIZE` | Exact payload length in bytes |
| `$18` | 4 | `u32` | `DATA_CRC32` | CRC-32 of RAM payload |
| `$1C` | 4 | `u32` | `HEADER_CRC32` | CRC-32 of header bytes `$00-$1B` |

`RAM_SIZE` must be 16, 32, 48, or 64 KiB. The file must have exactly
`HEADER_SIZE + RAM_SIZE` bytes. A new save is initialized to `$00`.

Fanticon first validates magic, version, cartridge ID, declared file length, and
both CRCs. Invalid or mismatched files are reported and left untouched. After a
valid save with the correct cartridge ID is loaded, its `RAM_SIZE` is compared
with `SAVE_BANKS × $4000`. If they differ, Fanticon deletes the old save and
creates a new zero-filled save of the required size; save data is not migrated.

## Persistence behavior

Writes through the save-RAM bank window update emulated RAM immediately and mark
the save dirty. The host flushes dirty data:

- after one second with no new save-RAM writes;
- when a cartridge is cleanly ejected or Game mode exits; and
- during orderly application shutdown.

Native writes use a temporary file in the same directory, flush it, and atomically
replace the `.SAV` file. This prevents a crash during saving from partially
overwriting the last valid save. A host crash can lose the newest unflushed
writes but must not corrupt the previously committed file.

Only one running Fanticon instance may own a particular save for writing. If a
save lock cannot be acquired, the cartridge runs with save RAM read-only and the
host displays a warning.

## Mapper 0

Format 1.0 defines only mapper 0, which is the standard memory map:

| `BANK_KIND` | Meaning | Bank range |
| ---: | --- | ---: |
| `$00` | Cartridge ROM | 0 to `ROM_BANKS-1` |
| `$01` | Work RAM | 0-3 |
| `$02` | Video RAM | 0-2 |
| `$03` | Battery-backed save RAM | 0 to `SAVE_BANKS-1` |

Mapper numbers permit a future architecture version to define genuinely
different hardware without overloading flags or guessing from file size. Format
1.0 cartridges must use mapper 0.

## Validation order

A loader validates a cartridge before exposing any ROM byte to the CPU:

1. minimum length and magic;
2. supported major/minor version and header size;
3. known flags and mapper;
4. ROM and save bank-count limits;
5. nonzero cartridge ID, valid title bytes, and compatible machine version;
6. exact total file size;
7. header, fixed-ROM, and banked-ROM CRC values;
8. fixed-ROM vector presence.

This format stores no host paths, executable host code, compression stream, or
embedded save data.

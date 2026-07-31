# Fanticon System Details Checklist

The v0.1 programmer-visible hardware contract is now frozen. Implementation may
be incomplete, but cartridges must not observe behavior outside these documents.

## Locked hardware contracts

- 3.144 MHz NMOS 6502 and 60 Hz, 400×262 raster clock tree
- Fixed same-cycle ordering for CPU transfers, devices, raster, and IRQ events
- CPU memory map and 16 KiB bank window
- 4 MiB maximum banked cartridge ROM
- 64 KiB work RAM, 48 KiB VRAM, and up to 64 KiB save RAM
- Deterministic cold boot, CPU RESET timing, register defaults, and CPU startup state
- No reserved zero-page bytes or hidden firmware ABI
- Shared, software-prioritized IRQ controller and unused standard NMI
- Machine major/minor identification and cartridge compatibility rules
- Tile and bitmap layouts, immediate raster writes, and bitmap-mode scroll behavior
- Exact RGB332 expansion, identity reset palette, and palette read/write increment
- Raster comparator re-arming and exact HBlank/VBlank status intervals
- Sprite priority, clipping, negative-edge coordinates, composite flips, and overflow
- Two pulse, one triangle, and one noise APU channel
- Exact APU waveforms, periods, divider/write timing, nonlinear mix, and reset state
- Deterministic mono VM audio with light host-side stereo width and reverb
- Controller sampling, pressed-edge latching, hot-unplug, and default bindings
- Timer start, reload, interval, count-latch, disable, and IRQ behavior
- Versioned `.FCN` cartridge and `.SAV` persistence formats

## Locked development contracts

- `FANTICON.CFG` project manifest and stable 64-bit cartridge identity
- Explicit `BANK` and `FIXED` assembly sections
- Bank-aware symbols and `BANKOF(label)`
- Bank overflow, overlap, vector, relocation, and cross-bank diagnostics
- Contiguous cartridge packing through the highest referenced bank
- `BUILD`, `RUN`, Build & Run, and direct command-line launch paths
- Editor-origin Game mode returns with Escape; direct cartridge launch does not
- Required v0.1 debugger scope
- Deterministic recording/replay deferred beyond v0.1

## Still implementation-defined

These details do not alter VM-visible behavior and can be tuned during host work:

- Exact stereo-width, reverb, reconstruction-filter, and resampler coefficients
- Host audio buffer size and underrun recovery
- Gamepad database and user-remapping interface
- Recent-cartridge UI and save-lock warning presentation
- Debugger window layout and keyboard shortcuts

## Required implementation tests

Each mapped device still needs end-to-end tests for exact-cycle register access,
reset, simultaneous events, invalid values, and long-run determinism. Cartridge
and save parsers need malformed-input and crash-safe persistence tests. The
packager needs golden `.FCN` files and diagnostics for every invalid layout.

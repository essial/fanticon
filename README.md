# Fanticon

Fanticon is a high-performance fantasy console with a cycle-accurate NMOS 6502,
mapped late-1980s-style video/audio/input hardware, native development tools, and
versioned bank-switched cartridges.

## Why Rust

The CPU implementation is allocation-free and has no runtime dependency on the
app host. Build the library with `--no-default-features` to omit all windowing and
GPU dependencies. Rust compiles to native Windows, Linux, and macOS targets and
to WebAssembly from the same source. Every `Bus::read` or `Bus::write` is one
physical CPU cycle, keeping video, audio, timers, and input hardware synchronized.

The core implements all 256 NMOS opcodes, including undocumented instructions and
their dummy reads/writes. `Cpu::step` executes one instruction and returns its
cycle count.

## Documentation

Developers writing 6502 programs for the Fanticon VM should start with the
[6502 Programmer's Reference](documentation/6502.md). It covers registers,
addressing, every official instruction, opcode and cycle tables, interrupts,
decimal arithmetic, undocumented opcodes, and optimization patterns.

The [video architecture](documentation/video.md) describes the 320×200 indexed
display, dot-timestamped raster events, GPU scaling, and CRT presentation pass.
The [audio programmer's reference](documentation/audio.md) defines the two pulse,
triangle, and noise voices, including exact waveforms, timing, and mixing.
The [editor and command console](documentation/editor.md) describes application
modes, the embedded character ROM, native commands, and future tool dispatch.
The [macro assembler](documentation/assembler.md) documents building raw `.BIN`
files, supported syntax and directives, Merlin-style macros, and diagnostics.
The [system architecture](documentation/system-architecture.md) defines the VM's
clock tree, memory and I/O maps, tile/bitmap video, sprites, NES-like audio,
controllers, interrupts, timers, and cartridge-visible reset model.
The [memory-map quick reference](documentation/memory-map.md) provides visual CPU
and I/O maps, VRAM layouts, register addresses, bit fields, and address formulas.
The [cartridge format](documentation/cartridge-format.md) specifies `.FCN`
cartridges, 4 MiB ROM banking, battery-backed RAM, and `.SAV` persistence. The
[cartridge-project guide](documentation/cartridge-projects.md) specifies
manifests, bank-aware assembly, packaging, launching, and debugger requirements.
The [system-details checklist](documentation/system-details-checklist.md)
separates frozen v0.1 contracts from remaining implementation work.

## Run the Fanticon host

The current app host runs a paced 60 Hz emulation loop and opens a resizable
8:5 virtual display. Game mode remains hardware-accurate at 320×200; native
Editor mode uses 640×400 with an 80×50 character grid. Both use aspect-correct
letterboxing, scanlines, phosphor beam shaping, and composite color bleed. On
launch, a centered Fanticon logo
appears for five seconds and can be dismissed by keyboard or mouse after a 500 ms
guard. The native Editor mode command console then appears by default.

```sh
cargo run --release
```

Native app builds mirror each folder inside the repository's `code-assets`
directory directly into `Documents/Fanticon`. This makes the checked-in examples
available at `/demos` inside Fanticon while keeping the repository copy
authoritative and preserving unrelated user files. Set
`FANTICON_SKIP_CODE_ASSET_SYNC=1` to skip this developer convenience when needed.

Inside Fanticon, `NEW PROJECT`, `BUILD`, and `RUN` create, package, and launch a
cartridge project. An existing image can be launched with `RUN GAME.FCN`, or
directly from the host:

```sh
cargo run --release -- /path/to/GAME.FCN
```

Directly launched games ignore Escape. Games launched by Editor `RUN` return to
the Editor with Escape after flushing battery-backed RAM.

Start explicitly in Game mode instead:

```sh
cargo run --release -- --game
```

In VS Code, install the recommended CodeLLDB and rust-analyzer extensions, then
press F5 and select **Fanticon App (Debug)**. VS Code builds the correct binary
automatically before opening the app under the debugger.

## Clock and hardware pins

`Cpu::clock` advances exactly one physical bus cycle and reports the 6502 `SYNC`
state, instruction completion, and persistent JAM state. `Cpu::step` is the
convenience API that loops over the same clock engine, so both APIs share identical
timing.

The bus supplies logical pin levels through `Bus::pins`:

- RESET performs its seven read-cycle sequence and can recover a jammed CPU.
- IRQ is level-sensitive and uses the interrupt-disable value sampled before the
  instruction's final cycle, including the NMOS CLI/SEI polling quirks.
- NMI is falling-edge-sensitive and remains latched until serviced.
- RDY repeats read cycles without advancing internal state; writes are not stalled.
- SO is falling-edge-sensitive and sets the overflow flag.

KIL/JAM opcodes enter a persistent state that repeatedly reads `$FFFF` until
RESET. The optional `step_fast` profiling path is instruction-atomic and bypasses
external pin handling; console emulation should use `clock` or `step`.

## Build and test

Clone Fanticon with its test fixtures:

```sh
git clone --recurse-submodules <fanticon-repository-url>
```

For an existing checkout, initialize the submodule once:

```sh
git submodule update --init --depth 1
```

Ordinary tests automatically use `tests/SingleStepTests/6502/v1`. The 256 opcode
files are exposed as 256 independently reported tests and run in parallel by
default. Release mode is recommended for the complete 2.56-million-case suite:

```sh
cargo test --release -- --nocapture
cargo build --release
```

Run the headless whole-machine performance workloads with:

```sh
cargo bench --no-default-features --bench vm
```

The report covers the audio, bitmap, raster, sprite, tile, and two-axis wave
demo cartridges and prints frames per second, emulated cycles per second, the
multiple of Fanticon's required 60 Hz real-time rate, and steady-state allocation
calls per frame.

Browser runtime tests use `wasm-pack` and headless Chrome:

```sh
wasm-pack test --headless --chrome --test web_runtime
```

They execute a cartridge inside WebAssembly and verify controller sampling,
indexed video output, generated audio, and persistent `.SAV` serialization
through browser storage.

Run one opcode by filtering on its test name:

```sh
cargo test --release --test singlestep opcode_a9 -- --nocapture
```

Limit every selected opcode to its first 100 cases while developing:

```sh
SINGLESTEP_LIMIT=100 cargo test --test singlestep opcode_a9 -- --nocapture
```

Rust chooses the parallelism from the machine's available CPU count. Override it
when needed, for example with `--test-threads=4` or `--test-threads=1` after the
second `--`. `SINGLESTEP_6502_DIR` remains available as an optional fixture-path
override. The fixtures are pinned beneath `tests/` as a Git submodule instead of
being copied into Fanticon's Git history.

## Cross-platform targets

Use ordinary Rust target triples, for example:

```sh
cargo build --release --target x86_64-pc-windows-msvc
cargo build --release --target x86_64-unknown-linux-gnu
cargo build --release --target aarch64-apple-darwin
rustup target add wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown
```

The `cdylib` output is the WebAssembly-facing library while `rlib` is the
zero-overhead native integration path.

CI builds and tests the library on Windows, Linux, and macOS, and separately
compiles the same core for `wasm32-unknown-unknown`.

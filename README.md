# Fanticon

Fanticon is a high-performance fantasy-console project. This repository currently
contains its cycle-accurate NMOS MOS 6502 CPU foundation.

## Why Rust

The CPU implementation is allocation-free and has no runtime dependency on the
app host. Build the library with `--no-default-features` to omit all windowing and
GPU dependencies. Rust compiles to native Windows, Linux, and macOS targets and
to WebAssembly from the same source. Every `Bus::read` or `Bus::write` is one
physical CPU cycle, allowing future video, audio, timers, and input hardware to
remain synchronized.

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
The [editor and command console](documentation/editor.md) describes application
modes, the embedded character ROM, native commands, and future tool dispatch.

## Run the Fanticon host

The current app host runs a paced 60 Hz emulation loop and opens a resizable
320×200 virtual display with aspect-correct letterboxing, scanlines, phosphor
beam shaping, and composite color bleed. On launch, a centered Fanticon logo
appears for five seconds and can be dismissed by keyboard or mouse after a 500 ms
guard. The native Editor mode command console then appears by default.

```sh
cargo run --release
```

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

The `cdylib` output supports a future WebAssembly-facing API while `rlib` is the
zero-overhead native integration path.

CI builds and tests the library on Windows, Linux, and macOS, and separately
compiles the same core for `wasm32-unknown-unknown`.

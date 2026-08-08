# Fanticon

Fanticon is a late-1980s-style fantasy console: a cycle-accurate NMOS 6502 CPU,
tile/sprite/bitmap video, four-voice audio, native development tools, and a
versioned, bank-switched cartridge format you can build real games on.

## Download

Prebuilt binaries for every tagged release are published on the
[Releases page](https://github.com/essial/fanticon/releases):

| Platform | Architecture   | Installer               | Portable                       |
| -------- | -------------- | ------------------------ | ------------------------------- |
| Windows  | x86_64 / arm64 | `Fanticon-Setup-*.exe`   | `fanticon-*-windows-*.zip`      |
| Linux    | x86_64 / arm64 | `fanticon_*.deb`         | `fanticon-*-linux-*.tar.gz`     |
| macOS    | universal      | `fanticon-*-macos.dmg`   | `fanticon-*-macos-universal.zip`|

Portable archives need no installation: unzip or untar the archive and run
`fanticon-app` (or `fanticon-app.exe` on Windows) directly. See
[Building from source](#building-from-source) to build it yourself instead.

## Get started

Launching Fanticon drops you into the native editor's command console.

1. `NEW PROJECT` creates a cartridge project with a manifest and starter source.
2. Write 6502 assembly in the built-in editor — syntax highlighting, symbol
   navigation, and inline diagnostics included.
3. `BUILD` assembles it; `RUN` launches it straight into Game mode.
4. Press Escape to return to the editor. Battery-backed save RAM is flushed
   first, so `RUN` is safe to use as a normal play-test loop.

When the game is ready to share, the bundled `fanticon-export` tool produces a
browser player or a standalone Windows, Linux, or macOS binary from the official
prebuilt runtime kit. Exporting is toolchain-free and cross-platform: creators
do not need Rust, WebAssembly tools, platform SDKs, or the target operating
system. Every official installer and portable archive includes all target
runtimes. See the [export guide](documentation/exporting.md).

An existing cartridge can be opened with `RUN GAME.FCN` from the console, or
launched directly:

```sh
fanticon-app /path/to/GAME.FCN
```

Cartridges launched this way ignore Escape, since there's no editor session to
return to. To land straight in Game mode instead of the editor on startup, add
`--game`.

Game mode is hardware-accurate at 320×200; the native editor runs at 640×400
with an 80×50 character grid. Both are presented with aspect-correct
letterboxing, scanlines, phosphor beam shaping, and composite color bleed.

## What you get

- **Cycle-accurate NMOS 6502** — all 256 opcodes, including undocumented
  instructions, dummy reads/writes, and hardware quirks like RESET's
  seven-cycle sequence, IRQ/NMI polling behavior, RDY stalls, and SO edges.
  If it runs on real hardware, it should run here.
- **320×200 indexed video** with tile, sprite, and bitmap modes, dot-timestamped
  raster events for mid-frame effects, and a CRT presentation pass.
- **Four-voice audio** — two pulse channels, triangle, and noise, NES-shaped
  and exactly timed against the CPU clock.
- **Native development tools** — a full-screen code editor, a macro assembler
  with named/defaulted parameters, private labels, compile-time conditionals and
  repetition, project manifests, and an integrated debugger with breakpoints and
  raster triggers.
- **Versioned `.FCN` cartridges** — 4 MiB ROM banking, battery-backed save
  RAM, and CRC-checked headers.
- **Cross-platform** — native Windows, Linux, and macOS builds (x86_64 and
  arm64), plus WebAssembly from the same source.

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
The [graphics editor](documentation/graphics-editor.md) documents `.GFX` resident
sets, shared `.PAL` resources, tile/map/sprite/bitmap tools, and direct VM loading.
The [macro assembler](documentation/assembler.md) documents building raw `.BIN`
files, supported syntax and directives, modern macros, and diagnostics.
The [system architecture](documentation/system-architecture.md) defines the VM's
clock tree, memory and I/O maps, tile/bitmap video, sprites, NES-like audio,
controllers, interrupts, timers, and cartridge-visible reset model.
The [memory-map quick reference](documentation/memory-map.md) provides visual CPU
and I/O maps, VRAM layouts, register addresses, bit fields, and address formulas.
The [cartridge format](documentation/cartridge-format.md) specifies `.FCN`
cartridges, 4 MiB ROM banking, battery-backed RAM, and `.SAV` persistence. The
[export guide](documentation/exporting.md) covers toolchain-free HTML and native
standalone builds for every supported platform. The
[cartridge-project guide](documentation/cartridge-projects.md) specifies
manifests, bank-aware assembly, packaging, launching, and debugger requirements.
The [system-details checklist](documentation/system-details-checklist.md)
separates frozen v0.1 contracts from remaining implementation work.

## Building from source

Clone Fanticon with its test fixtures:

```sh
git clone --recurse-submodules https://github.com/essial/fanticon.git
```

For an existing checkout, initialize the submodule once:

```sh
git submodule update --init --depth 1
```

```sh
cargo run --release
```

Native app builds mirror each folder inside the repository's `code-assets`
directory directly into `Documents/Fanticon`. This makes the checked-in examples
available at `/demos` and the standard hardware definitions available at
`/FANTICON.INC`, while keeping the repository copy authoritative and preserving
unrelated user files. The assembler also embeds that include, so every project
can use `INCLUDE FANTICON.INC` on native and web builds. The editor opens the
root system include from that embedded source as a read-only document; the
managed disk copy is only there to make it browsable. Set
`FANTICON_SKIP_CODE_ASSET_SYNC=1` to skip this developer convenience when needed.

```sh
cargo run --release -- /path/to/GAME.FCN   # launch a cartridge directly
cargo run --release -- --game              # start in Game mode
```

In VS Code, install the recommended CodeLLDB and rust-analyzer extensions, then
press F5 and select **Fanticon App (Debug)**. VS Code builds the correct binary
automatically before opening the app under the debugger.

### Cross-compiling

Use ordinary Rust target triples, for example:

```sh
cargo build --release --target x86_64-pc-windows-msvc
cargo build --release --target aarch64-pc-windows-msvc
cargo build --release --target x86_64-unknown-linux-gnu
cargo build --release --target aarch64-unknown-linux-gnu
cargo build --release --target aarch64-apple-darwin
rustup target add wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown
```

The `cdylib` output is the WebAssembly-facing library while `rlib` is the
zero-overhead native integration path — build with `--no-default-features` to
omit all windowing and GPU dependencies if you only need the CPU/VM core.

CI builds and tests the library on x86_64 Windows, Linux, and macOS, and on
native arm64 Linux; it cross-compiles and build-checks arm64 Windows, and
separately compiles the same core for `wasm32-unknown-unknown`. macOS builds
run on Apple Silicon runners and cover both `aarch64-apple-darwin` and
`x86_64-apple-darwin`.

### Testing

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

## License

Fanticon is dual-licensed under the [MIT License](LICENSE-MIT) or the
[Apache License, Version 2.0](LICENSE-APACHE), at your option.

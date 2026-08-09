# Exporting Fanticon Games

Fanticon exports web players and standalone desktop binaries without invoking a
compiler. Creators do not need Rust, `wasm-bindgen`, an SDK, or a computer that
runs the target operating system. The official runtime kit contains prebuilt
players for every supported target.

Official installers and portable archives include the complete `runtimes`
directory. A separately downloaded or locally built kit can instead be selected
with `--runtime-kit`, or through the `FANTICON_RUNTIME_KIT` environment variable.
The kit has a version manifest and Fanticon refuses an incomplete or mismatched
kit instead of silently generating a broken player.

Exports are available from the editor's **Build** menu, from the Fanticon prompt,
or through `fanticon-export`. Prompt examples are:

```text
EXPORT WEB
EXPORT WIN64
EXPORT WINARM
EXPORT LINUX64
EXPORT LINUXARM
EXPORT MACOS
EXPORT ALL
```

Each command builds the current project first. An optional second argument
overrides the default 8.3 output name.

## Web export

```text
fanticon-export html GAME.FCN GAME-WEB
```

The output directory contains `index.html`, the Fanticon JavaScript/WebAssembly
runtime, `game.fcn`, a web app manifest, and an offline service worker. It is an
installable Progressive Web App and caches the game after its first successful
load. Serve the directory from an HTTPS host (or localhost while testing); browsers
do not allow WebAssembly games to work reliably when `index.html` is opened as a
raw `file:` URL.

The generated player waits for a click before initializing, satisfying browser
audio autoplay policies and focusing game input. It includes fullscreen and PNG
screenshot buttons. Battery
RAM is stored in browser local storage under the cartridge's stable 64-bit ID,
so rebuilding the same project retains its save while unrelated games remain
isolated.

## Binary export

```text
fanticon-export binary windows-x86_64 GAME.FCN GAME.EXE
fanticon-export binary windows-arm64  GAME.FCN GAME-ARM.EXE
fanticon-export binary linux-x86_64   GAME.FCN GAME-X64
fanticon-export binary linux-arm64    GAME.FCN GAME-ARM64
fanticon-export binary macos-universal GAME.FCN GAME-MAC
```

The cartridge is appended to a prebuilt runtime template. PE, ELF, and Mach-O
loaders ignore the versioned Fanticon footer after the executable image, while
the player validates and launches that embedded cartridge at startup. No source
files or separate editor resources are packaged. Battery RAM is written beside
the cartridge for direct `.FCN` launches. Packaged games store it in the host
platform's per-user Fanticon application-data directory, keyed by the stable
cartridge ID, so installed or read-only app bundles can still save normally.

Every exported web directory and binary destination receives the Fanticon MIT
and Apache 2.0 license texts. Optional `AUTHOR`, `DESCRIPTION`, `ICON`,
`WEB_BACKGROUND`, and `WEB_FOREGROUND` values come from `FANTICON.CFG`. Native
exports write the descriptive metadata to a same-stem `.TXT` sidecar and copy a
configured icon to a same-stem `.PNG` sidecar.

Raw binaries remain available for scripts that need them. For normal game
distribution, use the packaged exports instead:

```text
fanticon-export package windows-x86_64 GAME.FCN
fanticon-export package windows-arm64  GAME.FCN
fanticon-export package linux-x86_64   GAME.FCN
fanticon-export package linux-arm64    GAME.FCN
fanticon-export package macos-universal GAME.FCN
```

Windows packages are ZIP archives containing a customized `.EXE`; on real PE
runtime templates Fanticon embeds the game title, author, description, version,
and optional custom icon in the executable. Linux packages are `.tar.gz` archives with
a relocatable AppDir layout, desktop entry, executable `AppRun`, and a project
or Fanticon icon. The macOS ZIP contains a complete `.app` bundle with `Info.plist`, a
universal executable, and an `.icns` generated from the project PNG. Archives
also contain the applicable readme and license files and preserve executable
permissions.

## Export all platforms

```text
fanticon-export all GAME.FCN GAME-RELEASE
```

`EXPORT ALL` at the Fanticon prompt and selecting every target in the editor do
the same job. Fanticon builds the cartridge once, then writes a publishable PWA,
two Windows ZIPs, two Linux `.tar.gz` archives, one universal macOS app ZIP, and
`RELEASE.txt` into one directory. It also writes `release.json`, a deterministic
inventory containing the byte size and SHA-256 digest of every release file.
Verify a copied or uploaded release without a runtime kit using:

```text
fanticon-export verify-release GAME-RELEASE
```

Verification rejects missing, additional, truncated, or modified files. Every
output is self-contained; players need neither Fanticon nor a Rust, WebAssembly,
SDK, or target-platform toolchain.

macOS public distribution may additionally require signing and notarization for
the publisher's identity. These are Gatekeeper trust operations, not compilation
requirements; the generated app itself is complete.

## Runtime kit layout

```text
runtimes/
  web/fanticon.js
  web/fanticon_bg.wasm
  windows-x86_64/fanticon-player.exe
  windows-arm64/fanticon-player.exe
  linux-x86_64/fanticon-player
  linux-arm64/fanticon-player
  macos-universal/fanticon-player
```

Runtime templates are ordinary release builds with no game attached. Release CI
builds them on their native platforms and assembles the kit, which prevents the
exporter's host platform from affecting generated games.

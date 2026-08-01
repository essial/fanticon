# Fanticon Video Architecture

Fanticon cartridges present a fixed 320×200 virtual display. Native Editor mode
uses a separate 640×400 surface so its 8×8 character ROM provides an 80×50 text
grid without changing VM video hardware. The host window may be any size: the GPU
scales either 8:5 surface to the largest centered rectangle and draws exact
solid-black letterbox or pillarbox bars around it.

## Running the host

```sh
cargo run --release
```

After boot, the host displays its native 80×50 Editor command console. `RUN`
builds and launches the current cartridge project, while `RUN GAME.FCN` launches
an existing image. The console uses the same indexed-color and CRT presentation
path as a game, but not the VM's 320×200 framebuffer or raster hardware.

Before the diagnostic screen, the app presents the centered Fanticon logo for
five seconds. Keyboard and mouse presses are ignored for the first 500 ms to
prevent the launch input from immediately dismissing it; after that guard period,
either input advances to the normal display. The boot timer begins when the GPU
renderer is ready, ensuring initialization time does not consume the logo's
visible duration.

## Emulation rate

The host runs one emulation tick at an average of exactly 60 Hz. Its frame pacer
accumulates the fractional nanosecond that cannot be represented by a single
`Duration`, preventing long-term clock drift from repeatedly rounding 1/60
second.

The OS event loop sleeps until the next deadline, so the app does not busy-wait.
If a frame misses its deadline, obsolete deadlines are skipped instead of
running multiple VM frames in a burst. After a long debugger stop, suspended
browser tab, or system sleep, the schedule rebases to current time. This prevents
a catch-up spiral while keeping ordinary frames phase-stable.

The current tick advances either the native editor or exactly 52,400 mapped
machine cycles in Game mode. Those cycles drive the CPU bus, raster, timers,
controllers, APU, and cartridge together before presenting the completed frame.

## Rendering path

The display pipeline has three intentionally separate layers:

1. `Video` owns the active indexed framebuffer—320×200 for games or 640×400 for
   native tools—plus its 256-entry RGBA palette and
   ordered raster-event log.
2. At the end of a VM frame, `Video::resolve_rgba` makes one linear pass over the
   active display and applies palette or pixel writes when the simulated beam
   reaches their timestamp.
3. The host uploads the resulting RGBA image—256 KiB for games or 1,000 KiB for
   Editor mode—to one persistent GPU texture and draws one fullscreen triangle.
   A WGSL shader performs scaling, letterboxing, scanline beam shaping, composite
   color filtering, bloom, and a mild vignette.

There are no per-pixel draw calls, transient GPU textures, or allocations in the
steady-state presentation path. The CPU-side resolve is bounded by 64,000 pixels
and the raster events generated in that frame.

## Raster timestamps

`RasterTick` identifies a dot within a frame:

```text
tick = scanline × DOTS_PER_SCANLINE + dot
```

The initial timing envelope is 400 dots per scanline and 262 scanlines per frame.
The visible region is dots 0–319 on lines 0–199. Representing blanking explicitly
leaves room for future horizontal-blank and vertical-blank hardware work without
changing the timestamp type.

Events must be recorded in beam order. This makes appending them constant-time
and lets resolution merge events with pixels in one pass, without sorting. Two
events may share a timestamp; their insertion order is preserved.

```rust
use fanticon::video::{RasterTick, Video};

let mut video = Video::new();
video.begin_frame();

// Change palette entry 3 immediately before visible pixel (120, 75) is fetched.
video.write_palette_at(
    RasterTick::new(75, 120).unwrap(),
    3,
    [255, 80, 40, 255],
)?;
```

An event takes effect at its timestamp, before that dot is fetched. A write to a
pixel the beam already passed remains in persistent video memory for the next
frame but does not retroactively alter the current image. This supports raster
palette splits, mid-scanline color changes, and timed framebuffer writes.

The v0.1 machine clock is now defined as two video dots per 6502 cycle: 200 CPU
cycles per scanline, 52,400 cycles per frame, and a 3.144 MHz CPU at 60 Hz. A bus
transfer becomes visible on the second dot of its CPU cycle. The complete clock
tree and mapped video-device contract are specified in
[System Architecture](system-architecture.md).

## Untimed and timed writes

Use `pixels_mut` and `set_palette` for initialization or writes known to occur
during vertical blank. They directly update persistent video state.

For active-display changes, call `begin_frame`, then use `write_pixel_at` and
`write_palette_at`. At presentation time, call `resolve_rgba` with a buffer of
`RGBA_FRAME_LEN` bytes.

This separation keeps ordinary bulk framebuffer updates cheap while retaining an
exact path for effects that depend on the beam position.

## CRT presentation

CRT effects are presentation-only. They never modify emulated framebuffer or
palette state. The single GPU pass currently provides:

- aspect-correct letterbox/pillarbox scaling;
- anisotropic beam reconstruction with continuous horizontal signal spread,
  weaker vertical blending, and a vertical scanline beam profile without a
  source-pixel column grid;
- scanline luminance modulation tied to source scanlines;
- composite-inspired YIQ reconstruction that preserves luma detail while
  reducing chroma bandwidth and adding restrained phase-dependent crosstalk;
- edge darkening;
- very subtle monochrome CRT snow refreshed by the 60 Hz presentation clock; and
- a thresholded phosphor/glass bloom that lets bright pixels softly illuminate
  their horizontal, vertical, and diagonal neighbors.

The command console and text editor use a dedicated text presentation variant.
It keeps a strong texel-centered character core, then adds restrained bilinear
softness, horizontal phosphor spread, and bloom. Composite chroma filtering
remains disabled so text stays readable, while scanline brightness, vignette,
and subtle CRT snow preserve the monitor character. The startup logo and future
game display continue using the fuller composite-style treatment.

The snow changes luminance by at most 0.15% and never moves, scales, or distorts
the image. It is presentation-only, so emulated pixels and raster timestamps
remain deterministic.

Bloom uses a compact bright-pass kernel in the existing presentation shader.
Ordinary midtones contribute little or nothing, preventing the whole image from
looking blurred. It requires no intermediate texture or additional draw pass.

Keeping these effects in the final shader makes their cost proportional to host
window pixels, leaves the VM's pixel timing deterministic, and allows the visual
style to become configurable later without touching emulation logic.

The signal filtering approach is inspired by Shay Green's (blargg's) NTSC video
filters, particularly their separate luma/chroma kernels and independently
controlled artifacting, fringing, and color bleed. Fanticon uses a compact GPU
approximation rather than incorporating those CPU filter sources directly.

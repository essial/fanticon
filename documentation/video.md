# Fanticon Video Architecture

Fanticon presents a fixed 320×200 virtual display. The host window may be any
size: the GPU scales the display to the largest centered 8:5 rectangle and draws
black letterbox or pillarbox bars around it.

## Running the host

```sh
cargo run --release
```

The initial host displays a diagnostic pattern. It does not run a cartridge yet.
The pattern deliberately changes palette color partway through scanline 100 to
exercise the same raster-event path future VM hardware will use. The split moves
at the host's 60 Hz emulation rate.

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

The current tick only prepares video. CPU cycles, timers, audio, and cartridge
execution will be added to `FanticonApp::emulate_frame` as those systems are
connected.

## Rendering path

The display pipeline has three intentionally separate layers:

1. `Video` owns the 320×200 indexed framebuffer, 256-entry RGBA palette, and
   ordered raster-event log.
2. At the end of a VM frame, `Video::resolve_rgba` makes one linear pass over the
   active display and applies palette or pixel writes when the simulated beam
   reaches their timestamp.
3. The host uploads the resulting 256 KiB RGBA image to one persistent GPU
   texture and draws one fullscreen triangle. A WGSL shader performs scaling,
   letterboxing, scanlines, RGB phosphor masking, horizontal color bleed, and a
   mild vignette.

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

The final CPU-cycle-to-video-dot ratio is not defined yet. When the fantasy
console's master clock and video registers are specified, the bus/video device
can convert master-clock time to `RasterTick` without changing the host renderer.

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
- nearest-neighbor sampling of the 320×200 source;
- scanline luminance modulation tied to source scanlines;
- an RGB phosphor triad mask tied to physical output pixels;
- inexpensive channel-dependent horizontal bleed for color artifacting;
- edge darkening;
- very subtle monochrome CRT snow refreshed by the 60 Hz presentation clock; and
- a thresholded phosphor/glass bloom that lets bright pixels softly illuminate
  their horizontal, vertical, and diagonal neighbors.

The snow changes luminance by at most 0.6% and never moves, scales, or distorts
the image. It is presentation-only, so emulated pixels and raster timestamps
remain deterministic.

Bloom uses a compact bright-pass kernel in the existing presentation shader.
Ordinary midtones contribute little or nothing, preventing the whole image from
looking blurred. It requires no intermediate texture or additional draw pass.

Keeping these effects in the final shader makes their cost proportional to host
window pixels, leaves the VM's pixel timing deterministic, and allows the visual
style to become configurable later without touching emulation logic.

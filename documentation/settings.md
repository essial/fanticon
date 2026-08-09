# Graphics and audio settings

Fanticon uses one versioned set of presentation preferences for the editor,
running cartridges, and games exported with the Fanticon runtime. Open it from
**Help > Settings** in the editor. While a game is running, press **F10** or the
gamepad **Guide/Mode** button to pause and open the system menu. Escape, F10,
the gamepad B button, or **Resume game** closes it. Escape keeps its existing
meaning when the menu is closed: it returns an editor-launched game to the
editor and does not quit a standalone cartridge.

The menu is keyboard and controller friendly. Use the arrow keys or D-pad to
select and change a value, and Enter, A, or Start to activate a command. Changes
are applied and saved immediately. The mouse can hover and click any row; a
saved or save-failed message confirms persistence without closing the menu.

## Graphics

The renderer provides six styles:

| Style | Presentation |
| --- | --- |
| Clean Pixel | Sharp nearest-texel output with no display simulation |
| VGA | A restrained scanline and bloom treatment |
| Arcade CRT | A brighter, sharper RGB arcade-monitor treatment |
| Consumer CRT | Composite chroma softness, beam shaping, bloom, and light noise |
| LCD | Hard pixels with a subtle panel-cell grid |
| Amber Mono | Luminance-preserving amber phosphor output |

**Effect strength** controls the intensity of the selected display treatment.
**Brightness** applies a final display gain. **Integer scaling** uses the largest
whole-number scale that fits whenever the window is at least as large as the
source image; smaller windows still scale down to remain visible. Letterboxing
and aspect ratio are preserved in every style.

Changing a rendering setting updates the actual display immediately. The six
clickable monitor swatches beneath the settings panel provide a compact style
picker, while the full menu remains the live preview.

## Audio

**Master volume**, **stereo width**, and **reverb** update the active audio
stream immediately. Reverb uses independent parallel room filters, stereo
diffusion, predelay, and treble-damped feedback to model a large concert hall;
0% is fully dry, while the high end retains a smooth hall decay at a restrained
quarter-strength return level so the full-volume direct sound stays forward. Filtering has five profiles:
Crisp, Balanced, Warm,
Vintage, and Minimal. They select different reconstruction cutoffs; Vintage
also applies gentle saturation.

The separate high-pass filter can be disabled or set to 20, 60, or 120 Hz.
The 60 Hz default removes DC, subsonic energy, and low-frequency rumble; 20 Hz
preserves more bass, while 120 Hz produces a leaner sound.

The factory presentation uses 50% master volume, 50% reverb, and 50% stereo
width.

Changing an audio-processing value plays a short decaying chord through the
active filter, stereo, and hall chain. This makes comparisons possible while a
game or tracker is paused behind the settings screen.

Native builds can request Auto, 128, 256, 512, 1024, or 2048 audio frames.
Smaller buffers reduce latency but are more sensitive to scheduling stalls;
larger buffers are more resilient. Fanticon clamps the request to the audio
device's supported range and retries with the device default if the fixed-size
stream cannot be created. Browsers control the Web Audio callback quantum, so
web exports retain the preference but use browser-managed buffering.
The Web settings screen therefore labels the value **Browser managed** and does
not present the native frame choices as if they could override Web Audio.

When **Mute when unfocused** is enabled, Fanticon clears queued sound as soon as
its window loses focus.

## Persistence

Native installations store `settings.json` in the operating system's Fanticon
configuration directory. Web builds store the same versioned data in local
storage for the page's origin. Invalid, unsupported-version, or out-of-range
values safely fall back to normalized defaults. Settings belong to the host,
not to a cartridge, so a game cannot silently replace the player's preferences.

## Diagnostics overlay

Enable **Diagnostics overlay** in settings to place a compact translucent panel
over the editor or running game. It reports measured presented FPS, most recent
frame lateness, skipped frame deadlines, device sample rate and channel count,
queued audio frames, queue capacity, buffer mode, underruns, and the active
rendering style. The renderer composites it after resolving either VM video or
host UI pixels, so diagnostics never alter cartridge memory or screenshots
produced by the emulated video hardware. The overlay is hidden while settings
itself is open and its toggle is persisted globally.

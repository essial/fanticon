# Fanticon Music Editor

Fanticon's music editor is a four-channel tracker for the console's two pulse,
triangle, and noise voices. Choose **File > New Music** or open a `.MUS` file.
Music resources use normal IDE tabs and save as assembler source.

The tracker has three views. Press `V` to rotate through them:

- **Pattern** edits notes, instruments, and volume. During playback the complete
  active row is highlighted and kept at the vertical center of the tracker.
- **Frames** is the song order. Every frame selects an independent pattern for
  pulse 1, pulse 2, triangle, and noise, like FamiTracker's frame editor.
- **Instrument** edits volume, arpeggio, pitch, and tone sequences.

All three names are also clickable tabs. The top-right **PLAY/STOP** button,
pattern piano, instrument/volume steppers, frame/order buttons, instrument
selectors, envelope step buttons, and envelope graphs allow a complete
mouse-driven workflow. Click an envelope label to select it, click a bar to set
its value, or drag across bars to draw a sequence. The mouse wheel moves the
current row, frame, or envelope step. Keyboard controls remain available for
fast tracker note entry.

During playback, each channel heading has a centered horizontal level graph
behind its text. The graph expands equally to the left and right using the live
pulse, triangle, or noise level produced by the shared tracker player.

## Pattern controls

| Input | Action |
| --- | --- |
| Arrow keys / Tab | Move through rows, channels, note, instrument, and volume |
| `A W S E D F T G Y H U J` | Enter a chromatic octave like piano keys |
| `Z` / `X` | Lower or raise the entry octave |
| `0`-`F` | Set the selected instrument or volume field |
| Delete or Backspace | Turn a note off, or clear an instrument/volume value |
| `-` or `.` | Hold the previous value |
| `[` / `]` | Move to the previous or next frame |
| `+` / `_` | Increase or decrease video frames per tracker row |

`OFF` disables a channel. `---`, `--`, and `-` hold the previous note,
instrument, or volume. A new note resets oscillator phase and all instrument
sequences. Noise notes `N00` through `N0F` select its sixteen periods.

In the Instrument view, the same `A W S E D F T G Y H U J` piano layout
auditions the selected instrument on the current channel and octave. The note
sounds while its key is held and does not alter the pattern. Key repeat does not
restart the envelope; release the key to stop the audition.

## Frame controls

Each order frame contains four pattern numbers. This lets a channel reuse one
pattern while another moves to a new pattern, and avoids copying repeated song
sections.

| Input | Action |
| --- | --- |
| Up / Down | Select an order frame |
| Left / Right | Select a channel |
| `0`-`F`, `+`, `-` | Select/change that channel's pattern |
| Insert | Duplicate the selected frame |
| Delete | Remove the selected frame |
| `L` or **Set Loop** | Make this frame the infinite-loop entry point |

Selecting a pattern number that does not exist creates an empty pattern.

## Instruments and envelopes

An instrument supplies four sequences. A sequence advances once per video
frame, including the frames between tracker rows:

- **Volume** contains `0`-`15`; it scales the row volume.
- **Arpeggio** contains signed semitone offsets from the note.
- **Pitch** contains signed timer offsets for fine slides and vibrato.
- **Tone/Duty** contains pulse duty `0`-`3`. On noise, `0` selects the long
  sequence and a nonzero value selects short/metallic noise. Triangle ignores
  tone/duty.

Use Up/Down to select a sequence, Left/Right to select a step, `+`/`-` to alter
signed values, Insert/Delete to add/remove steps, and `L` to set or clear the
loop point. `[`/`]` selects or creates instruments. Volume and tone can also be
entered directly with hexadecimal keys.

## Playback

| Input | Action |
| --- | --- |
| Space | Start or stop the tracker preview |
| F7 | Start or pause preview |
| Shift+F7 | Stop preview |
| F8 | Restart from row zero |
| Ctrl/Cmd+M | Toggle visual tracker / ASCII source |
| Ctrl/Cmd+Z | Undo |

Editor preview consumes the same compiled audio-register stream as the supplied
6502 cartridge player. Both therefore use identical note timers, volume,
arpeggio, pitch, duty/noise mode, retrigger timing, oscillator sequences,
nonlinear mixing, stereo treatment, and reverb.

## Editor background playlist

The Music menu can play `.MUS` and `.NSF` files while any editor tab is open.
Choose **Music > Choose Folder** to browse directories under Fanticon's managed
filesystem root, then choose **Music > Playlist** to select individual entries
or all of them. Scanning is intentionally non-recursive; select the exact folder
whose music you want to hear.

Multi-song NSF files appear as one selectable entry per internal track. Invalid
or unsupported files are skipped without stopping the rest of the playlist.
NSF tracks with a complete detected loop shorter than ten seconds are filtered
as likely sound effects. Tracks whose duration cannot be proven within that
window remain available, and explicit `.MUS` resources are never duration-filtered.
NSF duration checks run incrementally in the background, so opening the editor or
playlist remains responsive while the playlist reports its scan progress. Closing
and reopening the playlist preserves that progress, and playback can begin with
the entries already available while more NSF tracks continue to be checked. Scan
work uses a small per-frame time budget and prioritizes the current and included
tracks. Results are cached by file content and track, making later editor sessions
instant unless an NSF changes.

Tracker songs advance at their end, and NSF entries advance after one detected
loop. Sequential and shuffled ordering are available, along with whole-playlist
repeat. The selected folder, exclusions, shuffle/repeat options, and current
entry are retained in Fanticon's host settings.

`F7`, `Shift+F7`, `F8`, `Shift+F8`, and `Ctrl/Cmd+F8` control the active
playlist. System media Play, Pause, Play/Pause, Previous, Next, and Stop keys
work throughout the editor, including while a dialog is open. On macOS, Fanticon
publishes the active song and playback state to Control Center/Now Playing, so
these controls remain routed to Fanticon when another application is focused.
Opening a tracker preview or holding an instrument-audition key temporarily
suspends background music; it resumes the same playlist entry when the preview
ends. Running a game likewise leaves the playlist paused in place and resumes it
when control returns to the editor.

## File and cartridge format

A current resource begins with version 2 metadata. `PATTERN-ROWS` is the number
of rows in each reusable channel pattern.

```asm
;@FANTICON-MUSIC 2
;@TEMPO 6
;@PATTERN-ROWS 16
;@LOOP-ROW 0
SONG_MUSIC
         DFB   $F2
         DA    384
         DA    0
         DA    SONG_STREAM
         DA    SONG_LOOP
SONG_STREAM
SONG_LOOP
         DFB   $0F,$CF,$64,$01,$01,$AF,$25,$01
         DFB   $01,$01,$B1,$03,$01,$8F,$04,$00
         DFB   $01
```

The public `_MUSIC` label contains the v2 marker `$F2`, 16-bit total and loop
frame indexes, and pointers to the stream start and loop packet. Each frame
begins with a four-bit channel-change mask, followed by four bytes for every changed channel.
Pulse and triangle store control, timer low, timer high, and phase reset. Noise
stores control, period, reserved, and reset. An unchanged frame is one byte, so
held notes and long envelope tails take very little cartridge space.

Comment metadata after the stream stores the frame order, patterns, and
instruments. The editor can reconstruct the complete editable song, while the
assembler naturally omits those comment-only records from the cartridge. This
provides rich editing without asking the 6502 to interpret envelopes at runtime.

Version 1 flat-row songs remain readable and previewable. Saving one upgrades
it to version 2 frames, patterns, instruments, and compiled playback data.

## Cartridge playback

The `demos/music` project includes `PLAYER.INC`, a dual-format reference player.
Pass the song header in X/Y and call `MUSIC_TICK` once per VBlank:

```asm
         LDX   #<SONG_MUSIC
         LDY   #>SONG_MUSIC
         JSR   MUSIC_START
```

The v2 path decodes one delta frame into `$C030-$C03E`. It is intentionally
small and predictable; expensive pattern and envelope work occurs during the
build. At the end it jumps to the stored loop packet, allowing a one-time intro
followed by an infinite loop. `MUSIC_STOP` disables all voices. The driver
reserves zero-page `$30-$41`; relocate its `EQU` definitions if a game uses that range.

## Included classical demo

`demos/music/SONG.MUS` is a complete chiptune arrangement of Beethoven's “Ode
to Joy,” including the two opening phrases, contrasting middle section, and
final return. Its reference is the public-domain score and MIDI published as
[Mutopia Music ID 528](https://www.mutopiaproject.org/cgibin/piece-info.cgi?id=528).
Opening the legacy song demonstrates automatic v1-to-pattern migration; saving
it upgrades the resource to v2.

## Importing NSF files

Editor mode can execute an NSF driver and translate its output into an editable
MUS resource:

```text
NSF2MUS INPUT.NSF OUTPUT.MUS [TRACK]
```

`TRACK` defaults to the NSF's declared starting track. The importer automatically
stops when the NSF loop detector recognizes a complete intro plus one loop. The
resulting MUS frame order repeats indefinitely during normal playback; no fixed
song duration is stored. A ten-minute internal guard reports an error only when
a driver has no detectable repeating state. The importer samples all four supported channels at 60 Hz,
converts NES timers to the nearest Fanticon note, preserves effective volume,
pulse duty, noise mode, retriggers, oscillator gating, and legato pitch changes,
then deduplicates the result into per-channel 16-row patterns. A zero-volume
frame keeps the oscillator running, and a timer-only pitch update does not
restart its phase or instrument envelope.

An NSF is executable 6502 code rather than a score, so conversion is necessarily
a transcription. Fine pitch is quantized to Fanticon notes, PAL timing is
retuned to the Fanticon clock, and DPCM has no matching Fanticon channel. If the
source uses DPCM, the command completes but prints a warning that it was omitted.

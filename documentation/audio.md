# Fanticon Audio Programmer's Reference

Fanticon has four deterministic, monophonic voices: two pulse channels, one
triangle channel, and one pseudo-random noise channel. The sound character
follows the Nintendo Entertainment System, while Fanticon deliberately leaves
envelopes, sweeps, note lengths, and music sequencing to 6502 software.

The APU advances only from the 3.144 MHz emulated CPU clock. Host sample rate,
frame pacing, pausing, and audio-buffer size never change oscillator state.

## Register map

| Range | Voice |
| --- | --- |
| `$C030-$C033` | Pulse 1 |
| `$C034-$C037` | Pulse 2 |
| `$C038-$C03B` | Triangle |
| `$C03C-$C03F` | Noise |
| `$C040` | Master control |

Audio registers retain the last value written and are readable. Reserved bits
read as zero. A mapped write takes effect during the second video dot of its CPU
bus cycle, before that cycle's APU divider clock.

## Pulse channels

Pulse 1 and Pulse 2 are identical.

| Offset | Name | Meaning |
| ---: | --- | --- |
| 0 | `PULSE_CONTROL` | Bit 7 enable, bits 6-5 duty, bits 3-0 volume |
| 1 | `PULSE_TIMER_LOW` | Timer bits 0-7 |
| 2 | `PULSE_TIMER_HIGH` | Timer bits 8-10; other bits read zero |
| 3 | `PULSE_PHASE_RESET` | Any write resets phase and divider |

The duty sequences are eight steps long. A phase reset selects the first entry:

| Duty | Nominal width | Sequence |
| ---: | ---: | --- |
| 0 | 12.5% | `0 1 0 0 0 0 0 0` |
| 1 | 25% | `0 1 1 0 0 0 0 0` |
| 2 | 50% | `0 1 1 1 1 0 0 0` |
| 3 | 75% | `1 0 0 1 1 1 1 1` |

A high step outputs the 4-bit volume; a low step outputs zero. The 11-bit timer
range is `$000-$7FF`, and frequency is:

```text
pulse Hz = 3,144,000 / (16 × (timer + 1))
```

Timers `$000-$007` are allowed rather than forcibly silenced. They produce
ultrasonic fundamentals and normally should not be used for musical notes.

## Triangle channel

| Address | Name | Meaning |
| --- | --- | --- |
| `$C038` | `TRI_CONTROL` | Bit 7 enables output |
| `$C039` | `TRI_TIMER_LOW` | Timer bits 0-7 |
| `$C03A` | `TRI_TIMER_HIGH` | Timer bits 8-10; other bits read zero |
| `$C03B` | `TRI_PHASE_RESET` | Any write resets phase and divider |

The triangle has no per-channel volume, matching its main historical
inspiration. Its 32-step, 4-bit sequence is:

```text
15 14 13 12 11 10 9 8 7 6 5 4 3 2 1 0
 0  1  2  3  4  5 6 7 8 9 10 11 12 13 14 15
```

The first value is selected by phase reset. Frequency is:

```text
triangle Hz = 3,144,000 / (32 × (timer + 1))
```

When enabled, the current sequence value enters the mixer. When disabled it
contributes zero.

## Noise channel

| Address | Name | Meaning |
| --- | --- | --- |
| `$C03C` | `NOISE_CONTROL` | Bit 7 enable, bit 6 short mode, bits 3-0 volume |
| `$C03D` | `NOISE_PERIOD` | Period-table index in bits 3-0 |
| `$C03E` | `NOISE_RESET` | Any write loads seed 1 and resets the divider |
| `$C03F` | — | Reserved; reads `$00`, ignores writes |

The noise generator is a 15-bit right-shifting LFSR. On each shift, long mode
places `bit 0 XOR bit 1` into bit 14; short mode instead places
`bit 0 XOR bit 6` into bit 14. A set bit 0 outputs zero, and a clear bit 0
outputs the channel volume. The all-zero state is illegal and is repaired to the
reset seed of 1.

Long mode repeats after 32,767 shifts. Starting from reset, short mode repeats
after 93 shifts. Long mode supplies ordinary percussion noise; short mode has a
recognizably metallic, pitched quality.

The period table preserves the approximate pitches of the NTSC NES table after
scaling it to Fanticon's CPU clock:

| Index | CPU cycles per LFSR shift | Approx. shift rate |
| ---: | ---: | ---: |
| 0 | 7 | 449,143 Hz |
| 1 | 14 | 224,571 Hz |
| 2 | 28 | 112,286 Hz |
| 3 | 56 | 56,143 Hz |
| 4 | 112 | 28,071 Hz |
| 5 | 169 | 18,604 Hz |
| 6 | 225 | 13,973 Hz |
| 7 | 281 | 11,189 Hz |
| 8 | 355 | 8,856 Hz |
| 9 | 446 | 7,049 Hz |
| 10 | 668 | 4,707 Hz |
| 11 | 892 | 3,525 Hz |
| 12 | 1,339 | 2,348 Hz |
| 13 | 1,785 | 1,761 Hz |
| 14 | 3,573 | 880 Hz |
| 15 | 7,146 | 440 Hz |

The shift rate is not the perceived repetition frequency of the complete LFSR
sequence.

## Enable, phase, and divider behavior

Channel enable bits gate only that channel's contribution to the mixer. The
oscillator and divider continue running while muted, so enabling a voice does
not implicitly retrigger it. Use its phase-reset register when a synchronized
attack is required.

Writing a timer half changes the 11-bit reload value but does not reload the
current divider. A phase-reset write selects waveform step zero and loads the
divider from the current timer. Pulse dividers clock every second CPU cycle;
triangle dividers clock every CPU cycle. When a divider is zero, its next clock
advances the sequence and reloads the divider; otherwise that clock decrements
it. A noise reset similarly loads seed 1 and its selected period.

`AUDIO_MASTER` bit 7 gates final output and bits 3-0 set master volume. Master
disable and volume zero are silent but do not stop any channel. Re-enabling
therefore resumes at the phase the hardware reached while muted.

## Mixing

The two pulse DACs use one nonlinear path. Triangle and noise use a second
nonlinear path. For current 4-bit levels `p1`, `p2`, `t`, and `n`:

```text
pulse = 95.88 / (8128 / (p1 + p2) + 100)
tnd   = 159.79 / (1 / (t / 8227 + n / 12241) + 100)
mix   = (pulse + tnd) × master_volume / 15
```

A path with all-zero inputs contributes zero. Fanticon evaluates the equivalent
integer rational equations and produces unsigned Q0.16 output, so results are
bit-identical on native and WebAssembly hosts. This preserves NES-like nonlinear
balance without requiring floating-point VM state.

The presentation layer applies a source-rate-aware 20 Hz DC blocker followed by
a two-pole 14 kHz reconstruction filter before it downsamples the cycle-timestamped
signal to the host rate. This suppresses aliases and softens instantaneous digital
level edges without changing emulated register timing. It then presents the mono
hardware mix with stereo width and a short, subdued reverb. Differently delayed
taps feed the left and right sides, with a small cross-subtraction that decorrelates
sustained chip tones. The centered dry signal remains dominant; the effect must
not turn pulse or noise attacks into obviously displaced echoes.

Stereo and reverb are host presentation, not additional VM channels. They cannot
feed back into APU state, consume CPU cycles, change VM timing, or appear in a
save state or deterministic recording. Muting the presentation effect therefore
changes only what the listener hears, not the emulated audio stream.

## Reset behavior

Power-on, CPU RESET, and cartridge removal all perform the same APU reset:

- every register becomes zero;
- all channel and master enables clear;
- pulse and triangle phases become step zero;
- all dividers become zero; and
- the noise LFSR becomes 1.

RESET therefore silences audio immediately. Ordinary pause freezes emulated CPU
time and all APU state rather than resetting it.

## Deliberately software-controlled

Fanticon v0.1 has no hardware envelopes, pulse sweeps, length counters, APU frame
sequencer, sample channel, audio IRQ, or game-controlled stereo panning. A game
normally updates volume and timer registers from VBlank or an interval-timer IRQ.
This keeps the familiar four-voice sound while avoiding the NES APU's more
incidental control complexity.

## Editor NSF compatibility

The editor music radio runs NSF code in an isolated NMOS 6502 environment at the
declared NTSC or PAL rate. It supports NSF1 load/init/play addresses, the default
start track, playback speed fields, 4 KiB program banking, internal RAM mirrors,
and the two pulse, triangle, and noise register groups.

The player feeds those four voices through the same waveform tables, LFSR,
integer nonlinear mixer, host resampler, stereo width, and reverb used by a
Fanticon cartridge. Its source clock remains NES-rate so NSF timer values retain
their intended pitch.

Fanticon's radio does not add a fifth DMC/sample voice or cartridge expansion
audio. NSF files declaring FDS, MMC5, VRC6, VRC7, Namco 163, Sunsoft 5B, or other
expansion audio are rejected. Writes to the base NES DMC registers are ignored;
the four ordinary voices continue playing. The isolated NSF frontend implements
the NES pulse/noise envelopes, pulse/noise/triangle length counters, triangle
linear counter, and NTSC/PAL frame clocks, then feeds their resulting four voice
levels into Fanticon's shared waveform and mixer logic. NSF channel gates use a
1 ms post-mix step correction when a driver starts or expires a nonzero-volume
note. The correction begins at the preceding high-resolution mixed sample and
decays to the new level, so it does not quantize or repeatedly round an active
voice's four-bit volume. Oscillator waveform edges pass unchanged. Pulse sweep
units and frame IRQs remain unsupported. These NSF-only controls do not add the
NES control units to Fanticon's simpler in-game sound register model.

## Reference basis

The voice layout and register grouping follow the Ricoh 2A03 tradition. Exact
waveform, divider, LFSR, and mixer choices are frozen here as Fanticon behavior;
they are not claims that Fanticon is electrically identical to a particular NES
revision.

- [NESdev APU basics](https://www.nesdev.org/wiki/APU_basics)
- [Visual 2A03 chip-image project](https://www.qmtpro.com/~nes/chipimages/)

ODE TO JOY CHIPTUNE

This demo is the complete commonly performed Ode to
Joy hymn: two opening phrases, the contrasting middle
section, and the final return. It follows the public-
domain Mutopia soprano, alto, and bass parts. Noise
adds a Fanticon-specific chiptune percussion part.

Score reference: Mutopia Project, Music ID 528.
The score and MIDI are marked public domain.

Open SONG.MUS to use the visual music tracker.
Space starts or stops playback. The tracker centers
and highlights the complete currently playing row.

The four columns match Fanticon's sound hardware:
two pulse voices, triangle, and noise. Press V to
switch among pattern, frame order, and instrument
views. Each frame selects a pattern independently for
each channel. Instruments provide volume, arpeggio,
pitch, and pulse-duty/noise-tone envelopes.

MAIN.ASM shows cartridge playback. PUT includes the
song and PLAYER.INC. Call MUSIC_START with the address
of SONG_MUSIC in X/Y, then MUSIC_TICK once per VBlank.

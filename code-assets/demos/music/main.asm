; ---------------------------------------------------
; TRACKER MUSIC DEMO
; ---------------------------------------------------
;
; SONG.MUS is created and edited as a visual tracker.
; It is still assembler source, so PUT embeds it in the
; cartridge. PLAYER.INC writes the real APU registers.

         INCLUDE FANTICON.INC

         FIXED
         ORG   $C100
RESET    SEI
         LDX   #<SONG_MUSIC
         LDY   #>SONG_MUSIC
         JSR   MUSIC_START
         LDA   #$25
         STA   BACKDROP_COLOR
         PMC   SET_IRQS;IRQ_VBLANK
         CLI
FOREVER  JMP   FOREVER

; The player advances exactly once per VBlank.
IRQ
         PMC   PUSH_AXY
         JSR   MUSIC_TICK
         PMC   ACK_IRQ;IRQ_VBLANK
         PMC   POP_YXA
         RTI
NMI      RTI

; Include code and song before placing the vectors.
         PUT   PLAYER.INC
         PUT   SONG.MUS

         ORG   $FFFA
         DA    NMI,RESET,IRQ

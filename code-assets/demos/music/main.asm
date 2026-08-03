; ---------------------------------------------------
; TRACKER MUSIC DEMO
; ---------------------------------------------------
;
; SONG.MUS is created and edited as a visual tracker.
; It is still assembler source, so PUT embeds it in the
; cartridge. PLAYER.INC writes the real APU registers.

IRQPEND  EQU   $C002
IRQEN    EQU   $C003
BGCOLOR  EQU   $C012

         FIXED
         ORG   $C100
RESET    SEI
         LDX   #<SONG_MUSIC
         LDY   #>SONG_MUSIC
         JSR   MUSIC_START
         LDA   #$25
         STA   BGCOLOR
         LDA   #1
         STA   IRQEN
         CLI
FOREVER  JMP   FOREVER

; The player advances exactly once per VBlank.
IRQ      PHA
         TXA
         PHA
         TYA
         PHA
         JSR   MUSIC_TICK
         LDA   #1
         STA   IRQPEND
         PLA
         TAY
         PLA
         TAX
         PLA
         RTI
NMI      RTI

; Include code and song before placing the vectors.
         PUT   PLAYER.INC
         PUT   SONG.MUS

         ORG   $FFFA
         DA    NMI,RESET,IRQ

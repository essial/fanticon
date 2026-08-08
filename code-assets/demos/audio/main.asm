; ---------------------------------------------------
; TWO PULSE, TRIANGLE, AND NOISE DEMO
; ---------------------------------------------------
;
; Tonal voices have control, timer-low, timer-high, and
; phase-reset registers. A reset write starts waveform
; step zero. Channel enable gates output without
; stopping its oscillator. AUDIO_MASTER enables the
; final mix.

         INCLUDE FANTICON.INC

; ---------------------------------------------------
; HARDWARE REGISTERS AND WORK RAM
; ---------------------------------------------------
NOTE     EQU   $20
COUNT    EQU   $21

; ---------------------------------------------------
; RESET AND CHANNEL SETUP
; ---------------------------------------------------

         FIXED
         ORG   $C100
RESET    SEI

; Pulse 1: enable, 50% duty, volume 12, timer $1BD.
         PMC   SET_TONE;PULSE1_CONTROL;$CC;$1BD

; Pulse 2: enable, 25% duty, volume 8, timer $27A.
         PMC   SET_TONE;PULSE2_CONTROL;$A8;$27A

; Triangle: fixed amplitude, enabled with timer $0DE.
         PMC   SET_TONE;TRI_CONTROL;$80;$0DE

; Noise: volume 8, long LFSR mode, period entry 13.
         PMC   SET_NOISE;$88;13
         PMC   SET_AUDIO_MASTER;15
         LDA   #0
         STA   NOTE
         STA   COUNT
         PMC   SET_IRQS;IRQ_VBLANK
         CLI
LOOP     JMP   LOOP

; ---------------------------------------------------
; VBLANK MUSIC SEQUENCER
; ---------------------------------------------------
;
; VBlank arrives 60 times per second. COUNT divides it
; by 16 before selecting the next pulse-1 timer value.
; BACKDROP_COLOR makes each note change visible.
IRQ      PHA
         TXA
         PHA
         INC   COUNT
         LDA   COUNT
         AND   #15
         BNE   DONE
         INC   NOTE
         LDA   NOTE
         AND   #3
         STA   NOTE
         TAX
         LDA   NOTES,X
         STA   PULSE1_LOW
         STA   PULSE1_RESET
         LDA   COLORS,X
         STA   BACKDROP_COLOR
DONE
         PMC   ACK_IRQ;IRQ_VBLANK
; IRQ_PENDING is write-one-to-clear. Bit 0 acknowledges
; only VBlank.
         PLA
         TAX
         PLA
         RTI
NMI      RTI

; ---------------------------------------------------
; NOTE AND COLOR TABLES
; ---------------------------------------------------
;
; PULSE1_HIGH remains one. These low bytes provide four
; pitches.
NOTES    DFB   $BD,$7C,$52,$34
COLORS   DFB   $E0,$1C,$03,$FF

; ---------------------------------------------------
; INTERRUPT VECTORS
; ---------------------------------------------------

         ORG   $FFFA
         DA    NMI,RESET,IRQ

; ---------------------------------------------------
; TWO PULSE, TRIANGLE, AND NOISE DEMO
; ---------------------------------------------------
;
; Tonal voices have control, timer-low, timer-high, and
; phase-reset registers. A reset write starts waveform
; step zero. Channel enable gates output without
; stopping its oscillator. MASTER bit 7 enables the
; final mix.

; ---------------------------------------------------
; HARDWARE REGISTERS AND WORK RAM
; ---------------------------------------------------
IRQPEND  EQU   $C002
IRQEN    EQU   $C003
BGCOLOR  EQU   $C012
P1CTL    EQU   $C030
P1LO     EQU   $C031
P1HI     EQU   $C032
P1RST    EQU   $C033
P2CTL    EQU   $C034
P2LO     EQU   $C035
P2HI     EQU   $C036
P2RST    EQU   $C037
TRICTL   EQU   $C038
TRILO    EQU   $C039
TRIHI    EQU   $C03A
TRIRST   EQU   $C03B
NOISECTL EQU   $C03C
NOISEPER EQU   $C03D
NOISERST EQU   $C03E
MASTER   EQU   $C040
NOTE     EQU   $20
COUNT    EQU   $21

; Configure one tonal channel. Named parameters keep
; each channel's consecutive register block readable.
SETTONE  MAC   CONTROL;TIMER;BASE
         LDA   #]CONTROL
         STA   ]BASE
         LDA   #<]TIMER
         STA   ]BASE+1
         LDA   #>]TIMER
         STA   ]BASE+2
         STA   ]BASE+3
         EOM

; ---------------------------------------------------
; RESET AND CHANNEL SETUP
; ---------------------------------------------------

         FIXED
         ORG   $C100
RESET    SEI

; Pulse 1: enable, 50% duty, volume 12, timer $1BD.
         PMC   SETTONE;$CC;$1BD;P1CTL

; Pulse 2: enable, 25% duty, volume 8, timer $27A.
         PMC   SETTONE;$A8;$27A;P2CTL

; Triangle: fixed amplitude, enabled with timer $0DE.
         PMC   SETTONE;$80;$0DE;TRICTL

; Noise: volume 8, long LFSR mode, period entry 13.
         LDA   #$88
         STA   NOISECTL
         LDA   #13
         STA   NOISEPER
         STA   NOISERST
         LDA   #$8F
         STA   MASTER
         LDA   #0
         STA   NOTE
         STA   COUNT
         LDA   #1
         STA   IRQEN
         CLI
LOOP     JMP   LOOP

; ---------------------------------------------------
; VBLANK MUSIC SEQUENCER
; ---------------------------------------------------
;
; VBlank arrives 60 times per second. COUNT divides it
; by 16 before selecting the next pulse-1 timer value.
; BGCOLOR makes each note change visible.
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
         STA   P1LO
         STA   P1RST
         LDA   COLORS,X
         STA   BGCOLOR
DONE     LDA   #1
; IRQPEND is write-one-to-clear. Bit 0 acknowledges
; only VBlank.
         STA   IRQPEND
         PLA
         TAX
         PLA
         RTI
NMI      RTI

; ---------------------------------------------------
; NOTE AND COLOR TABLES
; ---------------------------------------------------
;
; P1HI remains one. These low bytes provide four
; pitches.
NOTES    DFB   $BD,$7C,$52,$34
COLORS   DFB   $E0,$1C,$03,$FF

; ---------------------------------------------------
; INTERRUPT VECTORS
; ---------------------------------------------------

         ORG   $FFFA
         DA    NMI,RESET,IRQ

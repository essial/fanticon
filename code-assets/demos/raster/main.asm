; ---------------------------------------------------
; RASTER IRQ COLOR-BAND DEMO
; ---------------------------------------------------
;
; Blank-background mode fills every pixel from BGCOLOR.
; Raster IRQs change that color at lines 50, 100, and
; 150. Line 200 runs in VBlank and prepares the next
; frame.
;
; IRQ bit 1 ($02) is the raster comparator. Writing it
; to IRQPEND clears only that source.

; ---------------------------------------------------
; HARDWARE REGISTERS AND WORK RAM
; ---------------------------------------------------
VMODE    EQU   $C010
BGCOLOR  EQU   $C012
RASTXLO  EQU   $C017
RASTXHI  EQU   $C018
RASTYLO  EQU   $C019
RASTYHI  EQU   $C01A
IRQPEND  EQU   $C002
IRQEN    EQU   $C003
STATE    EQU   $20

; ---------------------------------------------------
; RESET AND VIDEO SETUP
; ---------------------------------------------------

         FIXED
         ORG   $C100
; Fixed ROM is always visible, so reset and IRQ code
; remain safe while other banks are selected.
RESET    SEI
         LDA   #0
         STA   VMODE
         STA   RASTXLO
         STA   RASTXHI
         STA   RASTYHI
         STA   STATE
         LDA   #$03
         STA   BGCOLOR
         LDA   #50
         STA   RASTYLO
         LDA   #2
         STA   IRQEN
         CLI
LOOP     JMP   LOOP

; ---------------------------------------------------
; RASTER INTERRUPT
; ---------------------------------------------------
;
; STATE selects the next band. Each path programs the
; following line, so the comparator automatically
; re-arms for the next boundary.
IRQ      PHA
         LDA   STATE
         BEQ   RED
         CMP   #1
         BEQ   GREEN
         CMP   #2
         BEQ   BLUE
         LDA   #$03
         STA   BGCOLOR
         LDA   #50
         STA   RASTYLO
         LDA   #0
         STA   STATE
         BEQ   ACK
RED      LDA   #$E0
         STA   BGCOLOR
         LDA   #100
         STA   RASTYLO
         INC   STATE
         BNE   ACK
GREEN    LDA   #$1C
         STA   BGCOLOR
         LDA   #150
         STA   RASTYLO
         INC   STATE
         BNE   ACK
BLUE     LDA   #$03
         STA   BGCOLOR
         LDA   #200
         STA   RASTYLO
         INC   STATE
ACK      LDA   #2
         STA   IRQPEND
         PLA
         RTI
NMI      RTI

; ---------------------------------------------------
; INTERRUPT VECTORS
; ---------------------------------------------------
;
; All three vectors are required. Fanticon does not
; drive NMI in v0.1.
         ORG   $FFFA
         DA    NMI,RESET,IRQ

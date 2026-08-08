; ---------------------------------------------------
; RASTER IRQ COLOR-BAND DEMO
; ---------------------------------------------------
;
; Blank mode fills every pixel from BACKDROP_COLOR.
; Raster IRQs change that color at lines 50, 100, and
; 150. Line 200 runs in VBlank and prepares the next
; frame.
;
; IRQ bit 1 ($02) is the raster comparator. Writing it
; to IRQ_PENDING clears only that source.

         INCLUDE FANTICON.INC

; ---------------------------------------------------
; HARDWARE REGISTERS AND WORK RAM
; ---------------------------------------------------
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
         STA   VIDEO_MODE
         STA   RASTER_X_LOW
         STA   RASTER_X_HIGH
         STA   RASTER_Y_HIGH
         STA   STATE
         LDA   #$03
         STA   BACKDROP_COLOR
         LDA   #50
         STA   RASTER_Y_LOW
         PMC   SET_IRQS;IRQ_RASTER
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
         STA   BACKDROP_COLOR
         LDA   #50
         STA   RASTER_Y_LOW
         LDA   #0
         STA   STATE
         BEQ   ACK
RED      LDA   #$E0
         STA   BACKDROP_COLOR
         LDA   #100
         STA   RASTER_Y_LOW
         INC   STATE
         BNE   ACK
GREEN    LDA   #$1C
         STA   BACKDROP_COLOR
         LDA   #150
         STA   RASTER_Y_LOW
         INC   STATE
         BNE   ACK
BLUE     LDA   #$03
         STA   BACKDROP_COLOR
         LDA   #200
         STA   RASTER_Y_LOW
         INC   STATE
ACK
         PMC   ACK_IRQ;IRQ_RASTER
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

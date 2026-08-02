; -------------------------------------------------------
; TILEMAP, VRAM, INPUT, AND VBLANK DEMO
; -------------------------------------------------------
;
; BANKKIND=2 maps a 16 KiB VRAM bank at $8000-$BFFF.
; In VRAM bank 0:
;   $8020 = pattern bytes for tile 1
;   $A000 = 40x25 tile-number map
;   $A400 = 40x25 attribute map
;
; A tile is 8x8 pixels at four bits per pixel. Four
; packed bytes describe each row. The high nibble is
; the left pixel and the low nibble is the right.

; -------------------------------------------------------
; HARDWARE REGISTERS
; -------------------------------------------------------
BANKKIND EQU   $C000
IRQPEND  EQU   $C002
IRQEN    EQU   $C003
VMODE    EQU   $C010
VCTRL    EQU   $C011
SCRXLO   EQU   $C013
SCRXHI   EQU   $C014
SCRYLO   EQU   $C015
SCRYHI   EQU   $C016
PAD      EQU   $C050

; -------------------------------------------------------
; RESET AND PATTERN COPY
; -------------------------------------------------------

         FIXED
         ORG   $C100
RESET    SEI
; Select VRAM bank 0 and copy the 32-byte tile pattern
; from fixed ROM.
         LDA   #2
         STA   BANKKIND
         LDX   #0
COPY     LDA   PATTERN,X
         STA   $8020,X
         INX
         CPX   #32
         BNE   COPY

; -------------------------------------------------------
; TILEMAP SETUP
; -------------------------------------------------------
;
; Three complete 256-byte pages plus 232 bytes fill all
; 1,000 cells. X also selects changing palette banks.
         LDX   #0
MAPLOOP  LDA   #1
         STA   $A000,X
         STA   $A100,X
         STA   $A200,X
         CPX   #$E8
         BCS   NOMAP4
         STA   $A300,X
NOMAP4   TXA
         AND   #$0F
         STA   $A400,X
         STA   $A500,X
         STA   $A600,X
         CPX   #$E8
         BCS   NOATT4
         STA   $A700,X
NOATT4   INX
         BNE   MAPLOOP

; Mode 1 selects tiles. VCTRL bit 0 enables the
; background. IRQEN bit 0 requests one VBlank IRQ per
; frame.
         LDA   #1
         STA   VMODE
         STA   VCTRL
         STA   IRQEN
         CLI
IDLE     JMP   IDLE

; -------------------------------------------------------
; VBLANK INPUT AND SCROLLING
; -------------------------------------------------------
;
; Controller bits 0-3 are Up, Down, Left, and Right.
; The handler updates both bytes of each 16-bit scroll
; coordinate. Video hardware wraps them modulo 320x200.
IRQ      PHA
         LDA   PAD
         AND   #4
         BEQ   NOLEFT
         LDA   SCRXLO
         BNE   LEFTLO
         DEC   SCRXHI
LEFTLO   DEC   SCRXLO
NOLEFT   LDA   PAD
         AND   #8
         BEQ   NORIGHT
         INC   SCRXLO
         BNE   NORIGHT
         INC   SCRXHI
NORIGHT  LDA   PAD
         AND   #1
         BEQ   NOUP
         LDA   SCRYLO
         BNE   UPLO
         DEC   SCRYHI
UPLO     DEC   SCRYLO
NOUP     LDA   PAD
         AND   #2
         BEQ   NODOWN
         INC   SCRYLO
         BNE   NODOWN
         INC   SCRYHI
NODOWN   LDA   #1
         STA   IRQPEND
         PLA
         RTI
NMI      RTI

; -------------------------------------------------------
; TILE PATTERN DATA
; -------------------------------------------------------
;
; Rotating nibbles make scrolling and tile boundaries
; easy to see with the identity RGB332 palette.
PATTERN  HEX   12345678
         HEX   23456781
         HEX   34567812
         HEX   45678123
         HEX   56781234
         HEX   67812345
         HEX   78123456
         HEX   81234567

; -------------------------------------------------------
; INTERRUPT VECTORS
; -------------------------------------------------------
;
; Fixed-image offsets $3FFA-$3FFF map to CPU addresses
; $FFFA-$FFFF.
         ORG   $FFFA
         DA    NMI,RESET,IRQ

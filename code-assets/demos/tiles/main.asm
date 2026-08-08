; ---------------------------------------------------
; TILEMAP, VRAM, INPUT, AND VBLANK DEMO
; ---------------------------------------------------
;
; BANK_KIND maps a 16 KiB VRAM bank at $8000-$BFFF.
; In VRAM bank 0:
;   $8020 = pattern bytes for tile 1
;   $A000 = 64x32 tile-number map
;   $A800 = 64x32 attribute map
;
; A tile is 8x8 pixels at four bits per pixel. Four
; packed bytes describe each row. The high nibble is
; the left pixel and the low nibble is the right.

         INCLUDE FANTICON.INC

; ---------------------------------------------------
; HARDWARE REGISTERS
; ---------------------------------------------------
; ---------------------------------------------------
; RESET AND PATTERN COPY
; ---------------------------------------------------

         FIXED
         ORG   $C100
RESET    SEI
; Select VRAM bank 0 and copy the 32-byte tile pattern
; from fixed ROM.
         LDA   #BANK_VRAM
         STA   BANK_KIND
         PMC   UPLOAD_TILE;1;PATTERN

; ---------------------------------------------------
; TILEMAP SETUP
; ---------------------------------------------------
;
; Eight complete 256-byte pages fill all 2,048 cells.
; X also selects changing palette banks.
         LDX   #0
MAPLOOP  LDA   #1
         REPEAT 8;PAGE
         STA   VRAM_MAP_CPU+]PAGE*$100,X
         ENDREP
         TXA
         AND   #$0F
         REPEAT 8;PAGE
         STA   VRAM_ATTR_CPU+]PAGE*$100,X
         ENDREP
         INX
         BNE   MAPLOOP

; VIDEO_TILEMAP selects tiles. VIDEO_BG enables the
; background. IRQ_VBLANK requests one interrupt per
; frame.
         LDA   #VIDEO_TILEMAP
         STA   VIDEO_MODE
         STA   VIDEO_CONTROL
         STA   IRQ_ENABLE
         CLI
IDLE     JMP   IDLE

; ---------------------------------------------------
; VBLANK INPUT AND SCROLLING
; ---------------------------------------------------
;
; Controller bits 0-3 are Up, Down, Left, and Right.
; The handler updates both bytes of each 16-bit scroll
; coordinate. Video hardware wraps them modulo 512x256.
IRQ      PHA
         LDA   PAD0_STATE
         AND   #PAD_LEFT
         BEQ   NOLEFT
         LDA   SCROLL_X_LOW
         BNE   LEFTLO
         DEC   SCROLL_X_HIGH
LEFTLO   DEC   SCROLL_X_LOW
NOLEFT   LDA   PAD0_STATE
         AND   #PAD_RIGHT
         BEQ   NORIGHT
         INC   SCROLL_X_LOW
         BNE   NORIGHT
         INC   SCROLL_X_HIGH
NORIGHT  LDA   PAD0_STATE
         AND   #PAD_UP
         BEQ   NOUP
         LDA   SCROLL_Y_LOW
         BNE   UPLO
         DEC   SCROLL_Y_HIGH
UPLO     DEC   SCROLL_Y_LOW
NOUP     LDA   PAD0_STATE
         AND   #PAD_DOWN
         BEQ   NODOWN
         INC   SCROLL_Y_LOW
         BNE   NODOWN
         INC   SCROLL_Y_HIGH
NODOWN
         PMC   ACK_IRQ;IRQ_VBLANK
         PLA
         RTI
NMI      RTI

; ---------------------------------------------------
; TILE PATTERN DATA
; ---------------------------------------------------
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

; ---------------------------------------------------
; INTERRUPT VECTORS
; ---------------------------------------------------
;
; Fixed-image offsets $3FFA-$3FFF map to CPU addresses
; $FFFA-$FFFF.
         ORG   $FFFA
         DA    NMI,RESET,IRQ

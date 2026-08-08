; ---------------------------------------------------
; GRAPHICS ASSET LOADING DEMO
; ---------------------------------------------------
;
; GAME.PAL and SCENE.GFX are valid assembler sources
; and visual editor documents. PUT stores their bytes
; in cartridge ROM bank 0.
;
; Cartridge ROM and VRAM share the $8000-$BFFF CPU
; window. COPYVRAM stages one page in normal RAM
; before switching the window from ROM to VRAM.
;
; Arrow keys explore the 64x32 circular map through
; the 40x25 viewport. More scenery starts outside the
; initial view to demonstrate its streaming margin.

         INCLUDE FANTICON.INC

; ---------------------------------------------------
; HARDWARE REGISTERS AND LOADER RAM
; ---------------------------------------------------
SRC      EQU   $20
DST      EQU   $22
LEN      EQU   $24
BUFFER   EQU   $0200

; ---------------------------------------------------
; CARTRIDGE GRAPHICS DATA
; ---------------------------------------------------
;
; A tilemap .GFX occupies 12,288 bytes: 8 KiB of
; patterns, 2,048 map bytes, and 2,048 attributes.
; GAME.PAL is separate so graphics sets can share it.
         BANK  0
         ORG   $8000
         PUT   GAME.PAL
         PUT   SCENE.GFX

; ---------------------------------------------------
; RESET AND PALETTE UPLOAD
; ---------------------------------------------------
         FIXED
         ORG   $C100
RESET    SEI
         CLD
         LDA   #BANK_CARTRIDGE ; Cartridge ROM bank 0
         STA   BANK_KIND
         STA   BANK_NUMBER
; PALETTE_DATA advances after every write.
         STA   PALETTE_INDEX
         LDX   #0
PALCOPY  LDA   GAME_PAL,X
         STA   PALETTE_DATA
         INX
; X wrapping copies all 256 colors.
         BNE   PALCOPY

; ---------------------------------------------------
; COPY THE THREE .GFX BLOCKS TO THEIR VRAM DESTINATIONS
; ---------------------------------------------------
;
; COPYVRAM takes source, destination, and a 16-bit
; length in zero page. CPU destinations $8000,
; $A000, and $A800 map to VRAM offsets $0000,
; $2000, and $2800 in VRAM bank 0.
         PMC   STORE16;SRC;SCENE_CHR
         PMC   STORE16;DST;$8000
; Pattern length is $2000.
         PMC   STORE16;LEN;$2000
         JSR   COPYVRAM

         PMC   STORE16;SRC;SCENE_MAP
         PMC   STORE16;DST;$A000
; Map length is $0800 (2,048 bytes).
         PMC   STORE16;LEN;$0800
         JSR   COPYVRAM

         PMC   STORE16;SRC;SCENE_ATR
         PMC   STORE16;DST;$A800
; Attribute length is also $0800.
         PMC   STORE16;LEN;$0800
         JSR   COPYVRAM

; ---------------------------------------------------
; CREATE ONE 16X16 HARDWARE SPRITE
; ---------------------------------------------------
;
; Pattern 4 begins the aligned 4,5,6,7 composite.
; ATTR=$C1 means enabled, 16x16, palette bank 1.
         PMC   SET_SPRITE;0;152;96;4;$C1

; Enable tile mode, background, and sprite layers.
         LDA   #VIDEO_TILEMAP
         STA   VIDEO_MODE
         LDA   #VIDEO_ALL
         STA   VIDEO_CONTROL
         PMC   SET_IRQS;IRQ_VBLANK
         CLI
IDLE     JMP   IDLE

; ---------------------------------------------------
; ROM-TO-VRAM BLOCK COPY
; ---------------------------------------------------
;
; LEN+1 counts full 256-byte pages. LEN is the
; final partial page. Source data stays in cartridge
; bank 0 and all destinations stay in VRAM bank 0.
         PMC EMIT_VRAM_COPY;COPYVRAM;SRC;DST;LEN;BUFFER

; ---------------------------------------------------
; VBLANK INPUT AND FOUR-DIRECTION SCROLLING
; ---------------------------------------------------
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
; INTERRUPT VECTORS
; ---------------------------------------------------
         ORG   $FFFA
         DA    NMI,RESET,IRQ

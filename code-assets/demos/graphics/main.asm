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

; ---------------------------------------------------
; HARDWARE REGISTERS AND LOADER RAM
; ---------------------------------------------------
BANKKIND EQU   $C000
BANKNUM  EQU   $C001
IRQPEND  EQU   $C002
IRQEN    EQU   $C003
VMODE    EQU   $C010
VCTRL    EQU   $C011
SCRXLO   EQU   $C013
SCRXHI   EQU   $C014
SCRYLO   EQU   $C015
SCRYHI   EQU   $C016
PALINDEX EQU   $C01B
PALDATA  EQU   $C01C
PAD      EQU   $C050

SRCLO    EQU   $20
SRCHI    EQU   $21
DSTLO    EQU   $22
DSTHI    EQU   $23
LENLO    EQU   $24
LENHI    EQU   $25
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
         LDA   #0              ; Cartridge ROM bank 0
         STA   BANKKIND
         STA   BANKNUM
; PALDATA advances after every write.
         STA   PALINDEX
         LDX   #0
PALCOPY  LDA   GAME_PAL,X
         STA   PALDATA
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
         LDA   #<SCENE_CHR
         STA   SRCLO
         LDA   #>SCENE_CHR
         STA   SRCHI
         LDA   #$00
         STA   DSTLO
         LDA   #$80
         STA   DSTHI
; Pattern length is $2000.
         LDA   #$00
         STA   LENLO
         LDA   #$20
         STA   LENHI
         JSR   COPYVRAM

         LDA   #<SCENE_MAP
         STA   SRCLO
         LDA   #>SCENE_MAP
         STA   SRCHI
         LDA   #$00
         STA   DSTLO
         LDA   #$A0
         STA   DSTHI
; Map length is $0800 (2,048 bytes).
         LDA   #$00
         STA   LENLO
         LDA   #$08
         STA   LENHI
         JSR   COPYVRAM

         LDA   #<SCENE_ATR
         STA   SRCLO
         LDA   #>SCENE_ATR
         STA   SRCHI
         LDA   #$00
         STA   DSTLO
         LDA   #$A8
         STA   DSTHI
; Attribute length is also $0800.
         LDA   #$00
         STA   LENLO
         LDA   #$08
         STA   LENHI
         JSR   COPYVRAM

; ---------------------------------------------------
; CREATE ONE 16X16 HARDWARE SPRITE
; ---------------------------------------------------
;
; Pattern 4 begins the aligned 4,5,6,7 composite.
; ATTR=$C1 means enabled, 16x16, palette bank 1.
         LDA   #152
         STA   $B000           ; X low
         LDA   #0
; X bit 8 and behind flag.
         STA   $B001
         LDA   #96
         STA   $B002           ; Y
         LDA   #4
         STA   $B003           ; First pattern
         LDA   #$C1
; Enable, 16x16 size, and palette 1.
         STA   $B004

; Enable tile mode, background, and sprite layers.
         LDA   #1
         STA   VMODE
         LDA   #3
         STA   VCTRL
         LDA   #1
         STA   IRQEN
         CLI
IDLE     JMP   IDLE

; ---------------------------------------------------
; ROM-TO-VRAM BLOCK COPY
; ---------------------------------------------------
;
; LENHI counts full 256-byte pages. LENLO is the
; final partial page. Source data stays in cartridge
; bank 0 and all destinations stay in VRAM bank 0.
COPYVRAM LDA   LENHI
         BEQ   COPYTAIL
COPYPAGE LDA   #0
         STA   BANKKIND        ; Expose cartridge data
         STA   BANKNUM
         LDY   #0
READPAGE LDA   (SRCLO),Y
         STA   BUFFER,Y
         INY
         BNE   READPAGE
         LDA   #2
; Expose the VRAM destination.
         STA   BANKKIND
         LDA   #0
         STA   BANKNUM
         LDY   #0
WRITEPGE LDA   BUFFER,Y
         STA   (DSTLO),Y
         INY
         BNE   WRITEPGE
         INC   SRCHI
         INC   DSTHI
         DEC   LENHI
         BNE   COPYPAGE

COPYTAIL LDA   LENLO
         BEQ   COPYDONE
         LDA   #0
         STA   BANKKIND
         STA   BANKNUM
         LDY   #0
READTAIL LDA   (SRCLO),Y
         STA   BUFFER,Y
         INY
         CPY   LENLO
         BNE   READTAIL
         LDA   #2
         STA   BANKKIND
         LDA   #0
         STA   BANKNUM
         LDY   #0
WRITETAL LDA   BUFFER,Y
         STA   (DSTLO),Y
         INY
         CPY   LENLO
         BNE   WRITETAL
COPYDONE RTS

; ---------------------------------------------------
; VBLANK INPUT AND FOUR-DIRECTION SCROLLING
; ---------------------------------------------------
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

; ---------------------------------------------------
; INTERRUPT VECTORS
; ---------------------------------------------------
         ORG   $FFFA
         DA    NMI,RESET,IRQ

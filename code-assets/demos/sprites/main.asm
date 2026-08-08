; ---------------------------------------------------
; SPRITE, CLIPPING, AND CONTROLLER DEMO
; ---------------------------------------------------
;
; Sprite records occupy VRAM offsets $3000-$30FF. In
; bank 0 they appear at $B000. Record 0 uses:
;   +0 X low       +1 X bit 8 and priority
;   +2 Y           +3 tile       +4 attributes
;
; Attribute $82 enables an 8x8 sprite in palette bank
; 2. Color zero is transparent. Other pixels use
; palette entries $21-$2F.

         INCLUDE FANTICON.INC

; ---------------------------------------------------
; HARDWARE REGISTERS AND WORK RAM
; ---------------------------------------------------
XPOS     EQU   $20
XFLAG    EQU   $21
YPOS     EQU   $22

; ---------------------------------------------------
; RESET AND PATTERN COPY
; ---------------------------------------------------

         FIXED
         ORG   $C100
RESET    SEI
; Copy tile 1's 32 packed bytes to VRAM offset $0020.
         LDA   #BANK_VRAM
         STA   BANK_KIND
         PMC   UPLOAD_TILE;1;PATTERN

; ---------------------------------------------------
; SPRITE SETUP
; ---------------------------------------------------
;
; X is a nine-bit coordinate split across two bytes; Y
; is eight-bit. $1F0-$1FF and $F0-$FF are negative
; clipped positions.
         LDA   #156
         STA   XPOS
         STA   VRAM_SPR_CPU+SPR_X_LOW
         LDA   #0
         STA   XFLAG
         STA   VRAM_SPR_CPU+SPR_X_FLAGS
         LDA   #96
         STA   YPOS
         STA   VRAM_SPR_CPU+SPR_Y
         LDA   #1
         STA   VRAM_SPR_CPU+SPR_TILE
         LDA   #SPR_ENABLE+2
         STA   VRAM_SPR_CPU+SPR_ATTR
         LDA   #$49
         STA   BACKDROP_COLOR
         LDA   #VIDEO_SPRITES
         STA   VIDEO_CONTROL
         PMC   SET_IRQS;IRQ_VBLANK
         CLI
IDLE     JMP   IDLE

; ---------------------------------------------------
; VBLANK MOVEMENT
; ---------------------------------------------------
;
; Movement occurs once per VBlank. Crossing X=$000
; toggles bit 8 and naturally produces $1FF (-1). The
; renderer clips at every edge instead of wrapping.
IRQ      PHA
         LDA   PAD0_STATE
         AND   #PAD_LEFT
         BEQ   NOLEFT
         LDA   XPOS
         BNE   LEFTLO
         LDA   XFLAG
         EOR   #1
         STA   XFLAG
LEFTLO   DEC   XPOS
NOLEFT   LDA   PAD0_STATE
         AND   #PAD_RIGHT
         BEQ   NORIGHT
         INC   XPOS
         BNE   NORIGHT
         LDA   XFLAG
         EOR   #1
         STA   XFLAG
NORIGHT  LDA   PAD0_STATE
         AND   #PAD_UP
         BEQ   NOUP
         DEC   YPOS
NOUP     LDA   PAD0_STATE
         AND   #PAD_DOWN
         BEQ   NODOWN
         INC   YPOS
NODOWN   LDA   XPOS
; Records are sampled at each scanline start. These
; writes affect the next scanline snapshot.
         STA   VRAM_SPR_CPU+SPR_X_LOW
         LDA   XFLAG
         STA   VRAM_SPR_CPU+SPR_X_FLAGS
         LDA   YPOS
         STA   VRAM_SPR_CPU+SPR_Y
         PMC   ACK_IRQ;IRQ_VBLANK
         PLA
         RTI
NMI      RTI

; ---------------------------------------------------
; SPRITE PATTERN DATA
; ---------------------------------------------------
;
; Four packed bytes encode each row of this shape.
PATTERN  HEX   000FF000
         HEX   00FFFF00
         HEX   0FF00FF0
         HEX   FF0FF0FF
         HEX   FF0FF0FF
         HEX   0FF00FF0
         HEX   00FFFF00
         HEX   000FF000

; ---------------------------------------------------
; INTERRUPT VECTORS
; ---------------------------------------------------

         ORG   $FFFA
         DA    NMI,RESET,IRQ

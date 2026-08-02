; -------------------------------------------------------
; SPRITE, CLIPPING, AND CONTROLLER DEMO
; -------------------------------------------------------
;
; Sprite records occupy VRAM offsets $2800-$28FF. In
; bank 0 they appear at $A800. Record 0 uses:
;   +0 X low       +1 X bit 8 and priority
;   +2 Y           +3 tile       +4 attributes
;
; Attribute $82 enables an 8x8 sprite in palette bank
; 2. Color zero is transparent. Other pixels use
; palette entries $21-$2F.

; -------------------------------------------------------
; HARDWARE REGISTERS AND WORK RAM
; -------------------------------------------------------
BANKKIND EQU   $C000
IRQPEND  EQU   $C002
IRQEN    EQU   $C003
VCTRL    EQU   $C011
BGCOLOR  EQU   $C012
PAD      EQU   $C050
XPOS     EQU   $20
XFLAG    EQU   $21
YPOS     EQU   $22

; -------------------------------------------------------
; RESET AND PATTERN COPY
; -------------------------------------------------------

         FIXED
         ORG   $C100
RESET    SEI
; Copy tile 1's 32 packed bytes to VRAM offset $0020.
         LDA   #2
         STA   BANKKIND
         LDX   #0
COPY     LDA   PATTERN,X
         STA   $8020,X
         INX
         CPX   #32
         BNE   COPY

; -------------------------------------------------------
; SPRITE SETUP
; -------------------------------------------------------
;
; X is a nine-bit coordinate split across two bytes; Y
; is eight-bit. $1F0-$1FF and $F0-$FF are negative
; clipped positions.
         LDA   #156
         STA   XPOS
         STA   $A800
         LDA   #0
         STA   XFLAG
         STA   $A801
         LDA   #96
         STA   YPOS
         STA   $A802
         LDA   #1
         STA   $A803
         LDA   #$82
         STA   $A804
         LDA   #$49
         STA   BGCOLOR
         LDA   #2
         STA   VCTRL
         LDA   #1
         STA   IRQEN
         CLI
IDLE     JMP   IDLE

; -------------------------------------------------------
; VBLANK MOVEMENT
; -------------------------------------------------------
;
; Movement occurs once per VBlank. Crossing X=$000
; toggles bit 8 and naturally produces $1FF (-1). The
; renderer clips at every edge instead of wrapping.
IRQ      PHA
         LDA   PAD
         AND   #4
         BEQ   NOLEFT
         LDA   XPOS
         BNE   LEFTLO
         LDA   XFLAG
         EOR   #1
         STA   XFLAG
LEFTLO   DEC   XPOS
NOLEFT   LDA   PAD
         AND   #8
         BEQ   NORIGHT
         INC   XPOS
         BNE   NORIGHT
         LDA   XFLAG
         EOR   #1
         STA   XFLAG
NORIGHT  LDA   PAD
         AND   #1
         BEQ   NOUP
         DEC   YPOS
NOUP     LDA   PAD
         AND   #2
         BEQ   NODOWN
         INC   YPOS
NODOWN   LDA   XPOS
; Records are sampled at each scanline start. These
; writes affect the next scanline snapshot.
         STA   $A800
         LDA   XFLAG
         STA   $A801
         LDA   YPOS
         STA   $A802
         LDA   #1
         STA   IRQPEND
         PLA
         RTI
NMI      RTI

; -------------------------------------------------------
; SPRITE PATTERN DATA
; -------------------------------------------------------
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

; -------------------------------------------------------
; INTERRUPT VECTORS
; -------------------------------------------------------

         ORG   $FFFA
         DA    NMI,RESET,IRQ

; -------------------------------------------------------
; PACKED BITMAP AND VRAM-BANK DEMO
; -------------------------------------------------------
;
; The 320x200 bitmap packs two pixels per byte, for
; 32,000 bytes total. It begins at VRAM offset $4000.
; The first 16 KiB uses VRAM bank 1; the remaining
; 15,616 bytes use bank 2. Both map at $8000-$BFFF.
;
; PTR/PTRHI form a zero-page indirect pointer. Y fills
; one 256-byte page before PTRHI advances.

; -------------------------------------------------------
; HARDWARE REGISTERS AND WORK RAM
; -------------------------------------------------------
BANKKIND EQU   $C000
BANKNUM  EQU   $C001
VMODE    EQU   $C010
VCTRL    EQU   $C011
BMPPAL   EQU   $C01D
PTR      EQU   $20
PTRHI    EQU   $21

; -------------------------------------------------------
; RESET AND VRAM BANK 1
; -------------------------------------------------------

         FIXED
         ORG   $C100
RESET    SEI
; BANKKIND=2 selects VRAM. BANKNUM=1 exposes bitmap
; offsets $4000-$7FFF.
         LDA   #2
         STA   BANKKIND
         LDA   #1
         STA   BANKNUM
         LDA   #0
         STA   PTR
         LDA   #$80
         STA   PTRHI
         LDY   #0
FILL1    TYA
         EOR   PTRHI
         STA   (PTR),Y
         INY
         BNE   FILL1
         INC   PTRHI
         LDA   PTRHI
         CMP   #$C0
         BNE   FILL1

; -------------------------------------------------------
; VRAM BANK 2
; -------------------------------------------------------
;
; Stop at pointer high byte $BD after writing pages
; $80-$BC: 61 pages, or the remaining 15,616 bytes.
         LDA   #2
         STA   BANKNUM
         LDA   #$80
         STA   PTRHI
         LDY   #0
FILL2    TYA
         EOR   PTRHI
         EOR   #$FF
         STA   (PTR),Y
         INY
         BNE   FILL2
         INC   PTRHI
         LDA   PTRHI
         CMP   #$BD
         BNE   FILL2

; -------------------------------------------------------
; ENABLE BITMAP DISPLAY
; -------------------------------------------------------
;
; Mode 2 selects packed bitmap fetches. Palette bank 0
; makes each nibble select reset entries $00-$0F.
         LDA   #0
         STA   BMPPAL
         LDA   #2
         STA   VMODE
         LDA   #1
         STA   VCTRL
LOOP     JMP   LOOP
NMI      RTI
IRQ      RTI

; -------------------------------------------------------
; INTERRUPT VECTORS
; -------------------------------------------------------

         ORG   $FFFA
         DA    NMI,RESET,IRQ

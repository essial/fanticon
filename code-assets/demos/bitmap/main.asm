; ---------------------------------------------------
; PACKED BITMAP AND VRAM-BANK DEMO
; ---------------------------------------------------
;
; The 320x200 bitmap packs two pixels per byte, for
; 32,000 bytes total. It begins at VRAM offset $4000.
; The first 16 KiB uses VRAM bank 1; the remaining
; 15,616 bytes use bank 2. Both map at $8000-$BFFF.
;
; PTR/PTRHI form a zero-page indirect pointer. Y fills
; one 256-byte page before PTRHI advances.

         INCLUDE FANTICON.INC

; ---------------------------------------------------
; HARDWARE REGISTERS AND WORK RAM
; ---------------------------------------------------
PTR      EQU   $20
PTRHI    EQU   $21

; FILLVRAM demonstrates named/defaulted parameters,
; compile-time IF, and hygienic @LOCAL labels. Each
; call receives its own private loop label.
FILLVRAM MAC   BANK;STOP;RESETPTR=0;INVERT=0
         LDA   #]BANK
         STA   BANK_NUMBER
         IF    ]RESETPTR
         LDA   #0
         STA   PTR
         ENDIF
         LDA   #$80
         STA   PTRHI
         LDY   #0
@LOOP    TYA
         EOR   PTRHI
         IF    ]INVERT
         EOR   #$FF
         ENDIF
         STA   (PTR),Y
         INY
         BNE   @LOOP
         INC   PTRHI
         LDA   PTRHI
         CMP   #]STOP
         BNE   @LOOP
         EOM

; ---------------------------------------------------
; RESET AND VRAM BANK 1
; ---------------------------------------------------

         FIXED
         ORG   $C100
RESET    SEI
; BANK_KIND selects VRAM. BANK_NUMBER=1 exposes bitmap
; offsets $4000-$7FFF.
         LDA   #BANK_VRAM
         STA   BANK_KIND
         PMC   FILLVRAM;1;$C0;1

; ---------------------------------------------------
; VRAM BANK 2
; ---------------------------------------------------
;
; Stop at pointer high byte $BD after writing pages
; $80-$BC: 61 pages, or the remaining 15,616 bytes.
         PMC   FILLVRAM;2;$BD;0;1

; ---------------------------------------------------
; ENABLE BITMAP DISPLAY
; ---------------------------------------------------
;
; Mode 2 selects packed bitmap fetches. Palette bank 0
; makes each nibble select reset entries $00-$0F.
         PMC   SET_BITMAP;0
LOOP     JMP   LOOP
NMI      RTI
IRQ      RTI

; ---------------------------------------------------
; INTERRUPT VECTORS
; ---------------------------------------------------

         ORG   $FFFA
         DA    NMI,RESET,IRQ

; -------------------------------------------------------
; RASTER WAVE DEMO
; -------------------------------------------------------
;
; This demo changes SCROLL_X once per scanline along a
; 128-step sine curve. SCROLL_Y follows the curve once
; per frame, one quarter cycle ahead of X. Both axes move
; smoothly without discontinuities between raster lines.
;
; The comparator fires at dot 220. IRQ entry plus handler
; work delays the scroll write until HBlank, so the new
; value is ready before the next line begins.

; -------------------------------------------------------
; HARDWARE REGISTERS AND WORK RAM
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
RASTXLO  EQU   $C017
RASTXHI  EQU   $C018
RASTYLO  EQU   $C019
RASTYHI  EQU   $C01A
PALINDEX EQU   $C01B
PALDATA  EQU   $C01C

PHASE    EQU   $20
SCANLINE EQU   $21

; -------------------------------------------------------
; RESET AND TILE PATTERN SETUP
; -------------------------------------------------------

         FIXED
         ORG   $C100
RESET    SEI
         CLD
         LDX   #$FF
         TXS

; Map VRAM bank 0 at $8000-$BFFF. Tile pattern 1 begins
; at $8020 because every four-bit 8x8 tile uses 32 bytes.
         LDA   #2
         STA   BANKKIND
         LDX   #0
COPYTILE LDA   PATTERN,X
         STA   $8020,X
         INX
         CPX   #32
         BNE   COPYTILE

; Fill all 1,000 tile cells with pattern 1. Palette bank
; zero is used throughout, so pixel values 1, 2, and 3
; select the RGB entries configured below.
         LDX   #0
FILLMAP  LDA   #1
         STA   $A000,X
         STA   $A100,X
         STA   $A200,X
         CPX   #$E8
         BCS   NOMAP4
         STA   $A300,X
NOMAP4   LDA   #0
         STA   $A400,X
         STA   $A500,X
         STA   $A600,X
         CPX   #$E8
         BCS   NOATTR4
         STA   $A700,X
NOATTR4  INX
         BNE   FILLMAP

; Tile pixels select palette indexes. Map indexes 1-3 to
; pure RGB332 red, green, and blue. PALDATA automatically
; advances PALINDEX after every write.
         LDA   #1
         STA   PALINDEX
         LDA   #$E0
         STA   PALDATA
         LDA   #$1C
         STA   PALDATA
         LDA   #$03
         STA   PALDATA

; -------------------------------------------------------
; VIDEO AND FIRST RASTER EVENT
; -------------------------------------------------------
;
; Mode 1 selects the tilemap and VCTRL bit 0 enables it.
; The first comparator waits at line 261 so the first
; visible frame begins with line zero prepared.
         LDA   #1
         STA   VMODE
         STA   VCTRL
         LDA   #0
         STA   PHASE
         STA   SCRXHI
         STA   SCRYHI
         LDA   #$FF
         STA   SCANLINE
         LDA   #220
         STA   RASTXLO
         LDA   #0
         STA   RASTXHI
         LDA   #5
         STA   RASTYLO
         LDA   #1
         STA   RASTYHI
         LDA   #2
         STA   IRQEN
         CLI
IDLE     JMP   IDLE

; -------------------------------------------------------
; RASTER INTERRUPT
; -------------------------------------------------------
;
; SCANLINE records the line whose comparator just fired.
; $FF means line 261, the final VBlank line. SETWAVE
; always prepares the following visible line.
IRQ      PHA
         TXA
         PHA
         LDX   SCANLINE
         CPX   #$FF
         BEQ   NEWFRAME
         CPX   #199
         BEQ   WAITFRAME

; Prepare the next visible scanline and move the
; comparator down by one line.
         INX
         STX   SCANLINE
         JSR   SETWAVE
         LDA   SCANLINE
         STA   RASTYLO
         LDA   #0
         STA   RASTYHI
         JMP   IRQDONE

; Visible line 199 is followed by VBlank. Wait for line
; 261 before restarting.
WAITFRAME
         LDA   #$FF
         STA   SCANLINE
         LDA   #5
         STA   RASTYLO
         LDA   #1
         STA   RASTYHI
         JMP   IRQDONE

; Advance both axes once per frame. Y has a quarter-cycle
; phase offset. It stays constant for the frame; changing
; it per line would fold the image.
NEWFRAME INC   PHASE
         LDX   #0
         STX   SCANLINE
         JSR   SETWAVE
         LDA   PHASE
         CLC
         ADC   #32
         AND   #127
         TAX
         LDA   WAVETAB,X
         STA   SCRYLO
         LDA   #0
         STA   RASTYLO
         STA   RASTYHI

; Raster IRQ is bit 1. IRQPEND is write-one-to-clear.
; Other IRQ sources stay pending if a game enables them.
IRQDONE  LDA   #2
         STA   IRQPEND
         PLA
         TAX
         PLA
         RTI

; -------------------------------------------------------
; WAVE LOOKUP
; -------------------------------------------------------
;
; X is the next scanline. PHASE moves the 128-entry curve
; each frame. Adjacent entries differ by at most two
; pixels, avoiding the old coarse steps.
SETWAVE  TXA
         CLC
         ADC   PHASE
         AND   #127
         TAX
         LDA   WAVETAB,X
         STA   SCRXLO
         RTS

WAVETAB  HEX   2021222425262728
         HEX   292A2B2C2D2E2F30
         HEX   3132333334353536
         HEX   3637373738383838
         HEX   3838383838373737
         HEX   3636353534333332
         HEX   31302F2E2D2C2B2A
         HEX   2928272625242221
         HEX   201F1E1C1B1A1918
         HEX   1716151413121110
         HEX   0F0E0D0D0C0B0B0A
         HEX   0A09090908080808
         HEX   0808080808090909
         HEX   0A0A0B0B0C0D0D0E
         HEX   0F10111213141516
         HEX   1718191A1B1C1E1F

; Colors vary across both axes. The raster curve bends
; vertical boundaries, while the frame curve moves
; horizontal boundaries smoothly up and down.
PATTERN  HEX   11122333
         HEX   11122333
         HEX   22233111
         HEX   22233111
         HEX   33311222
         HEX   33311222
         HEX   11223331
         HEX   11223331

NMI      RTI

; -------------------------------------------------------
; INTERRUPT VECTORS
; -------------------------------------------------------
         ORG   $FFFA
         DA    NMI,RESET,IRQ

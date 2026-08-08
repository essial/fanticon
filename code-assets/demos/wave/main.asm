; ---------------------------------------------------
; RASTER WAVE DEMO
; ---------------------------------------------------
;
; This demo changes SCROLL_X once per scanline along a
; 128-step sine curve. SCROLL_Y follows the curve once
; per frame, one quarter cycle ahead of X. Both axes
; move smoothly without discontinuities between lines.
;
; The comparator fires at dot 220. IRQ entry and work
; delay the scroll write until HBlank, so the new value
; is ready before the next line begins.

         INCLUDE FANTICON.INC

; ---------------------------------------------------
; HARDWARE REGISTERS AND WORK RAM
; ---------------------------------------------------
PHASE    EQU   $20
SCANLINE EQU   $21

; ---------------------------------------------------
; RESET AND TILE PATTERN SETUP
; ---------------------------------------------------

         FIXED
         ORG   $C100
RESET    SEI
         CLD
         LDX   #$FF
         TXS

; Map VRAM bank 0 at $8000-$BFFF. Tile pattern 1
; begins at $8020. Every four-bit 8x8 tile uses
; 32 bytes.
         LDA   #BANK_VRAM
         STA   BANK_KIND
         PMC   UPLOAD_TILE;1;PATTERN

; Fill all 2,048 tile cells with pattern 1. Palette
; bank zero is used throughout, so pixel values 1, 2,
; and 3 select the RGB entries configured below.
         PMC   FILL_TILEMAP;1;0

; Tile pixels select palette indexes. Map indexes 1-3
; to pure RGB332 red, green, and blue. PALETTE_DATA
; advances PALETTE_INDEX after every write.
         LDA   #1
         STA   PALETTE_INDEX
         LDA   #$E0
         STA   PALETTE_DATA
         LDA   #$1C
         STA   PALETTE_DATA
         LDA   #$03
         STA   PALETTE_DATA

; ---------------------------------------------------
; VIDEO AND FIRST RASTER EVENT
; ---------------------------------------------------
;
; VIDEO_TILEMAP and VIDEO_BG enable the tile layer.
; The first comparator waits at line 261 so the first
; visible frame begins with line zero prepared.
         LDA   #1
         STA   VIDEO_MODE
         STA   VIDEO_CONTROL
         LDA   #0
         STA   PHASE
         STA   SCROLL_X_HIGH
         STA   SCROLL_Y_HIGH
         LDA   #$FF
         STA   SCANLINE
         LDA   #220
         STA   RASTER_X_LOW
         LDA   #0
         STA   RASTER_X_HIGH
         LDA   #5
         STA   RASTER_Y_LOW
         LDA   #1
         STA   RASTER_Y_HIGH
         PMC   SET_IRQS;IRQ_RASTER
         CLI
IDLE     JMP   IDLE

; ---------------------------------------------------
; RASTER INTERRUPT
; ---------------------------------------------------
;
; SCANLINE records the comparator line that just fired.
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
         STA   RASTER_Y_LOW
         LDA   #0
         STA   RASTER_Y_HIGH
         JMP   IRQDONE

; Visible line 199 is followed by VBlank. Wait for line
; 261 before restarting.
WAITFRAME
         LDA   #$FF
         STA   SCANLINE
         LDA   #5
         STA   RASTER_Y_LOW
         LDA   #1
         STA   RASTER_Y_HIGH
         JMP   IRQDONE

; Advance both axes once per frame. Y has a quarter
; cycle offset. It stays constant for the frame;
; changing it per line would fold the image.
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
         STA   SCROLL_Y_LOW
         LDA   #0
         STA   RASTER_Y_LOW
         STA   RASTER_Y_HIGH

; IRQ_RASTER is write-one-to-clear in IRQ_PENDING.
; Other IRQ sources stay pending when enabled.
IRQDONE
         PMC   ACK_IRQ;IRQ_RASTER
         PLA
         TAX
         PLA
         RTI

; ---------------------------------------------------
; WAVE LOOKUP
; ---------------------------------------------------
;
; X is the next line. PHASE moves the 128-entry curve
; each frame. Adjacent entries differ by at most two
; pixels, avoiding the old coarse steps.
SETWAVE  TXA
         CLC
         ADC   PHASE
         AND   #127
         TAX
         LDA   WAVETAB,X
         STA   SCROLL_X_LOW
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

; ---------------------------------------------------
; INTERRUPT VECTORS
; ---------------------------------------------------
         ORG   $FFFA
         DA    NMI,RESET,IRQ

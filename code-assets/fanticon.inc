; ---------------------------------------------------
; FANTICON 1.0 HARDWARE DEFINITIONS
; ---------------------------------------------------
;
; Globally available with: INCLUDE FANTICON.INC
; Constants match the cartridge-visible hardware.
; Helper macros document their register clobbers.

; ---------------------------------------------------
; MACHINE, MEMORY, AND TIMING
; ---------------------------------------------------
FANTICON_MAJOR EQU 1
FANTICON_MINOR EQU 0
CPU_CLOCK_KHZ  EQU 3144
FRAME_HZ       EQU 60
CYCLES_FRAME   EQU 52400
CYCLES_LINE    EQU 200
SCREEN_WIDTH   EQU 320
SCREEN_HEIGHT  EQU 200
RASTER_WIDTH   EQU 400
RASTER_HEIGHT  EQU 262

ZERO_PAGE      EQU $0000
ZERO_PAGE_END  EQU $00FF
STACK_PAGE     EQU $0100
STACK_PAGE_END EQU $01FF
MAIN_RAM       EQU $0000
USER_RAM       EQU $0200
MAIN_RAM_END   EQU $7FFF
BANK_WINDOW    EQU $8000
BANK_WINDOW_END EQU $BFFF
IO_PAGE        EQU $C000
IO_PAGE_END    EQU $C0FF
FIXED_ROM      EQU $C100
FIXED_ROM_END  EQU $FFFF
VECTOR_NMI     EQU $FFFA
VECTOR_RESET   EQU $FFFC
VECTOR_IRQ     EQU $FFFE
MAIN_RAM_SIZE  EQU $8000
BANK_SIZE      EQU $4000
IO_PAGE_SIZE   EQU $0100
FIXED_ROM_SIZE EQU $3F00

; ---------------------------------------------------
; BANKING AND INTERRUPTS
; ---------------------------------------------------
BANK_KIND      EQU $C000
BANK_NUMBER    EQU $C001
IRQ_PENDING    EQU $C002
IRQ_ENABLE     EQU $C003
FRAME_LOW      EQU $C004
FRAME_HIGH     EQU $C005
MACHINE_MAJOR  EQU $C006
MACHINE_MINOR  EQU $C007

BANK_CARTRIDGE EQU $00
BANK_WORK_RAM  EQU $01
BANK_VRAM      EQU $02
BANK_SAVE_RAM  EQU $03
ROM_BANKS_MAX  EQU 256
WORK_RAM_BANKS EQU 4
VRAM_BANKS     EQU 3
SAVE_BANKS_MAX EQU 4

IRQ_VBLANK     EQU $01
IRQ_RASTER     EQU $02
IRQ_TIMER0     EQU $04
IRQ_TIMER1     EQU $08

; ---------------------------------------------------
; VIDEO REGISTERS AND FLAGS
; ---------------------------------------------------
VIDEO_MODE     EQU $C010
VIDEO_CONTROL  EQU $C011
BACKDROP_COLOR EQU $C012
SCROLL_X_LOW   EQU $C013
SCROLL_X_HIGH  EQU $C014
SCROLL_Y_LOW   EQU $C015
SCROLL_Y_HIGH  EQU $C016
RASTER_X_LOW   EQU $C017
RASTER_X_HIGH  EQU $C018
RASTER_Y_LOW   EQU $C019
RASTER_Y_HIGH  EQU $C01A
PALETTE_INDEX  EQU $C01B
PALETTE_DATA   EQU $C01C
BITMAP_PALETTE EQU $C01D
VIDEO_STATUS   EQU $C01E

VIDEO_BLANK    EQU $00
VIDEO_TILEMAP  EQU $01
VIDEO_BITMAP   EQU $02
VIDEO_BG       EQU $01
VIDEO_SPRITES  EQU $02
VIDEO_ALL      EQU $03
STATUS_VBLANK  EQU $01
STATUS_HBLANK  EQU $02
STATUS_SPROVER EQU $04

RGB332_RED     EQU $E0
RGB332_GREEN   EQU $1C
RGB332_BLUE    EQU $03

; ---------------------------------------------------
; VRAM LAYOUT
; ---------------------------------------------------
VRAM_TILE_DATA EQU $0000
VRAM_TILE_MAP  EQU $2000
VRAM_TILE_ATTR EQU $2800
VRAM_SPRITES   EQU $3000
VRAM_BITMAP    EQU $4000

VRAM_TILE_CPU  EQU $8000
VRAM_MAP_CPU   EQU $A000
VRAM_ATTR_CPU  EQU $A800
VRAM_SPR_CPU   EQU $B000
TILE_BYTES     EQU 32
TILE_COUNT     EQU 256
MAP_WIDTH      EQU 64
MAP_HEIGHT     EQU 32
MAP_PIX_WIDTH  EQU 512
MAP_PIX_HEIGHT EQU 256
SPRITE_COUNT   EQU 32
SPRITES_LINE   EQU 8
SPRITE_BYTES   EQU 8
TILE_MAP_BYTES EQU $0800
SPR_TABLE_SIZE EQU $0100
BITMAP_BYTES   EQU $7D00
VRAM_SIZE      EQU $C000

TILE_PAL_MASK  EQU $0F
TILE_HFLIP     EQU $10
TILE_VFLIP     EQU $20
TILE_PRIORITY  EQU $40

SPR_X_LOW      EQU 0
SPR_X_FLAGS    EQU 1
SPR_Y          EQU 2
SPR_TILE       EQU 3
SPR_ATTR       EQU 4
SPR_X_HIGH     EQU $01
SPR_BEHIND_BG  EQU $02
SPR_PAL_MASK   EQU $0F
SPR_HFLIP      EQU $10
SPR_VFLIP      EQU $20
SPR_SIZE_16    EQU $40
SPR_ENABLE     EQU $80

; ---------------------------------------------------
; AUDIO REGISTERS AND FLAGS
; ---------------------------------------------------
PULSE1_CONTROL EQU $C030
PULSE1_LOW     EQU $C031
PULSE1_HIGH    EQU $C032
PULSE1_RESET   EQU $C033
PULSE2_CONTROL EQU $C034
PULSE2_LOW     EQU $C035
PULSE2_HIGH    EQU $C036
PULSE2_RESET   EQU $C037
TRI_CONTROL    EQU $C038
TRI_LOW        EQU $C039
TRI_HIGH       EQU $C03A
TRI_RESET      EQU $C03B
NOISE_CONTROL  EQU $C03C
NOISE_PERIOD   EQU $C03D
NOISE_RESET    EQU $C03E
AUDIO_MASTER   EQU $C040

AUDIO_ENABLE   EQU $80
AUDIO_VOL_MASK EQU $0F
AUDIO_HIGHMASK EQU $07
PULSE_DUTY_12_5 EQU $00
PULSE_DUTY_25  EQU $20
PULSE_DUTY_50  EQU $40
PULSE_DUTY_75  EQU $60
NOISE_SHORT    EQU $40
NOISE_PER_MASK EQU $0F

; ---------------------------------------------------
; CONTROLLERS
; ---------------------------------------------------
PAD0_STATE     EQU $C050
PAD0_PRESSED   EQU $C051
PAD1_STATE     EQU $C052
PAD1_PRESSED   EQU $C053

PAD_UP         EQU $01
PAD_DOWN       EQU $02
PAD_LEFT       EQU $04
PAD_RIGHT      EQU $08
PAD_A          EQU $10
PAD_B          EQU $20
PAD_SELECT     EQU $40
PAD_START      EQU $80

; ---------------------------------------------------
; INTERVAL TIMERS
; ---------------------------------------------------
TIMER0_RELOADL EQU $C060
TIMER0_RELOADH EQU $C061
TIMER0_CONTROL EQU $C062
TIMER0_COUNTL  EQU $C063
TIMER0_COUNTH  EQU $C064
TIMER1_RELOADL EQU $C068
TIMER1_RELOADH EQU $C069
TIMER1_CONTROL EQU $C06A
TIMER1_COUNTL  EQU $C06B
TIMER1_COUNTH  EQU $C06C

TIMER_ENABLE   EQU $01
TIMER_REPEAT   EQU $02

; ---------------------------------------------------
; HELPER MACROS
; ---------------------------------------------------
; Select a mapped bank. Clobbers A.
SET_BANK MAC KIND;NUMBER
         LDA #]KIND
         STA BANK_KIND
         LDA #]NUMBER
         STA BANK_NUMBER
         EOM

; Acknowledge selected IRQ sources. Clobbers A.
ACK_IRQ MAC MASK
         LDA #]MASK
         STA IRQ_PENDING
         EOM

; Replace the enabled IRQ mask. Clobbers A.
SET_IRQS MAC MASK
         LDA #]MASK
         STA IRQ_ENABLE
         EOM

; Select video mode and layers. Clobbers A.
SET_VIDEO MAC MODE;LAYERS=VIDEO_BG
         LDA #]MODE
         STA VIDEO_MODE
         LDA #]LAYERS
         STA VIDEO_CONTROL
         EOM

; Select bitmap mode and its palette. Clobbers A.
SET_BITMAP MAC PALETTE;LAYERS=VIDEO_BG
         LDA #]PALETTE
         STA BITMAP_PALETTE
         LDA #VIDEO_BITMAP
         STA VIDEO_MODE
         LDA #]LAYERS
         STA VIDEO_CONTROL
         EOM

; Set the backdrop palette index. Clobbers A.
SET_BACKDROP MAC COLOR
         LDA #]COLOR
         STA BACKDROP_COLOR
         EOM

; Set immediate 16-bit scroll values. Clobbers A.
SET_SCROLL MAC X;Y
         LDA #<]X
         STA SCROLL_X_LOW
         LDA #>]X
         STA SCROLL_X_HIGH
         LDA #<]Y
         STA SCROLL_Y_LOW
         LDA #>]Y
         STA SCROLL_Y_HIGH
         EOM

; Set an immediate raster target. Clobbers A.
SET_RASTER MAC X;Y
         LDA #<]X
         STA RASTER_X_LOW
         LDA #>]X
         STA RASTER_X_HIGH
         LDA #<]Y
         STA RASTER_Y_LOW
         LDA #>]Y
         STA RASTER_Y_HIGH
         EOM

; Write one RGB332 palette entry. Clobbers A.
SET_COLOR MAC INDEX;COLOR
         LDA #]INDEX
         STA PALETTE_INDEX
         LDA #]COLOR
         STA PALETTE_DATA
         EOM

; Upload one tile to mapped VRAM bank 0. Clobbers A/X.
UPLOAD_TILE MAC INDEX;SOURCE
         LDX #0
@COPY    LDA ]SOURCE,X
         STA VRAM_TILE_CPU+]INDEX*TILE_BYTES,X
         INX
         CPX #TILE_BYTES
         BNE @COPY
         EOM

; Fill mapped tile and attribute maps. Clobbers A/X.
FILL_TILEMAP MAC TILE;ATTR
         LDX #0
@FILL    LDA #]TILE
         REPEAT 8;PAGE
         STA VRAM_MAP_CPU+]PAGE*$100,X
         ENDREP
         LDA #]ATTR
         REPEAT 8;PAGE
         STA VRAM_ATTR_CPU+]PAGE*$100,X
         ENDREP
         INX
         BNE @FILL
         EOM

; Initialize one mapped sprite record. Clobbers A.
SET_SPRITE MAC INDEX;X;Y;TILE;ATTR;FLAGS=0
@BASE    EQU VRAM_SPR_CPU+]INDEX*SPRITE_BYTES
         LDA #<]X
         STA @BASE+SPR_X_LOW
         LDA #>]X|]FLAGS
         STA @BASE+SPR_X_FLAGS
         LDA #]Y
         STA @BASE+SPR_Y
         LDA #]TILE
         STA @BASE+SPR_TILE
         LDA #]ATTR
         STA @BASE+SPR_ATTR
         EOM

; Configure a pulse or triangle block. Clobbers A.
SET_TONE MAC BASE;CONTROL;TIMER
         LDA #]CONTROL
         STA ]BASE
         LDA #<]TIMER
         STA ]BASE+1
         LDA #>]TIMER
         STA ]BASE+2
         STA ]BASE+3
         EOM

; Configure and reset the noise voice. Clobbers A.
SET_NOISE MAC CONTROL;PERIOD
         LDA #]CONTROL
         STA NOISE_CONTROL
         LDA #]PERIOD
         STA NOISE_PERIOD
         STA NOISE_RESET
         EOM

; Set master enable and volume. Clobbers A.
SET_AUDIO_MASTER MAC VOLUME;ENABLE=AUDIO_ENABLE
         LDA #]ENABLE|]VOLUME
         STA AUDIO_MASTER
         EOM

; Disable all audio channels and master. Clobbers A.
SILENCE_AUDIO MAC
         LDA #0
         STA PULSE1_CONTROL
         STA PULSE2_CONTROL
         STA TRI_CONTROL
         STA NOISE_CONTROL
         STA AUDIO_MASTER
         EOM

; Start timer BASE with a 16-bit reload. Clobbers A.
START_TIMER MAC BASE;RELOAD;CONTROL=TIMER_ENABLE
         LDA #<]RELOAD
         STA ]BASE
         LDA #>]RELOAD
         STA ]BASE+1
         LDA #]CONTROL
         STA ]BASE+2
         EOM

; Stop a timer without changing its reload. Clobbers A.
STOP_TIMER MAC BASE
         LDA #0
         STA ]BASE+2
         EOM

; Read frame or timer words in required latch order.
; Both macros clobber A.
READ_FRAME16 MAC DEST
         LDA FRAME_LOW
         STA ]DEST
         LDA FRAME_HIGH
         STA ]DEST+1
         EOM

READ_TIMER16 MAC BASE;DEST
         LDA ]BASE+3
         STA ]DEST
         LDA ]BASE+4
         STA ]DEST+1
         EOM

; Wait for the live VBlank interval. Clobbers A.
WAIT_VBLANK MAC
@WAIT    LDA VIDEO_STATUS
         AND #STATUS_VBLANK
         BEQ @WAIT
         EOM

; Wait for a new VBlank edge. Clobbers A.
WAIT_NEXT_VBLANK MAC
@LEAVE   LDA VIDEO_STATUS
         AND #STATUS_VBLANK
         BNE @LEAVE
@ENTER   LDA VIDEO_STATUS
         AND #STATUS_VBLANK
         BEQ @ENTER
         EOM

; Save bank selection and A on the CPU stack.
; Must run outside the bank window.
PUSH_BANK MAC
         PHA
         LDA BANK_KIND
         PHA
         LDA BANK_NUMBER
         PHA
         EOM

; Restore bank selection and A from the CPU stack.
; Must run outside the bank window.
POP_BANK MAC
         PLA
         STA BANK_NUMBER
         PLA
         STA BANK_KIND
         PLA
         EOM

; Save and restore an IRQ handler's common registers.
; PUSH_AXY clobbers A. POP_YXA restores A, X, and Y.
PUSH_AXY MAC
         PHA
         TXA
         PHA
         TYA
         PHA
         EOM

POP_YXA MAC
         PLA
         TAY
         PLA
         TAX
         PLA
         EOM

; Store a 16-bit immediate value. Clobbers A.
STORE16 MAC DEST;VALUE
         LDA #<]VALUE
         STA ]DEST
         LDA #>]VALUE
         STA ]DEST+1
         EOM

; Copy a little-endian word. Clobbers A.
COPY16 MAC DEST;SOURCE
         LDA ]SOURCE
         STA ]DEST
         LDA ]SOURCE+1
         STA ]DEST+1
         EOM

; Add or subtract an immediate word. Clobbers A.
ADD16 MAC ADDRESS;VALUE
         CLC
         LDA ]ADDRESS
         ADC #<]VALUE
         STA ]ADDRESS
         LDA ]ADDRESS+1
         ADC #>]VALUE
         STA ]ADDRESS+1
         EOM

SUB16 MAC ADDRESS;VALUE
         SEC
         LDA ]ADDRESS
         SBC #<]VALUE
         STA ]ADDRESS
         LDA ]ADDRESS+1
         SBC #>]VALUE
         STA ]ADDRESS+1
         EOM

; Increment or decrement a little-endian word.
INC16 MAC ADDRESS
         INC ]ADDRESS
         BNE @DONE
         INC ]ADDRESS+1
@DONE
         EOM

DEC16 MAC ADDRESS
         LDA ]ADDRESS
         BNE @LOW
         DEC ]ADDRESS+1
@LOW     DEC ]ADDRESS
         EOM

; Fanticon has no hardware entropy source; the noise
; voice's LFSR is audio-only and not meant for gameplay
; use. These two macros run an 8-bit Galois LFSR (tap
; $1D, maximal period 255) over a caller-owned seed byte,
; so results are fully deterministic for a given input
; sequence: seed once from FRAME_LOW and PAD0_STATE so
; different players, or different input timing from the
; same player, diverge, while a fixed input recording
; always reproduces the same sequence.

; Seed SEED from the live frame count and pad state, and
; guarantee it is nonzero (an all-zero seed never
; changes). Clobbers A.
SEED_RANDOM MAC SEED
         LDA FRAME_LOW
         EOR PAD0_STATE
         BNE @NONZERO
         LDA #1
@NONZERO STA ]SEED
         EOM

; Advance SEED one LFSR step and leave the new byte in A.
; Clobbers A.
NEXT_RANDOM MAC SEED
         LDA ]SEED
         ASL A
         BCC @NOEOR
         EOR #$1D
@NOEOR   STA ]SEED
         EOM

; ---------------------------------------------------
; OPT-IN RUNTIME PROCEDURE EMITTERS
; ---------------------------------------------------
; Standard emitters require the FIXED section.
; Emit NAME to copy ROM bank 0 into mapped VRAM bank 0.
; SRC, DST, and LEN are zero-page words. BUF is a page.
; The procedure clobbers A/Y and leaves VRAM selected.
EMIT_VRAM_COPY MAC NAME;SRC;DST;LEN;BUF
         REQUIRE_FIXED
]NAME    PROC
         LDA ]LEN+1
         BEQ .TAIL
.PAGE    LDA #BANK_CARTRIDGE
         STA BANK_KIND
         STA BANK_NUMBER
         LDY #0
.READ    LDA (]SRC),Y
         STA ]BUF,Y
         INY
         BNE .READ
         LDA #BANK_VRAM
         STA BANK_KIND
         LDA #0
         STA BANK_NUMBER
         LDY #0
.WRITE   LDA ]BUF,Y
         STA (]DST),Y
         INY
         BNE .WRITE
         INC ]SRC+1
         INC ]DST+1
         DEC ]LEN+1
         BNE .PAGE
.TAIL    LDA ]LEN
         BEQ .DONE
         LDA #BANK_CARTRIDGE
         STA BANK_KIND
         STA BANK_NUMBER
         LDY #0
.READTL  LDA (]SRC),Y
         STA ]BUF,Y
         INY
         CPY ]LEN
         BNE .READTL
         LDA #BANK_VRAM
         STA BANK_KIND
         LDA #0
         STA BANK_NUMBER
         LDY #0
.WRITETL LDA ]BUF,Y
         STA (]DST),Y
         INY
         CPY ]LEN
         BNE .WRITETL
.DONE    RTS
         ENDPROC
         EOM

; Emit NAME to update signed scroll words from a pad.
; The procedure clobbers A and preserves X/Y.
EMIT_PAD_SCROLL MAC NAME;X;Y;PAD=PAD0_STATE
         REQUIRE_FIXED
]NAME    PROC
         LDA ]PAD
         AND #PAD_LEFT
         BEQ .NOLEFT
         LDA ]X
         BNE .LEFTLO
         DEC ]X+1
.LEFTLO  DEC ]X
.NOLEFT  LDA ]PAD
         AND #PAD_RIGHT
         BEQ .NORIGHT
         INC ]X
         BNE .NORIGHT
         INC ]X+1
.NORIGHT LDA ]PAD
         AND #PAD_UP
         BEQ .NOUP
         LDA ]Y
         BNE .UPLO
         DEC ]Y+1
.UPLO    DEC ]Y
.NOUP    LDA ]PAD
         AND #PAD_DOWN
         BEQ .DONE
         INC ]Y
         BNE .DONE
         INC ]Y+1
.DONE    RTS
         ENDPROC
         EOM

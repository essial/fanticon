; ---------------------------------------------------
; FANTICON FOUR-VOICE MUSIC PLAYER
; ---------------------------------------------------
;
; MUSIC_START takes the song address in X/Y.
; MUSIC_TICK must be called once per video frame.
; The player reserves zero-page bytes $30-$41.

         INCLUDE FANTICON.INC

MPPTR    EQU   $30
MPBASE   EQU   $32
MPROW    EQU   $34
MPWAIT   EQU   $35
MPTEMPO  EQU   $36
MPLEN    EQU   $37
MPTEMP   EQU   $38
MPVER    EQU   $39
MPFRAME  EQU   $3A
MPFLEN   EQU   $3C
MPLOOP   EQU   $3E
MPLOOPF  EQU   $40
MUSIC_START
         STX   MPPTR
         STY   MPPTR+1
         LDY   #0
         LDA   (MPPTR),Y
         CMP   #$F2
         BEQ   MPSTART2
         LDA   #1
         STA   MPVER
         LDA   (MPPTR),Y
         STA   MPTEMPO
         INY
         LDA   (MPPTR),Y
         STA   MPLEN
         CLC
         LDA   MPPTR
         ADC   #2
         STA   MPPTR
         STA   MPBASE
         LDA   MPPTR+1
         ADC   #0
         STA   MPPTR+1
         STA   MPBASE+1
         LDA   #0
         STA   MPROW
         LDA   #1
         STA   MPWAIT
         PMC   SET_AUDIO_MASTER;15
         RTS

; Version 2 points at one compiled register frame for
; every VBlank. This makes envelopes sound identical
; in the editor preview and on cartridge hardware.
MPSTART2 LDA   #2
         STA   MPVER
         INY
         LDA   (MPPTR),Y
         STA   MPFLEN
         INY
         LDA   (MPPTR),Y
         STA   MPFLEN+1
         INY
         LDA   (MPPTR),Y
         STA   MPLOOPF
         INY
         LDA   (MPPTR),Y
         STA   MPLOOPF+1
         INY
         LDA   (MPPTR),Y
         STA   MPBASE
         INY
         LDA   (MPPTR),Y
         STA   MPBASE+1
         INY
         LDA   (MPPTR),Y
         STA   MPLOOP
         INY
         LDA   (MPPTR),Y
         STA   MPLOOP+1
         LDA   MPBASE
         STA   MPPTR
         LDA   MPBASE+1
         STA   MPPTR+1
         LDA   #0
         STA   MPFRAME
         STA   MPFRAME+1
         PMC   SET_AUDIO_MASTER;15
         RTS

MUSIC_STOP
         LDA   #0
         STA   PULSE1_CONTROL
         STA   PULSE2_CONTROL
         STA   TRI_CONTROL
         STA   NOISE_CONTROL
         RTS

MUSIC_TICK
         LDA   MPVER
         CMP   #2
         BNE   MPTICK1
         JMP   MPPLAY2
MPTICK1  DEC   MPWAIT
         BEQ   MPPLAY
         RTS
MPPLAY   LDA   MPTEMPO
         STA   MPWAIT
         LDY   #0

; Pulse 1 uses 50-percent duty.
         LDA   (MPPTR),Y
         INY
         CMP   #$FF
         BEQ   MPP2
         TAX
         LDA   (MPPTR),Y
         STA   MPTEMP
         TXA
         BEQ   MPP1OFF
         TAX
         LDA   MPTEMP
         ORA   #$C0
         STA   PULSE1_CONTROL
         LDA   MP_PLO,X
         STA   PULSE1_LOW
         LDA   MP_PHI,X
         STA   PULSE1_HIGH
         STA   PULSE1_RESET
         JMP   MPP2
MPP1OFF  LDA   #0
         STA   PULSE1_CONTROL

; Pulse 2 uses 25-percent duty.
MPP2     INY
         LDA   (MPPTR),Y
         INY
         CMP   #$FF
         BEQ   MPTRI
         TAX
         LDA   (MPPTR),Y
         STA   MPTEMP
         TXA
         BEQ   MPP2OFF
         TAX
         LDA   MPTEMP
         ORA   #$A0
         STA   PULSE2_CONTROL
         LDA   MP_PLO,X
         STA   PULSE2_LOW
         LDA   MP_PHI,X
         STA   PULSE2_HIGH
         STA   PULSE2_RESET
         JMP   MPTRI
MPP2OFF  LDA   #0
         STA   PULSE2_CONTROL

; Triangle treats volume zero as disabled.
MPTRI    INY
         LDA   (MPPTR),Y
         INY
         CMP   #$FF
         BEQ   MPNOISE
         TAX
         LDA   (MPPTR),Y
         BEQ   MPTRIOFF
         TXA
         BEQ   MPTRIOFF
         TAX
         LDA   #$80
         STA   TRI_CONTROL
         LDA   MP_TLO,X
         STA   TRI_LOW
         LDA   MP_THI,X
         STA   TRI_HIGH
         STA   TRI_RESET
         JMP   MPNOISE
MPTRIOFF LDA   #0
         STA   TRI_CONTROL

; Noise notes N00-N0F select period entries.
MPNOISE  INY
         LDA   (MPPTR),Y
         INY
         CMP   #$FF
         BEQ   MPADV
         TAX
         LDA   (MPPTR),Y
         STA   MPTEMP
         TXA
         BEQ   MPNOFF
         SEC
         SBC   #1
         STA   NOISE_PERIOD
         LDA   MPTEMP
         ORA   #$80
         STA   NOISE_CONTROL
         STA   NOISE_RESET
         JMP   MPADV
MPNOFF   LDA   #0
         STA   NOISE_CONTROL

; Move to the next eight-byte tracker row.
MPADV    CLC
         LDA   MPPTR
         ADC   #8
         STA   MPPTR
         LDA   MPPTR+1
         ADC   #0
         STA   MPPTR+1
         INC   MPROW
         LDA   MPROW
         CMP   MPLEN
         BNE   MPDONE
         LDA   #0
         STA   MPROW
         LDA   MPBASE
         STA   MPPTR
         LDA   MPBASE+1
         STA   MPPTR+1
MPDONE   RTS

; Decode one delta-compressed audio register frame.
MPPLAY2  LDY   #0
         LDA   (MPPTR),Y
         STA   MPTEMP
         JSR   MPADD1
         LSR   MPTEMP
         BCC   MPV2P2
         LDY   #0
         LDA   (MPPTR),Y
         STA   PULSE1_CONTROL
         INY
         LDA   (MPPTR),Y
         STA   PULSE1_LOW
         INY
         LDA   (MPPTR),Y
         STA   PULSE1_HIGH
         INY
         LDA   (MPPTR),Y
         BEQ   MPNR1
         STA   PULSE1_RESET
MPNR1    JSR   MPADD4
MPV2P2   LSR   MPTEMP
         BCC   MPV2TRI
         LDY   #0
         LDA   (MPPTR),Y
         STA   PULSE2_CONTROL
         INY
         LDA   (MPPTR),Y
         STA   PULSE2_LOW
         INY
         LDA   (MPPTR),Y
         STA   PULSE2_HIGH
         INY
         LDA   (MPPTR),Y
         BEQ   MPNR2
         STA   PULSE2_RESET
MPNR2    JSR   MPADD4
MPV2TRI  LSR   MPTEMP
         BCC   MPV2NOI
         LDY   #0
         LDA   (MPPTR),Y
         STA   TRI_CONTROL
         INY
         LDA   (MPPTR),Y
         STA   TRI_LOW
         INY
         LDA   (MPPTR),Y
         STA   TRI_HIGH
         INY
         LDA   (MPPTR),Y
         BEQ   MPNRT
         STA   TRI_RESET
MPNRT    JSR   MPADD4
MPV2NOI  LSR   MPTEMP
         BCC   MPV2ADV
         LDY   #0
         LDA   (MPPTR),Y
         STA   NOISE_CONTROL
         INY
         LDA   (MPPTR),Y
         STA   NOISE_PERIOD
         INY
         INY
         LDA   (MPPTR),Y
         BEQ   MPNRN
         STA   NOISE_RESET
MPNRN    JSR   MPADD4
MPV2ADV  INC   MPFRAME
         BNE   MPCMP2
         INC   MPFRAME+1
MPCMP2   LDA   MPFRAME+1
         CMP   MPFLEN+1
         BNE   MPDONE2
         LDA   MPFRAME
         CMP   MPFLEN
         BNE   MPDONE2
         LDA   MPLOOPF
         STA   MPFRAME
         LDA   MPLOOPF+1
         STA   MPFRAME+1
         LDA   MPLOOP
         STA   MPPTR
         LDA   MPLOOP+1
         STA   MPPTR+1
MPDONE2  RTS

MPADD1   INC   MPPTR
         BNE   MPADONE
         INC   MPPTR+1
MPADONE  RTS

MPADD4   CLC
         LDA   MPPTR
         ADC   #4
         STA   MPPTR
         LDA   MPPTR+1
         ADC   #0
         STA   MPPTR+1
         RTS

; Pulse timer table, indexed by C-0 through B-7.
MP_PLO
         DFB $FF,$FF,$FF,$FF,$FF,$FF,$FF,$FF,$FF,$FF,$FF,$FF,$FF,$FF,$FF,$FF
         DFB $FF,$FF,$FF,$FF,$FF,$FF,$FF,$FF,$FF,$FF,$FF,$FF,$FF,$FF,$FF,$D4
         DFB $64,$F9,$95,$36,$DD,$89,$39,$EE,$A7,$64,$25,$EA,$B1,$7C,$4A,$1B
         DFB $EE,$C4,$9C,$77,$53,$32,$12,$F4,$D8,$BE,$A5,$8D,$77,$61,$4E,$3B
         DFB $29,$18,$09,$FA,$EC,$DE,$D2,$C6,$BB,$B0,$A6,$9D,$94,$8C,$84,$7C
         DFB $75,$6F,$68,$62,$5D,$58,$53,$4E,$4A,$45,$41,$3E,$3A,$37,$34,$31
MP_PHI
         DFB $07,$07,$07,$07,$07,$07,$07,$07,$07,$07,$07,$07,$07,$07,$07,$07
         DFB $07,$07,$07,$07,$07,$07,$07,$07,$07,$07,$07,$07,$07,$07,$07,$07
         DFB $07,$06,$06,$06,$05,$05,$05,$04,$04,$04,$04,$03,$03,$03,$03,$03
         DFB $02,$02,$02,$02,$02,$02,$02,$01,$01,$01,$01,$01,$01,$01,$01,$01
         DFB $01,$01,$01,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00
         DFB $00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00

; Triangle timer table uses the same note indexes.
MP_TLO
         DFB $FF,$FF,$FF,$FF,$FF,$FF,$FF,$FF,$FF,$FF,$FF,$FF,$FF,$FF,$FF,$FF
         DFB $FF,$FF,$FF,$D4,$64,$F9,$95,$36,$DD,$89,$39,$EE,$A7,$64,$25,$EA
         DFB $B1,$7C,$4A,$1B,$EE,$C4,$9C,$77,$53,$32,$12,$F4,$D8,$BE,$A5,$8D
         DFB $77,$61,$4E,$3B,$29,$18,$09,$FA,$EC,$DE,$D2,$C6,$BB,$B0,$A6,$9D
         DFB $94,$8C,$84,$7C,$75,$6F,$68,$62,$5D,$58,$53,$4E,$4A,$45,$41,$3E
         DFB $3A,$37,$34,$31,$2E,$2B,$29,$26,$24,$22,$20,$1E,$1D,$1B,$19,$18
MP_THI
         DFB $07,$07,$07,$07,$07,$07,$07,$07,$07,$07,$07,$07,$07,$07,$07,$07
         DFB $07,$07,$07,$07,$07,$06,$06,$06,$05,$05,$05,$04,$04,$04,$04,$03
         DFB $03,$03,$03,$03,$02,$02,$02,$02,$02,$02,$02,$01,$01,$01,$01,$01
         DFB $01,$01,$01,$01,$01,$01,$01,$00,$00,$00,$00,$00,$00,$00,$00,$00
         DFB $00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00
         DFB $00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00

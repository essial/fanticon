use core::fmt;

/// One call is one physical 6502 bus cycle. Implementations can clock mapped
/// devices from these methods, which keeps the CPU cycle-exact without a slow
/// per-cycle heap allocation or callback layer.
pub trait Bus {
    fn read(&mut self, address: u16) -> u8;
    fn write(&mut self, address: u16, value: u8);

    /// Input pins sampled at the end of each physical bus cycle.
    fn pins(&self) -> Pins {
        Pins::default()
    }
}

fn decode(opcode: u8) -> Action {
    use Action::*;
    use ImpliedOp::*;
    use Mode::*;
    use ReadOp::*;
    use RmwOp::*;
    use WriteOp::*;
    match opcode {
        0x00 => Brk,
        0x20 => Jsr,
        0x40 => Rti,
        0x60 => Rts,
        0x4c => JmpAbs,
        0x6c => JmpInd,
        0x08 => Php,
        0x28 => Plp,
        0x48 => Pha,
        0x68 => Pla,
        0x02 | 0x12 | 0x22 | 0x32 | 0x42 | 0x52 | 0x62 | 0x72 | 0x92 | 0xb2 | 0xd2 | 0xf2 => Kil,

        0x01 => Read(Ora, IndX),
        0x05 => Read(Ora, Zp),
        0x09 => Read(Ora, Imm),
        0x0d => Read(Ora, Abs),
        0x11 => Read(Ora, IndY),
        0x15 => Read(Ora, ZpX),
        0x19 => Read(Ora, AbsY),
        0x1d => Read(Ora, AbsX),
        0x21 => Read(And, IndX),
        0x25 => Read(And, Zp),
        0x29 => Read(And, Imm),
        0x2d => Read(And, Abs),
        0x31 => Read(And, IndY),
        0x35 => Read(And, ZpX),
        0x39 => Read(And, AbsY),
        0x3d => Read(And, AbsX),
        0x41 => Read(Eor, IndX),
        0x45 => Read(Eor, Zp),
        0x49 => Read(Eor, Imm),
        0x4d => Read(Eor, Abs),
        0x51 => Read(Eor, IndY),
        0x55 => Read(Eor, ZpX),
        0x59 => Read(Eor, AbsY),
        0x5d => Read(Eor, AbsX),
        0x61 => Read(Adc, IndX),
        0x65 => Read(Adc, Zp),
        0x69 => Read(Adc, Imm),
        0x6d => Read(Adc, Abs),
        0x71 => Read(Adc, IndY),
        0x75 => Read(Adc, ZpX),
        0x79 => Read(Adc, AbsY),
        0x7d => Read(Adc, AbsX),
        0xa1 => Read(Lda, IndX),
        0xa5 => Read(Lda, Zp),
        0xa9 => Read(Lda, Imm),
        0xad => Read(Lda, Abs),
        0xb1 => Read(Lda, IndY),
        0xb5 => Read(Lda, ZpX),
        0xb9 => Read(Lda, AbsY),
        0xbd => Read(Lda, AbsX),
        0xc1 => Read(Cmp, IndX),
        0xc5 => Read(Cmp, Zp),
        0xc9 => Read(Cmp, Imm),
        0xcd => Read(Cmp, Abs),
        0xd1 => Read(Cmp, IndY),
        0xd5 => Read(Cmp, ZpX),
        0xd9 => Read(Cmp, AbsY),
        0xdd => Read(Cmp, AbsX),
        0xe1 => Read(Sbc, IndX),
        0xe5 => Read(Sbc, Zp),
        0xe9 | 0xeb => Read(Sbc, Imm),
        0xed => Read(Sbc, Abs),
        0xf1 => Read(Sbc, IndY),
        0xf5 => Read(Sbc, ZpX),
        0xf9 => Read(Sbc, AbsY),
        0xfd => Read(Sbc, AbsX),
        0xa0 => Read(Ldy, Imm),
        0xa4 => Read(Ldy, Zp),
        0xac => Read(Ldy, Abs),
        0xb4 => Read(Ldy, ZpX),
        0xbc => Read(Ldy, AbsX),
        0xa2 => Read(Ldx, Imm),
        0xa6 => Read(Ldx, Zp),
        0xae => Read(Ldx, Abs),
        0xb6 => Read(Ldx, ZpY),
        0xbe => Read(Ldx, AbsY),
        0xc0 => Read(Cpy, Imm),
        0xc4 => Read(Cpy, Zp),
        0xcc => Read(Cpy, Abs),
        0xe0 => Read(Cpx, Imm),
        0xe4 => Read(Cpx, Zp),
        0xec => Read(Cpx, Abs),
        0x24 => Read(Bit, Zp),
        0x2c => Read(Bit, Abs),

        0x81 => Write(A, IndX),
        0x85 => Write(A, Zp),
        0x8d => Write(A, Abs),
        0x91 => Write(A, IndY),
        0x95 => Write(A, ZpX),
        0x99 => Write(A, AbsY),
        0x9d => Write(A, AbsX),
        0x84 => Write(Y, Zp),
        0x8c => Write(Y, Abs),
        0x94 => Write(Y, ZpX),
        0x86 => Write(X, Zp),
        0x8e => Write(X, Abs),
        0x96 => Write(X, ZpY),
        0x83 => Write(Sax, IndX),
        0x87 => Write(Sax, Zp),
        0x8f => Write(Sax, Abs),
        0x97 => Write(Sax, ZpY),
        0x93 => Write(Ahx, IndY),
        0x9f => Write(Ahx, AbsY),
        0x9b => Write(Tas, AbsY),
        0x9c => Write(Shy, AbsX),
        0x9e => Write(Shx, AbsY),

        0x06 => Rmw(Asl, Zp),
        0x0e => Rmw(Asl, Abs),
        0x16 => Rmw(Asl, ZpX),
        0x1e => Rmw(Asl, AbsX),
        0x0a => Accumulator(Asl),
        0x26 => Rmw(Rol, Zp),
        0x2e => Rmw(Rol, Abs),
        0x36 => Rmw(Rol, ZpX),
        0x3e => Rmw(Rol, AbsX),
        0x2a => Accumulator(Rol),
        0x46 => Rmw(Lsr, Zp),
        0x4e => Rmw(Lsr, Abs),
        0x56 => Rmw(Lsr, ZpX),
        0x5e => Rmw(Lsr, AbsX),
        0x4a => Accumulator(Lsr),
        0x66 => Rmw(Ror, Zp),
        0x6e => Rmw(Ror, Abs),
        0x76 => Rmw(Ror, ZpX),
        0x7e => Rmw(Ror, AbsX),
        0x6a => Accumulator(Ror),
        0xc6 => Rmw(Dec, Zp),
        0xce => Rmw(Dec, Abs),
        0xd6 => Rmw(Dec, ZpX),
        0xde => Rmw(Dec, AbsX),
        0xe6 => Rmw(Inc, Zp),
        0xee => Rmw(Inc, Abs),
        0xf6 => Rmw(Inc, ZpX),
        0xfe => Rmw(Inc, AbsX),
        0x03 => Rmw(Slo, IndX),
        0x07 => Rmw(Slo, Zp),
        0x0f => Rmw(Slo, Abs),
        0x13 => Rmw(Slo, IndY),
        0x17 => Rmw(Slo, ZpX),
        0x1b => Rmw(Slo, AbsY),
        0x1f => Rmw(Slo, AbsX),
        0x23 => Rmw(Rla, IndX),
        0x27 => Rmw(Rla, Zp),
        0x2f => Rmw(Rla, Abs),
        0x33 => Rmw(Rla, IndY),
        0x37 => Rmw(Rla, ZpX),
        0x3b => Rmw(Rla, AbsY),
        0x3f => Rmw(Rla, AbsX),
        0x43 => Rmw(Sre, IndX),
        0x47 => Rmw(Sre, Zp),
        0x4f => Rmw(Sre, Abs),
        0x53 => Rmw(Sre, IndY),
        0x57 => Rmw(Sre, ZpX),
        0x5b => Rmw(Sre, AbsY),
        0x5f => Rmw(Sre, AbsX),
        0x63 => Rmw(Rra, IndX),
        0x67 => Rmw(Rra, Zp),
        0x6f => Rmw(Rra, Abs),
        0x73 => Rmw(Rra, IndY),
        0x77 => Rmw(Rra, ZpX),
        0x7b => Rmw(Rra, AbsY),
        0x7f => Rmw(Rra, AbsX),
        0xc3 => Rmw(Dcp, IndX),
        0xc7 => Rmw(Dcp, Zp),
        0xcf => Rmw(Dcp, Abs),
        0xd3 => Rmw(Dcp, IndY),
        0xd7 => Rmw(Dcp, ZpX),
        0xdb => Rmw(Dcp, AbsY),
        0xdf => Rmw(Dcp, AbsX),
        0xe3 => Rmw(Isc, IndX),
        0xe7 => Rmw(Isc, Zp),
        0xef => Rmw(Isc, Abs),
        0xf3 => Rmw(Isc, IndY),
        0xf7 => Rmw(Isc, ZpX),
        0xfb => Rmw(Isc, AbsY),
        0xff => Rmw(Isc, AbsX),

        0xa3 => Read(Lax, IndX),
        0xa7 => Read(Lax, Zp),
        0xaf => Read(Lax, Abs),
        0xb3 => Read(Lax, IndY),
        0xb7 => Read(Lax, ZpY),
        0xbf => Read(Lax, AbsY),
        0xab => Read(Lax, Imm), // Immediate LAX is adjusted in apply_read below.
        0x0b | 0x2b => Read(Anc, Imm),
        0x4b => Read(Alr, Imm),
        0x6b => Read(Arr, Imm),
        0x8b => Read(Xaa, Imm),
        0xcb => Read(Axs, Imm),
        0xbb => Read(Las, AbsY),

        0x10 => Branch(Status::NEGATIVE, false),
        0x30 => Branch(Status::NEGATIVE, true),
        0x50 => Branch(Status::OVERFLOW, false),
        0x70 => Branch(Status::OVERFLOW, true),
        0x90 => Branch(Status::CARRY, false),
        0xb0 => Branch(Status::CARRY, true),
        0xd0 => Branch(Status::ZERO, false),
        0xf0 => Branch(Status::ZERO, true),
        0x18 => Implied(Clc),
        0x38 => Implied(Sec),
        0x58 => Implied(Cli),
        0x78 => Implied(Sei),
        0xb8 => Implied(Clv),
        0xd8 => Implied(Cld),
        0xf8 => Implied(Sed),
        0x88 => Implied(Dey),
        0xc8 => Implied(Iny),
        0xca => Implied(Dex),
        0xe8 => Implied(Inx),
        0x8a => Implied(Txa),
        0x98 => Implied(Tya),
        0x9a => Implied(Txs),
        0xaa => Implied(Tax),
        0xa8 => Implied(Tay),
        0xba => Implied(Tsx),
        0xea | 0x1a | 0x3a | 0x5a | 0x7a | 0xda | 0xfa => Implied(ImpliedOp::Nop),
        0x80 | 0x82 | 0x89 | 0xc2 | 0xe2 => Action::Nop(Imm),
        0x04 | 0x44 | 0x64 => Action::Nop(Zp),
        0x14 | 0x34 | 0x54 | 0x74 | 0xd4 | 0xf4 => Action::Nop(ZpX),
        0x0c => Action::Nop(Abs),
        0x1c | 0x3c | 0x5c | 0x7c | 0xdc | 0xfc => Action::Nop(AbsX),
    }
}

/// Logical input-pin levels. `reset`, `irq`, `nmi`, and `set_overflow` are
/// `true` when their active-low physical pins are asserted. RDY is active high.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pins {
    pub reset: bool,
    pub irq: bool,
    pub nmi: bool,
    pub ready: bool,
    pub set_overflow: bool,
}

impl Default for Pins {
    fn default() -> Self {
        Self { reset: false, irq: false, nmi: false, ready: true, set_overflow: false }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockResult {
    /// True during an opcode fetch, matching the physical SYNC output.
    pub sync: bool,
    /// True on the last cycle of an instruction or interrupt sequence.
    pub instruction_complete: bool,
    /// True once a KIL opcode has permanently jammed the processor.
    pub jammed: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Status(pub u8);

impl Status {
    pub const CARRY: u8 = 0x01;
    pub const ZERO: u8 = 0x02;
    pub const INTERRUPT_DISABLE: u8 = 0x04;
    pub const DECIMAL: u8 = 0x08;
    pub const BREAK: u8 = 0x10;
    pub const UNUSED: u8 = 0x20;
    pub const OVERFLOW: u8 = 0x40;
    pub const NEGATIVE: u8 = 0x80;
}

impl fmt::Debug for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Status({:#04x})", self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cpu {
    pub pc: u16,
    pub sp: u8,
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub status: Status,
    pub cycles: u64,
    engine: Engine,
    opcode: u8,
    phase: u8,
    lo: u8,
    hi: u8,
    data: u8,
    address: u16,
    base: u16,
    nmi_pending: bool,
    irq_pending: bool,
    reset_pending: bool,
    previous_nmi: bool,
    previous_so: bool,
    last_irq: bool,
    i_before_cycle: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Engine {
    Fetch,
    Instruction,
    Interrupt(Interrupt),
    Reset,
    Jammed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Interrupt {
    Irq,
    Nmi,
}

#[derive(Clone, Copy)]
enum Mode {
    Imm,
    Zp,
    ZpX,
    ZpY,
    Abs,
    AbsX,
    AbsY,
    IndX,
    IndY,
}

#[derive(Clone, Copy)]
enum ReadOp {
    Ora,
    And,
    Eor,
    Adc,
    Lda,
    Cmp,
    Sbc,
    Ldx,
    Ldy,
    Cpx,
    Cpy,
    Bit,
    Lax,
    Anc,
    Alr,
    Arr,
    Xaa,
    Axs,
    Las,
}

#[derive(Clone, Copy)]
enum RmwOp {
    Asl,
    Rol,
    Lsr,
    Ror,
    Dec,
    Inc,
    Slo,
    Rla,
    Sre,
    Rra,
    Dcp,
    Isc,
}

#[derive(Clone, Copy)]
enum WriteOp {
    A,
    X,
    Y,
    Sax,
    Ahx,
    Tas,
    Shy,
    Shx,
}

#[derive(Clone, Copy)]
enum ImpliedOp {
    Nop,
    Clc,
    Sec,
    Cli,
    Sei,
    Clv,
    Cld,
    Sed,
    Dey,
    Iny,
    Dex,
    Inx,
    Txa,
    Tya,
    Txs,
    Tax,
    Tay,
    Tsx,
}

#[derive(Clone, Copy)]
enum Action {
    Read(ReadOp, Mode),
    Write(WriteOp, Mode),
    Rmw(RmwOp, Mode),
    Accumulator(RmwOp),
    Implied(ImpliedOp),
    Branch(u8, bool),
    Nop(Mode),
    Brk,
    Jsr,
    Rti,
    Rts,
    JmpAbs,
    JmpInd,
    Php,
    Plp,
    Pha,
    Pla,
    Kil,
}

impl Default for Cpu {
    fn default() -> Self {
        Self {
            pc: 0,
            sp: 0xfd,
            a: 0,
            x: 0,
            y: 0,
            status: Status(0x24),
            cycles: 0,
            engine: Engine::Fetch,
            opcode: 0,
            phase: 0,
            lo: 0,
            hi: 0,
            data: 0,
            address: 0,
            base: 0,
            nmi_pending: false,
            irq_pending: false,
            reset_pending: false,
            previous_nmi: false,
            previous_so: false,
            last_irq: false,
            i_before_cycle: true,
        }
    }
}

impl Cpu {
    #[inline(always)]
    fn read<B: Bus>(&mut self, bus: &mut B, address: u16) -> u8 {
        self.cycles = self.cycles.wrapping_add(1);
        bus.read(address)
    }

    #[inline(always)]
    fn write<B: Bus>(&mut self, bus: &mut B, address: u16, value: u8) {
        self.cycles = self.cycles.wrapping_add(1);
        bus.write(address, value);
    }

    #[inline(always)]
    fn fetch<B: Bus>(&mut self, bus: &mut B) -> u8 {
        let value = self.read(bus, self.pc);
        self.pc = self.pc.wrapping_add(1);
        value
    }

    #[inline(always)]
    fn flag(&self, mask: u8) -> bool {
        self.status.0 & mask != 0
    }

    #[inline(always)]
    fn set_flag(&mut self, mask: u8, set: bool) {
        self.status.0 = (self.status.0 & !mask) | if set { mask } else { 0 };
    }

    #[inline(always)]
    fn nz(&mut self, value: u8) {
        self.set_flag(Status::ZERO, value == 0);
        self.set_flag(Status::NEGATIVE, value & 0x80 != 0);
    }

    #[inline(always)]
    fn push<B: Bus>(&mut self, bus: &mut B, value: u8) {
        self.write(bus, 0x0100 | self.sp as u16, value);
        self.sp = self.sp.wrapping_sub(1);
    }

    #[inline(always)]
    fn pull<B: Bus>(&mut self, bus: &mut B) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        self.read(bus, 0x0100 | self.sp as u16)
    }

    /// Execute exactly one instruction (or interrupt/reset sequence) by
    /// repeatedly advancing the real one-cycle clock engine.
    pub fn step<B: Bus>(&mut self, bus: &mut B) -> u8 {
        let start = self.cycles;
        loop {
            if self.clock(bus).instruction_complete {
                break;
            }
        }
        self.cycles.wrapping_sub(start) as u8
    }

    /// Advance the processor by exactly one physical clock/bus cycle.
    pub fn clock<B: Bus>(&mut self, bus: &mut B) -> ClockResult {
        let input = bus.pins();
        self.observe_pins(input);
        if input.reset && !matches!(self.engine, Engine::Reset) {
            self.reset_pending = true;
        }
        if self.reset_pending && !matches!(self.engine, Engine::Reset) {
            self.engine = Engine::Reset;
            self.phase = 0;
            self.reset_pending = false;
        }

        let sync = matches!(self.engine, Engine::Fetch);
        let active_engine = self.engine;
        let complete = match active_engine {
            Engine::Fetch => self.clock_fetch(bus),
            Engine::Instruction => self.clock_action(bus),
            Engine::Interrupt(kind) => self.clock_interrupt(bus, kind),
            Engine::Reset => self.clock_reset(bus),
            Engine::Jammed => {
                self.tick_read(bus, 0xffff);
                false
            }
        };
        if complete {
            self.irq_pending = matches!(active_engine, Engine::Instruction)
                && self.last_irq
                && !self.i_before_cycle;
            if !matches!(self.engine, Engine::Jammed) {
                self.engine = Engine::Fetch;
            }
        }
        ClockResult { sync, instruction_complete: complete, jammed: self.jammed() }
    }

    pub fn jammed(&self) -> bool {
        matches!(self.engine, Engine::Jammed)
    }

    /// Schedule the seven-cycle hardware reset sequence.
    pub fn request_reset(&mut self) {
        self.reset_pending = true;
    }

    fn observe_pins(&mut self, pins: Pins) {
        if pins.nmi && !self.previous_nmi {
            self.nmi_pending = true;
        }
        if pins.set_overflow && !self.previous_so {
            self.status.0 |= Status::OVERFLOW;
        }
        self.previous_nmi = pins.nmi;
        self.previous_so = pins.set_overflow;
    }

    fn tick_read<B: Bus>(&mut self, bus: &mut B, address: u16) -> Option<u8> {
        self.i_before_cycle = self.flag(Status::INTERRUPT_DISABLE);
        self.cycles = self.cycles.wrapping_add(1);
        let value = bus.read(address);
        let pins = bus.pins();
        self.observe_pins(pins);
        self.last_irq = pins.irq;
        if pins.reset && !matches!(self.engine, Engine::Reset) {
            self.reset_pending = true;
        }
        pins.ready.then_some(value)
    }

    fn tick_write<B: Bus>(&mut self, bus: &mut B, address: u16, value: u8) {
        self.i_before_cycle = self.flag(Status::INTERRUPT_DISABLE);
        self.cycles = self.cycles.wrapping_add(1);
        bus.write(address, value);
        let pins = bus.pins();
        self.observe_pins(pins);
        self.last_irq = pins.irq;
        if pins.reset && !matches!(self.engine, Engine::Reset) {
            self.reset_pending = true;
        }
    }

    fn clock_fetch<B: Bus>(&mut self, bus: &mut B) -> bool {
        if self.nmi_pending {
            self.nmi_pending = false;
            self.engine = Engine::Interrupt(Interrupt::Nmi);
            self.phase = 0;
            return self.clock_interrupt(bus, Interrupt::Nmi);
        }
        if self.irq_pending {
            self.irq_pending = false;
            self.engine = Engine::Interrupt(Interrupt::Irq);
            self.phase = 0;
            return self.clock_interrupt(bus, Interrupt::Irq);
        }
        if let Some(opcode) = self.tick_read(bus, self.pc) {
            self.opcode = opcode;
            self.pc = self.pc.wrapping_add(1);
            self.phase = 0;
            self.engine = Engine::Instruction;
        }
        false
    }

    fn clock_interrupt<B: Bus>(&mut self, bus: &mut B, kind: Interrupt) -> bool {
        match self.phase {
            0 | 1 => {
                if self.tick_read(bus, self.pc).is_some() {
                    self.phase += 1;
                }
            }
            2 => {
                self.tick_write(bus, 0x0100 | self.sp as u16, (self.pc >> 8) as u8);
                self.sp = self.sp.wrapping_sub(1);
                self.phase = 3;
            }
            3 => {
                self.tick_write(bus, 0x0100 | self.sp as u16, self.pc as u8);
                self.sp = self.sp.wrapping_sub(1);
                self.phase = 4;
            }
            4 => {
                self.tick_write(
                    bus,
                    0x0100 | self.sp as u16,
                    (self.status.0 | Status::UNUSED) & !Status::BREAK,
                );
                self.sp = self.sp.wrapping_sub(1);
                self.set_flag(Status::INTERRUPT_DISABLE, true);
                self.phase = 5;
            }
            5 => {
                let vector = if matches!(kind, Interrupt::Nmi) { 0xfffa } else { 0xfffe };
                if let Some(value) = self.tick_read(bus, vector) {
                    self.lo = value;
                    self.phase = 6;
                }
            }
            6 => {
                let vector = if matches!(kind, Interrupt::Nmi) { 0xfffb } else { 0xffff };
                if let Some(value) = self.tick_read(bus, vector) {
                    self.pc = u16::from_le_bytes([self.lo, value]);
                    return true;
                }
            }
            _ => unreachable!(),
        }
        false
    }

    fn clock_reset<B: Bus>(&mut self, bus: &mut B) -> bool {
        match self.phase {
            0 | 1 => {
                if self.tick_read(bus, self.pc).is_some() {
                    self.phase += 1;
                }
            }
            2..=4 => {
                if self.tick_read(bus, 0x0100 | self.sp as u16).is_some() {
                    self.sp = self.sp.wrapping_sub(1);
                    self.phase += 1;
                }
            }
            5 => {
                if let Some(value) = self.tick_read(bus, 0xfffc) {
                    self.lo = value;
                    self.phase = 6;
                }
            }
            6 => {
                if let Some(value) = self.tick_read(bus, 0xfffd) {
                    self.pc = u16::from_le_bytes([self.lo, value]);
                    self.set_flag(Status::INTERRUPT_DISABLE, true);
                    return true;
                }
            }
            _ => unreachable!(),
        }
        false
    }

    /// Original instruction-atomic executor retained as an explicitly named
    /// fast path for profiling; system emulation should use `step`/`clock`.
    pub fn step_fast<B: Bus>(&mut self, bus: &mut B) -> u8 {
        let start = self.cycles;
        let opcode = self.fetch(bus);
        self.execute(bus, opcode);
        self.cycles.wrapping_sub(start) as u8
    }

    fn clock_action<B: Bus>(&mut self, bus: &mut B) -> bool {
        match decode(self.opcode) {
            Action::Read(op, mode) => {
                if let Some(value) = self.clock_read_mode(bus, mode) {
                    if self.opcode == 0xab {
                        let result = (self.a | 0xee) & value;
                        self.a = result;
                        self.x = result;
                        self.nz(result);
                    } else {
                        self.apply_read(op, value);
                    }
                    true
                } else {
                    false
                }
            }
            Action::Nop(mode) => self.clock_read_mode(bus, mode).is_some(),
            Action::Write(op, mode) => self.clock_write_mode(bus, op, mode),
            Action::Rmw(op, mode) => self.clock_rmw_mode(bus, op, mode),
            Action::Accumulator(op) => {
                if self.tick_read(bus, self.pc).is_some() {
                    self.accumulator(op);
                    true
                } else {
                    false
                }
            }
            Action::Implied(op) => {
                if self.tick_read(bus, self.pc).is_some() {
                    self.apply_implied(op);
                    true
                } else {
                    false
                }
            }
            Action::Branch(mask, set) => self.clock_branch(bus, self.flag(mask) == set),
            Action::Brk => self.clock_brk(bus),
            Action::Jsr => self.clock_jsr(bus),
            Action::Rti => self.clock_rti(bus),
            Action::Rts => self.clock_rts(bus),
            Action::JmpAbs => self.clock_jmp_abs(bus),
            Action::JmpInd => self.clock_jmp_ind(bus),
            Action::Php => self.clock_push(bus, self.status.0 | 0x30),
            Action::Pha => self.clock_push(bus, self.a),
            Action::Plp => self.clock_pull(bus, true),
            Action::Pla => self.clock_pull(bus, false),
            Action::Kil => self.clock_kil(bus),
        }
    }

    fn clock_read_mode<B: Bus>(&mut self, bus: &mut B, mode: Mode) -> Option<u8> {
        match mode {
            Mode::Imm => {
                let value = self.tick_read(bus, self.pc)?;
                self.pc = self.pc.wrapping_add(1);
                Some(value)
            }
            Mode::Zp => match self.phase {
                0 => {
                    self.lo = self.tick_read(bus, self.pc)?;
                    self.pc = self.pc.wrapping_add(1);
                    self.phase = 1;
                    None
                }
                1 => self.tick_read(bus, self.lo as u16),
                _ => unreachable!(),
            },
            Mode::ZpX | Mode::ZpY => {
                let index = if matches!(mode, Mode::ZpX) { self.x } else { self.y };
                match self.phase {
                    0 => {
                        self.lo = self.tick_read(bus, self.pc)?;
                        self.pc = self.pc.wrapping_add(1);
                        self.phase = 1;
                        None
                    }
                    1 => {
                        self.tick_read(bus, self.lo as u16)?;
                        self.address = self.lo.wrapping_add(index) as u16;
                        self.phase = 2;
                        None
                    }
                    2 => self.tick_read(bus, self.address),
                    _ => unreachable!(),
                }
            }
            Mode::Abs => match self.phase {
                0 => {
                    self.lo = self.tick_read(bus, self.pc)?;
                    self.pc = self.pc.wrapping_add(1);
                    self.phase = 1;
                    None
                }
                1 => {
                    self.hi = self.tick_read(bus, self.pc)?;
                    self.pc = self.pc.wrapping_add(1);
                    self.address = u16::from_le_bytes([self.lo, self.hi]);
                    self.phase = 2;
                    None
                }
                2 => self.tick_read(bus, self.address),
                _ => unreachable!(),
            },
            Mode::AbsX | Mode::AbsY => {
                let index = if matches!(mode, Mode::AbsX) { self.x } else { self.y };
                match self.phase {
                    0 => {
                        self.lo = self.tick_read(bus, self.pc)?;
                        self.pc = self.pc.wrapping_add(1);
                        self.phase = 1;
                        None
                    }
                    1 => {
                        self.hi = self.tick_read(bus, self.pc)?;
                        self.pc = self.pc.wrapping_add(1);
                        self.base = u16::from_le_bytes([self.lo, self.hi]);
                        self.address = self.base.wrapping_add(index as u16);
                        self.phase = 2;
                        None
                    }
                    2 => {
                        let wrong = (self.base & 0xff00) | (self.address & 0xff);
                        let value = self.tick_read(bus, wrong)?;
                        if wrong == self.address {
                            Some(value)
                        } else {
                            self.phase = 3;
                            None
                        }
                    }
                    3 => self.tick_read(bus, self.address),
                    _ => unreachable!(),
                }
            }
            Mode::IndX => match self.phase {
                0 => {
                    self.lo = self.tick_read(bus, self.pc)?;
                    self.pc = self.pc.wrapping_add(1);
                    self.phase = 1;
                    None
                }
                1 => {
                    self.tick_read(bus, self.lo as u16)?;
                    self.lo = self.lo.wrapping_add(self.x);
                    self.phase = 2;
                    None
                }
                2 => {
                    self.data = self.tick_read(bus, self.lo as u16)?;
                    self.phase = 3;
                    None
                }
                3 => {
                    self.hi = self.tick_read(bus, self.lo.wrapping_add(1) as u16)?;
                    self.address = u16::from_le_bytes([self.data, self.hi]);
                    self.phase = 4;
                    None
                }
                4 => self.tick_read(bus, self.address),
                _ => unreachable!(),
            },
            Mode::IndY => match self.phase {
                0 => {
                    self.lo = self.tick_read(bus, self.pc)?;
                    self.pc = self.pc.wrapping_add(1);
                    self.phase = 1;
                    None
                }
                1 => {
                    self.data = self.tick_read(bus, self.lo as u16)?;
                    self.phase = 2;
                    None
                }
                2 => {
                    self.hi = self.tick_read(bus, self.lo.wrapping_add(1) as u16)?;
                    self.base = u16::from_le_bytes([self.data, self.hi]);
                    self.address = self.base.wrapping_add(self.y as u16);
                    self.phase = 3;
                    None
                }
                3 => {
                    let wrong = (self.base & 0xff00) | (self.address & 0xff);
                    let value = self.tick_read(bus, wrong)?;
                    if wrong == self.address {
                        Some(value)
                    } else {
                        self.phase = 4;
                        None
                    }
                }
                4 => self.tick_read(bus, self.address),
                _ => unreachable!(),
            },
        }
    }

    fn write_value(&mut self, op: WriteOp) -> u8 {
        match op {
            WriteOp::A => self.a,
            WriteOp::X => self.x,
            WriteOp::Y => self.y,
            WriteOp::Sax => self.a & self.x,
            WriteOp::Ahx => self.a & self.x & self.hi.wrapping_add(1),
            WriteOp::Tas => {
                self.sp = self.a & self.x;
                self.sp & self.hi.wrapping_add(1)
            }
            WriteOp::Shy => self.y & self.hi.wrapping_add(1),
            WriteOp::Shx => self.x & self.hi.wrapping_add(1),
        }
    }

    fn clock_write_mode<B: Bus>(&mut self, bus: &mut B, op: WriteOp, mode: Mode) -> bool {
        match mode {
            Mode::Zp => match self.phase {
                0 => {
                    if let Some(v) = self.tick_read(bus, self.pc) {
                        self.pc = self.pc.wrapping_add(1);
                        self.address = v as u16;
                        self.phase = 1;
                    }
                    false
                }
                1 => {
                    let value = self.write_value(op);
                    self.tick_write(bus, self.address, value);
                    true
                }
                _ => unreachable!(),
            },
            Mode::ZpX | Mode::ZpY => {
                let index = if matches!(mode, Mode::ZpX) { self.x } else { self.y };
                match self.phase {
                    0 => {
                        if let Some(v) = self.tick_read(bus, self.pc) {
                            self.pc = self.pc.wrapping_add(1);
                            self.lo = v;
                            self.phase = 1;
                        }
                        false
                    }
                    1 => {
                        if self.tick_read(bus, self.lo as u16).is_some() {
                            self.address = self.lo.wrapping_add(index) as u16;
                            self.phase = 2;
                        }
                        false
                    }
                    2 => {
                        let value = self.write_value(op);
                        self.tick_write(bus, self.address, value);
                        true
                    }
                    _ => unreachable!(),
                }
            }
            Mode::Abs => match self.phase {
                0 => {
                    if let Some(v) = self.tick_read(bus, self.pc) {
                        self.pc = self.pc.wrapping_add(1);
                        self.lo = v;
                        self.phase = 1;
                    }
                    false
                }
                1 => {
                    if let Some(v) = self.tick_read(bus, self.pc) {
                        self.pc = self.pc.wrapping_add(1);
                        self.hi = v;
                        self.address = u16::from_le_bytes([self.lo, v]);
                        self.phase = 2;
                    }
                    false
                }
                2 => {
                    let value = self.write_value(op);
                    self.tick_write(bus, self.address, value);
                    true
                }
                _ => unreachable!(),
            },
            Mode::AbsX | Mode::AbsY => {
                let index = if matches!(mode, Mode::AbsX) { self.x } else { self.y };
                match self.phase {
                    0 => {
                        if let Some(v) = self.tick_read(bus, self.pc) {
                            self.pc = self.pc.wrapping_add(1);
                            self.lo = v;
                            self.phase = 1;
                        }
                        false
                    }
                    1 => {
                        if let Some(v) = self.tick_read(bus, self.pc) {
                            self.pc = self.pc.wrapping_add(1);
                            self.hi = v;
                            self.base = u16::from_le_bytes([self.lo, v]);
                            self.address = self.base.wrapping_add(index as u16);
                            self.phase = 2;
                        }
                        false
                    }
                    2 => {
                        let wrong = (self.base & 0xff00) | (self.address & 0xff);
                        if self.tick_read(bus, wrong).is_some() {
                            self.phase = 3;
                        }
                        false
                    }
                    3 => {
                        let value = self.write_value(op);
                        let unstable =
                            matches!(op, WriteOp::Ahx | WriteOp::Tas | WriteOp::Shy | WriteOp::Shx);
                        let address = if unstable && self.base & 0xff00 != self.address & 0xff00 {
                            (value as u16) << 8 | (self.address & 0xff)
                        } else {
                            self.address
                        };
                        self.tick_write(bus, address, value);
                        true
                    }
                    _ => unreachable!(),
                }
            }
            Mode::IndX => match self.phase {
                0 => {
                    if let Some(v) = self.tick_read(bus, self.pc) {
                        self.pc = self.pc.wrapping_add(1);
                        self.lo = v;
                        self.phase = 1;
                    }
                    false
                }
                1 => {
                    if self.tick_read(bus, self.lo as u16).is_some() {
                        self.lo = self.lo.wrapping_add(self.x);
                        self.phase = 2;
                    }
                    false
                }
                2 => {
                    if let Some(v) = self.tick_read(bus, self.lo as u16) {
                        self.data = v;
                        self.phase = 3;
                    }
                    false
                }
                3 => {
                    if let Some(v) = self.tick_read(bus, self.lo.wrapping_add(1) as u16) {
                        self.hi = v;
                        self.address = u16::from_le_bytes([self.data, v]);
                        self.phase = 4;
                    }
                    false
                }
                4 => {
                    let value = self.write_value(op);
                    self.tick_write(bus, self.address, value);
                    true
                }
                _ => unreachable!(),
            },
            Mode::IndY => match self.phase {
                0 => {
                    if let Some(v) = self.tick_read(bus, self.pc) {
                        self.pc = self.pc.wrapping_add(1);
                        self.lo = v;
                        self.phase = 1;
                    }
                    false
                }
                1 => {
                    if let Some(v) = self.tick_read(bus, self.lo as u16) {
                        self.data = v;
                        self.phase = 2;
                    }
                    false
                }
                2 => {
                    if let Some(v) = self.tick_read(bus, self.lo.wrapping_add(1) as u16) {
                        self.hi = v;
                        self.base = u16::from_le_bytes([self.data, v]);
                        self.address = self.base.wrapping_add(self.y as u16);
                        self.phase = 3;
                    }
                    false
                }
                3 => {
                    let wrong = (self.base & 0xff00) | (self.address & 0xff);
                    if self.tick_read(bus, wrong).is_some() {
                        self.phase = 4;
                    }
                    false
                }
                4 => {
                    let value = self.write_value(op);
                    let address = if matches!(op, WriteOp::Ahx)
                        && self.base & 0xff00 != self.address & 0xff00
                    {
                        (value as u16) << 8 | (self.address & 0xff)
                    } else {
                        self.address
                    };
                    self.tick_write(bus, address, value);
                    true
                }
                _ => unreachable!(),
            },
            Mode::Imm => unreachable!(),
        }
    }

    fn clock_rmw_mode<B: Bus>(&mut self, bus: &mut B, op: RmwOp, mode: Mode) -> bool {
        let read_phase = match mode {
            Mode::Zp => 1,
            Mode::ZpX => 2,
            Mode::Abs => 2,
            Mode::AbsX | Mode::AbsY => 3,
            Mode::IndX | Mode::IndY => 4,
            _ => unreachable!(),
        };
        if self.phase < read_phase {
            // Address generation is identical to a store through its dummy-read cycle.
            return self.clock_rmw_address(bus, mode);
        }
        if self.phase == read_phase {
            if let Some(v) = self.tick_read(bus, self.address) {
                self.data = v;
                self.phase += 1;
            }
            return false;
        }
        if self.phase == read_phase + 1 {
            self.tick_write(bus, self.address, self.data);
            self.phase += 1;
            return false;
        }
        let value = self.apply_rmw(op, self.data);
        self.tick_write(bus, self.address, value);
        true
    }

    fn clock_rmw_address<B: Bus>(&mut self, bus: &mut B, mode: Mode) -> bool {
        match mode {
            Mode::Zp => {
                if let Some(v) = self.tick_read(bus, self.pc) {
                    self.pc = self.pc.wrapping_add(1);
                    self.address = v as u16;
                    self.phase = 1;
                }
            }
            Mode::ZpX => match self.phase {
                0 => {
                    if let Some(v) = self.tick_read(bus, self.pc) {
                        self.pc = self.pc.wrapping_add(1);
                        self.lo = v;
                        self.phase = 1;
                    }
                }
                1 => {
                    if self.tick_read(bus, self.lo as u16).is_some() {
                        self.address = self.lo.wrapping_add(self.x) as u16;
                        self.phase = 2;
                    }
                }
                _ => unreachable!(),
            },
            Mode::Abs => match self.phase {
                0 => {
                    if let Some(v) = self.tick_read(bus, self.pc) {
                        self.pc = self.pc.wrapping_add(1);
                        self.lo = v;
                        self.phase = 1;
                    }
                }
                1 => {
                    if let Some(v) = self.tick_read(bus, self.pc) {
                        self.pc = self.pc.wrapping_add(1);
                        self.address = u16::from_le_bytes([self.lo, v]);
                        self.phase = 2;
                    }
                }
                _ => unreachable!(),
            },
            Mode::AbsX | Mode::AbsY => {
                let index = if matches!(mode, Mode::AbsX) { self.x } else { self.y };
                match self.phase {
                    0 => {
                        if let Some(v) = self.tick_read(bus, self.pc) {
                            self.pc = self.pc.wrapping_add(1);
                            self.lo = v;
                            self.phase = 1;
                        }
                    }
                    1 => {
                        if let Some(v) = self.tick_read(bus, self.pc) {
                            self.pc = self.pc.wrapping_add(1);
                            self.base = u16::from_le_bytes([self.lo, v]);
                            self.address = self.base.wrapping_add(index as u16);
                            self.phase = 2;
                        }
                    }
                    2 => {
                        let wrong = (self.base & 0xff00) | (self.address & 0xff);
                        if self.tick_read(bus, wrong).is_some() {
                            self.phase = 3;
                        }
                    }
                    _ => unreachable!(),
                }
            }
            Mode::IndX => match self.phase {
                0 => {
                    if let Some(v) = self.tick_read(bus, self.pc) {
                        self.pc = self.pc.wrapping_add(1);
                        self.lo = v;
                        self.phase = 1;
                    }
                }
                1 => {
                    if self.tick_read(bus, self.lo as u16).is_some() {
                        self.lo = self.lo.wrapping_add(self.x);
                        self.phase = 2;
                    }
                }
                2 => {
                    if let Some(v) = self.tick_read(bus, self.lo as u16) {
                        self.data = v;
                        self.phase = 3;
                    }
                }
                3 => {
                    if let Some(v) = self.tick_read(bus, self.lo.wrapping_add(1) as u16) {
                        self.address = u16::from_le_bytes([self.data, v]);
                        self.phase = 4;
                    }
                }
                _ => unreachable!(),
            },
            Mode::IndY => match self.phase {
                0 => {
                    if let Some(v) = self.tick_read(bus, self.pc) {
                        self.pc = self.pc.wrapping_add(1);
                        self.lo = v;
                        self.phase = 1;
                    }
                }
                1 => {
                    if let Some(v) = self.tick_read(bus, self.lo as u16) {
                        self.data = v;
                        self.phase = 2;
                    }
                }
                2 => {
                    if let Some(v) = self.tick_read(bus, self.lo.wrapping_add(1) as u16) {
                        self.base = u16::from_le_bytes([self.data, v]);
                        self.address = self.base.wrapping_add(self.y as u16);
                        self.phase = 3;
                    }
                }
                3 => {
                    let wrong = (self.base & 0xff00) | (self.address & 0xff);
                    if self.tick_read(bus, wrong).is_some() {
                        self.phase = 4;
                    }
                }
                _ => unreachable!(),
            },
            _ => unreachable!(),
        }
        false
    }

    fn apply_implied(&mut self, op: ImpliedOp) {
        match op {
            ImpliedOp::Nop => {}
            ImpliedOp::Clc => self.set_flag(Status::CARRY, false),
            ImpliedOp::Sec => self.set_flag(Status::CARRY, true),
            ImpliedOp::Cli => self.set_flag(Status::INTERRUPT_DISABLE, false),
            ImpliedOp::Sei => self.set_flag(Status::INTERRUPT_DISABLE, true),
            ImpliedOp::Clv => self.set_flag(Status::OVERFLOW, false),
            ImpliedOp::Cld => self.set_flag(Status::DECIMAL, false),
            ImpliedOp::Sed => self.set_flag(Status::DECIMAL, true),
            ImpliedOp::Dey => {
                self.y = self.y.wrapping_sub(1);
                self.nz(self.y)
            }
            ImpliedOp::Iny => {
                self.y = self.y.wrapping_add(1);
                self.nz(self.y)
            }
            ImpliedOp::Dex => {
                self.x = self.x.wrapping_sub(1);
                self.nz(self.x)
            }
            ImpliedOp::Inx => {
                self.x = self.x.wrapping_add(1);
                self.nz(self.x)
            }
            ImpliedOp::Txa => {
                self.a = self.x;
                self.nz(self.a)
            }
            ImpliedOp::Tya => {
                self.a = self.y;
                self.nz(self.a)
            }
            ImpliedOp::Txs => self.sp = self.x,
            ImpliedOp::Tax => {
                self.x = self.a;
                self.nz(self.x)
            }
            ImpliedOp::Tay => {
                self.y = self.a;
                self.nz(self.y)
            }
            ImpliedOp::Tsx => {
                self.x = self.sp;
                self.nz(self.x)
            }
        }
    }

    fn clock_branch<B: Bus>(&mut self, bus: &mut B, condition: bool) -> bool {
        match self.phase {
            0 => {
                if let Some(v) = self.tick_read(bus, self.pc) {
                    self.pc = self.pc.wrapping_add(1);
                    if !condition {
                        return true;
                    }
                    self.data = v;
                    self.phase = 1;
                }
                false
            }
            1 => {
                if self.tick_read(bus, self.pc).is_some() {
                    let old = self.pc;
                    self.address = self.pc.wrapping_add_signed(self.data as i8 as i16);
                    if old & 0xff00 == self.address & 0xff00 {
                        self.pc = self.address;
                        return true;
                    }
                    self.phase = 2;
                }
                false
            }
            2 => {
                let wrong = (self.pc & 0xff00) | (self.address & 0xff);
                if self.tick_read(bus, wrong).is_some() {
                    self.pc = self.address;
                    return true;
                }
                false
            }
            _ => unreachable!(),
        }
    }

    fn clock_brk<B: Bus>(&mut self, bus: &mut B) -> bool {
        match self.phase {
            0 => {
                if self.tick_read(bus, self.pc).is_some() {
                    self.pc = self.pc.wrapping_add(1);
                    self.phase = 1;
                }
            }
            1 => {
                self.tick_write(bus, 0x0100 | self.sp as u16, (self.pc >> 8) as u8);
                self.sp = self.sp.wrapping_sub(1);
                self.phase = 2;
            }
            2 => {
                self.tick_write(bus, 0x0100 | self.sp as u16, self.pc as u8);
                self.sp = self.sp.wrapping_sub(1);
                self.phase = 3;
            }
            3 => {
                self.tick_write(bus, 0x0100 | self.sp as u16, self.status.0 | 0x30);
                self.sp = self.sp.wrapping_sub(1);
                self.set_flag(Status::INTERRUPT_DISABLE, true);
                self.phase = 4;
            }
            4 => {
                if let Some(v) = self.tick_read(bus, 0xfffe) {
                    self.lo = v;
                    self.phase = 5;
                }
            }
            5 => {
                if let Some(v) = self.tick_read(bus, 0xffff) {
                    self.pc = u16::from_le_bytes([self.lo, v]);
                    return true;
                }
            }
            _ => unreachable!(),
        }
        false
    }

    fn clock_jsr<B: Bus>(&mut self, bus: &mut B) -> bool {
        match self.phase {
            0 => {
                if let Some(v) = self.tick_read(bus, self.pc) {
                    self.pc = self.pc.wrapping_add(1);
                    self.lo = v;
                    self.phase = 1;
                }
            }
            1 => {
                if self.tick_read(bus, 0x0100 | self.sp as u16).is_some() {
                    self.phase = 2;
                }
            }
            2 => {
                self.tick_write(bus, 0x0100 | self.sp as u16, (self.pc >> 8) as u8);
                self.sp = self.sp.wrapping_sub(1);
                self.phase = 3;
            }
            3 => {
                self.tick_write(bus, 0x0100 | self.sp as u16, self.pc as u8);
                self.sp = self.sp.wrapping_sub(1);
                self.phase = 4;
            }
            4 => {
                if let Some(v) = self.tick_read(bus, self.pc) {
                    self.pc = u16::from_le_bytes([self.lo, v]);
                    return true;
                }
            }
            _ => unreachable!(),
        }
        false
    }

    fn clock_jmp_abs<B: Bus>(&mut self, bus: &mut B) -> bool {
        match self.phase {
            0 => {
                if let Some(v) = self.tick_read(bus, self.pc) {
                    self.pc = self.pc.wrapping_add(1);
                    self.lo = v;
                    self.phase = 1;
                }
            }
            1 => {
                if let Some(v) = self.tick_read(bus, self.pc) {
                    self.pc = u16::from_le_bytes([self.lo, v]);
                    return true;
                }
            }
            _ => unreachable!(),
        }
        false
    }
    fn clock_jmp_ind<B: Bus>(&mut self, bus: &mut B) -> bool {
        match self.phase {
            0 => {
                if let Some(v) = self.tick_read(bus, self.pc) {
                    self.pc = self.pc.wrapping_add(1);
                    self.lo = v;
                    self.phase = 1;
                }
            }
            1 => {
                if let Some(v) = self.tick_read(bus, self.pc) {
                    self.pc = self.pc.wrapping_add(1);
                    self.base = u16::from_le_bytes([self.lo, v]);
                    self.phase = 2;
                }
            }
            2 => {
                if let Some(v) = self.tick_read(bus, self.base) {
                    self.lo = v;
                    self.phase = 3;
                }
            }
            3 => {
                let a = (self.base & 0xff00) | (self.base.wrapping_add(1) & 0xff);
                if let Some(v) = self.tick_read(bus, a) {
                    self.pc = u16::from_le_bytes([self.lo, v]);
                    return true;
                }
            }
            _ => unreachable!(),
        }
        false
    }

    fn clock_rti<B: Bus>(&mut self, bus: &mut B) -> bool {
        match self.phase {
            0 => {
                if self.tick_read(bus, self.pc).is_some() {
                    self.phase = 1;
                }
            }
            1 => {
                if self.tick_read(bus, 0x0100 | self.sp as u16).is_some() {
                    self.phase = 2;
                }
            }
            2 => {
                let a = 0x0100 | self.sp.wrapping_add(1) as u16;
                if let Some(v) = self.tick_read(bus, a) {
                    self.sp = self.sp.wrapping_add(1);
                    self.status.0 = (v & !Status::BREAK) | Status::UNUSED;
                    self.phase = 3;
                }
            }
            3 => {
                let a = 0x0100 | self.sp.wrapping_add(1) as u16;
                if let Some(v) = self.tick_read(bus, a) {
                    self.sp = self.sp.wrapping_add(1);
                    self.lo = v;
                    self.phase = 4;
                }
            }
            4 => {
                let a = 0x0100 | self.sp.wrapping_add(1) as u16;
                if let Some(v) = self.tick_read(bus, a) {
                    self.sp = self.sp.wrapping_add(1);
                    self.pc = u16::from_le_bytes([self.lo, v]);
                    return true;
                }
            }
            _ => unreachable!(),
        }
        false
    }
    fn clock_rts<B: Bus>(&mut self, bus: &mut B) -> bool {
        match self.phase {
            0 => {
                if self.tick_read(bus, self.pc).is_some() {
                    self.phase = 1;
                }
            }
            1 => {
                if self.tick_read(bus, 0x0100 | self.sp as u16).is_some() {
                    self.phase = 2;
                }
            }
            2 => {
                let a = 0x0100 | self.sp.wrapping_add(1) as u16;
                if let Some(v) = self.tick_read(bus, a) {
                    self.sp = self.sp.wrapping_add(1);
                    self.lo = v;
                    self.phase = 3;
                }
            }
            3 => {
                let a = 0x0100 | self.sp.wrapping_add(1) as u16;
                if let Some(v) = self.tick_read(bus, a) {
                    self.sp = self.sp.wrapping_add(1);
                    self.pc = u16::from_le_bytes([self.lo, v]);
                    self.phase = 4;
                }
            }
            4 => {
                if self.tick_read(bus, self.pc).is_some() {
                    self.pc = self.pc.wrapping_add(1);
                    return true;
                }
            }
            _ => unreachable!(),
        }
        false
    }

    fn clock_push<B: Bus>(&mut self, bus: &mut B, value: u8) -> bool {
        match self.phase {
            0 => {
                if self.tick_read(bus, self.pc).is_some() {
                    self.phase = 1;
                }
                false
            }
            1 => {
                self.tick_write(bus, 0x0100 | self.sp as u16, value);
                self.sp = self.sp.wrapping_sub(1);
                true
            }
            _ => unreachable!(),
        }
    }
    fn clock_pull<B: Bus>(&mut self, bus: &mut B, status: bool) -> bool {
        match self.phase {
            0 => {
                if self.tick_read(bus, self.pc).is_some() {
                    self.phase = 1;
                }
                false
            }
            1 => {
                if self.tick_read(bus, 0x0100 | self.sp as u16).is_some() {
                    self.phase = 2;
                }
                false
            }
            2 => {
                let a = 0x0100 | self.sp.wrapping_add(1) as u16;
                if let Some(v) = self.tick_read(bus, a) {
                    self.sp = self.sp.wrapping_add(1);
                    if status {
                        self.status.0 = (v & !Status::BREAK) | Status::UNUSED
                    } else {
                        self.a = v;
                        self.nz(v)
                    }
                    return true;
                }
                false
            }
            _ => unreachable!(),
        }
    }
    fn clock_kil<B: Bus>(&mut self, bus: &mut B) -> bool {
        let address = match self.phase {
            0 => self.pc,
            1 | 4 | 5 | 6 | 7 | 8 | 9 => 0xffff,
            2 | 3 => 0xfffe,
            _ => unreachable!(),
        };
        if self.tick_read(bus, address).is_some() {
            if self.phase == 0 {
                self.pc = self.pc.wrapping_add(1);
            }
            if self.phase == 9 {
                self.pc = self.pc.wrapping_sub(1);
                self.engine = Engine::Jammed;
                return true;
            }
            self.phase += 1;
        }
        false
    }

    fn read_addr<B: Bus>(&mut self, bus: &mut B, mode: Mode) -> u8 {
        match mode {
            Mode::Imm => self.fetch(bus),
            Mode::Zp => {
                let a = self.fetch(bus);
                self.read(bus, a as u16)
            }
            Mode::ZpX => {
                let a = self.fetch(bus);
                self.read(bus, a as u16);
                self.read(bus, a.wrapping_add(self.x) as u16)
            }
            Mode::ZpY => {
                let a = self.fetch(bus);
                self.read(bus, a as u16);
                self.read(bus, a.wrapping_add(self.y) as u16)
            }
            Mode::Abs => {
                let lo = self.fetch(bus);
                let hi = self.fetch(bus);
                self.read(bus, u16::from_le_bytes([lo, hi]))
            }
            Mode::AbsX => self.read_indexed(bus, self.x),
            Mode::AbsY => self.read_indexed(bus, self.y),
            Mode::IndX => {
                let zp = self.fetch(bus);
                self.read(bus, zp as u16);
                let p = zp.wrapping_add(self.x);
                let lo = self.read(bus, p as u16);
                let hi = self.read(bus, p.wrapping_add(1) as u16);
                self.read(bus, u16::from_le_bytes([lo, hi]))
            }
            Mode::IndY => {
                let zp = self.fetch(bus);
                let lo = self.read(bus, zp as u16);
                let hi = self.read(bus, zp.wrapping_add(1) as u16);
                let base = u16::from_le_bytes([lo, hi]);
                let addr = base.wrapping_add(self.y as u16);
                let wrong = (base & 0xff00) | (addr & 0x00ff);
                let value = self.read(bus, wrong);
                if wrong != addr { self.read(bus, addr) } else { value }
            }
        }
    }

    fn read_indexed<B: Bus>(&mut self, bus: &mut B, index: u8) -> u8 {
        let lo = self.fetch(bus);
        let hi = self.fetch(bus);
        let base = u16::from_le_bytes([lo, hi]);
        let addr = base.wrapping_add(index as u16);
        let wrong = (base & 0xff00) | (addr & 0x00ff);
        let value = self.read(bus, wrong);
        if wrong != addr { self.read(bus, addr) } else { value }
    }

    fn write_addr<B: Bus>(&mut self, bus: &mut B, mode: Mode, value: u8) {
        let addr = match mode {
            Mode::Zp => self.fetch(bus) as u16,
            Mode::ZpX => {
                let a = self.fetch(bus);
                self.read(bus, a as u16);
                a.wrapping_add(self.x) as u16
            }
            Mode::ZpY => {
                let a = self.fetch(bus);
                self.read(bus, a as u16);
                a.wrapping_add(self.y) as u16
            }
            Mode::Abs => {
                let lo = self.fetch(bus);
                let hi = self.fetch(bus);
                u16::from_le_bytes([lo, hi])
            }
            Mode::AbsX | Mode::AbsY => {
                let index = if matches!(mode, Mode::AbsX) { self.x } else { self.y };
                let lo = self.fetch(bus);
                let hi = self.fetch(bus);
                let base = u16::from_le_bytes([lo, hi]);
                let addr = base.wrapping_add(index as u16);
                self.read(bus, (base & 0xff00) | (addr & 0xff));
                addr
            }
            Mode::IndX => {
                let zp = self.fetch(bus);
                self.read(bus, zp as u16);
                let p = zp.wrapping_add(self.x);
                let lo = self.read(bus, p as u16);
                let hi = self.read(bus, p.wrapping_add(1) as u16);
                u16::from_le_bytes([lo, hi])
            }
            Mode::IndY => {
                let zp = self.fetch(bus);
                let lo = self.read(bus, zp as u16);
                let hi = self.read(bus, zp.wrapping_add(1) as u16);
                let base = u16::from_le_bytes([lo, hi]);
                let addr = base.wrapping_add(self.y as u16);
                self.read(bus, (base & 0xff00) | (addr & 0xff));
                addr
            }
            Mode::Imm => unreachable!(),
        };
        self.write(bus, addr, value);
    }

    fn rmw_addr<B: Bus>(&mut self, bus: &mut B, mode: Mode, op: RmwOp) {
        let addr = match mode {
            Mode::Zp => self.fetch(bus) as u16,
            Mode::ZpX => {
                let a = self.fetch(bus);
                self.read(bus, a as u16);
                a.wrapping_add(self.x) as u16
            }
            Mode::Abs => {
                let lo = self.fetch(bus);
                let hi = self.fetch(bus);
                u16::from_le_bytes([lo, hi])
            }
            Mode::AbsX | Mode::AbsY => {
                let index = if matches!(mode, Mode::AbsX) { self.x } else { self.y };
                let lo = self.fetch(bus);
                let hi = self.fetch(bus);
                let base = u16::from_le_bytes([lo, hi]);
                let addr = base.wrapping_add(index as u16);
                self.read(bus, (base & 0xff00) | (addr & 0xff));
                addr
            }
            Mode::IndX => {
                let zp = self.fetch(bus);
                self.read(bus, zp as u16);
                let p = zp.wrapping_add(self.x);
                let lo = self.read(bus, p as u16);
                let hi = self.read(bus, p.wrapping_add(1) as u16);
                u16::from_le_bytes([lo, hi])
            }
            Mode::IndY => {
                let zp = self.fetch(bus);
                let lo = self.read(bus, zp as u16);
                let hi = self.read(bus, zp.wrapping_add(1) as u16);
                let base = u16::from_le_bytes([lo, hi]);
                let addr = base.wrapping_add(self.y as u16);
                self.read(bus, (base & 0xff00) | (addr & 0xff));
                addr
            }
            _ => unreachable!(),
        };
        let old = self.read(bus, addr);
        self.write(bus, addr, old);
        let new = self.apply_rmw(op, old);
        self.write(bus, addr, new);
    }

    fn apply_read(&mut self, op: ReadOp, value: u8) {
        match op {
            ReadOp::Ora => {
                self.a |= value;
                self.nz(self.a);
            }
            ReadOp::And => {
                self.a &= value;
                self.nz(self.a);
            }
            ReadOp::Eor => {
                self.a ^= value;
                self.nz(self.a);
            }
            ReadOp::Adc => self.adc(value),
            ReadOp::Lda => {
                self.a = value;
                self.nz(value);
            }
            ReadOp::Cmp => self.compare(self.a, value),
            ReadOp::Sbc => self.sbc(value),
            ReadOp::Ldx => {
                self.x = value;
                self.nz(value);
            }
            ReadOp::Ldy => {
                self.y = value;
                self.nz(value);
            }
            ReadOp::Cpx => self.compare(self.x, value),
            ReadOp::Cpy => self.compare(self.y, value),
            ReadOp::Bit => {
                self.set_flag(Status::ZERO, self.a & value == 0);
                self.status.0 = (self.status.0 & 0x3f) | (value & 0xc0);
            }
            ReadOp::Lax => {
                self.a = value;
                self.x = value;
                self.nz(value);
            }
            ReadOp::Anc => {
                self.a &= value;
                self.nz(self.a);
                self.set_flag(Status::CARRY, self.a & 0x80 != 0);
            }
            ReadOp::Alr => {
                self.a &= value;
                self.set_flag(Status::CARRY, self.a & 1 != 0);
                self.a >>= 1;
                self.nz(self.a);
            }
            ReadOp::Arr => self.arr(value),
            ReadOp::Xaa => {
                self.a = (self.a | 0xee) & self.x & value;
                self.nz(self.a);
            }
            ReadOp::Axs => {
                let v = self.a & self.x;
                self.x = v.wrapping_sub(value);
                self.set_flag(Status::CARRY, v >= value);
                self.nz(self.x);
            }
            ReadOp::Las => {
                let v = value & self.sp;
                self.a = v;
                self.x = v;
                self.sp = v;
                self.nz(v);
            }
        }
    }

    fn apply_rmw(&mut self, op: RmwOp, old: u8) -> u8 {
        let value = match op {
            RmwOp::Asl | RmwOp::Slo => {
                self.set_flag(Status::CARRY, old & 0x80 != 0);
                old << 1
            }
            RmwOp::Rol | RmwOp::Rla => {
                let c = self.flag(Status::CARRY) as u8;
                self.set_flag(Status::CARRY, old & 0x80 != 0);
                (old << 1) | c
            }
            RmwOp::Lsr | RmwOp::Sre => {
                self.set_flag(Status::CARRY, old & 1 != 0);
                old >> 1
            }
            RmwOp::Ror | RmwOp::Rra => {
                let c = if self.flag(Status::CARRY) { 0x80 } else { 0 };
                self.set_flag(Status::CARRY, old & 1 != 0);
                (old >> 1) | c
            }
            RmwOp::Dec | RmwOp::Dcp => old.wrapping_sub(1),
            RmwOp::Inc | RmwOp::Isc => old.wrapping_add(1),
        };
        match op {
            RmwOp::Slo => {
                self.a |= value;
                self.nz(self.a);
            }
            RmwOp::Rla => {
                self.a &= value;
                self.nz(self.a);
            }
            RmwOp::Sre => {
                self.a ^= value;
                self.nz(self.a);
            }
            RmwOp::Rra => self.adc(value),
            RmwOp::Dcp => self.compare(self.a, value),
            RmwOp::Isc => self.sbc(value),
            _ => self.nz(value),
        }
        value
    }

    fn compare(&mut self, lhs: u8, rhs: u8) {
        self.set_flag(Status::CARRY, lhs >= rhs);
        self.nz(lhs.wrapping_sub(rhs));
    }

    fn adc(&mut self, value: u8) {
        let a = self.a;
        let carry = self.flag(Status::CARRY) as u16;
        let binary = a as u16 + value as u16 + carry;
        self.set_flag(Status::OVERFLOW, (!(a ^ value) & (a ^ binary as u8) & 0x80) != 0);
        if self.flag(Status::DECIMAL) {
            // The NMOS part corrects each nibble independently. This matters
            // for the deliberately invalid BCD operands in SingleStepTests.
            let mut low = (a & 0x0f) + (value & 0x0f) + carry as u8;
            if low > 9 {
                low = low.wrapping_add(6);
            }
            let mut high = (a >> 4) + (value >> 4) + u8::from(low > 0x0f);
            let intermediate = (high << 4) | (low & 0x0f);
            self.set_flag(Status::ZERO, binary as u8 == 0);
            self.set_flag(Status::NEGATIVE, high & 0x08 != 0);
            self.set_flag(Status::OVERFLOW, (!(a ^ value) & (a ^ intermediate) & 0x80) != 0);
            if high > 9 {
                high = high.wrapping_add(6);
            }
            self.set_flag(Status::CARRY, high > 0x0f);
            self.a = (high << 4) | (low & 0x0f);
        } else {
            self.set_flag(Status::CARRY, binary > 0xff);
            self.a = binary as u8;
            self.nz(self.a);
        }
    }

    fn sbc(&mut self, value: u8) {
        let a = self.a;
        let borrow = (!self.flag(Status::CARRY)) as i16;
        let binary = a as i16 - value as i16 - borrow;
        let result = binary as u8;
        self.set_flag(Status::OVERFLOW, ((a ^ result) & (a ^ value) & 0x80) != 0);
        self.set_flag(Status::CARRY, binary >= 0);
        self.nz(result);
        if self.flag(Status::DECIMAL) {
            let mut lo = (a & 0x0f) as i16 - (value & 0x0f) as i16 - borrow;
            let mut hi = (a >> 4) as i16 - (value >> 4) as i16;
            if lo < 0 {
                lo -= 6;
                hi -= 1;
            }
            if hi < 0 {
                hi -= 6;
            }
            self.a = (((hi << 4) | (lo & 0x0f)) & 0xff) as u8;
        } else {
            self.a = result;
        }
    }

    fn arr(&mut self, value: u8) {
        let anded = self.a & value;
        let carry_in = if self.flag(Status::CARRY) { 0x80 } else { 0 };
        let rotated = (anded >> 1) | carry_in;
        if self.flag(Status::DECIMAL) {
            let mut result = rotated;
            self.set_flag(Status::ZERO, result == 0);
            self.set_flag(Status::NEGATIVE, carry_in != 0);
            self.set_flag(Status::OVERFLOW, ((rotated ^ anded) & 0x40) != 0);
            if (anded & 0x0f) + (anded & 1) > 5 {
                result = (result & 0xf0) | ((result.wrapping_add(6)) & 0x0f);
            }
            if (anded & 0xf0) as u16 + (anded & 0x10) as u16 > 0x50 {
                result = result.wrapping_add(0x60);
                self.set_flag(Status::CARRY, true);
            } else {
                self.set_flag(Status::CARRY, false);
            }
            self.a = result;
        } else {
            self.a = rotated;
            self.nz(rotated);
            self.set_flag(Status::CARRY, rotated & 0x40 != 0);
            self.set_flag(Status::OVERFLOW, ((rotated >> 6) ^ (rotated >> 5)) & 1 != 0);
        }
    }

    fn branch<B: Bus>(&mut self, bus: &mut B, condition: bool) {
        let offset = self.fetch(bus) as i8;
        if condition {
            self.read(bus, self.pc);
            let old = self.pc;
            let new = self.pc.wrapping_add_signed(offset as i16);
            if old & 0xff00 != new & 0xff00 {
                self.read(bus, (old & 0xff00) | (new & 0xff));
            }
            self.pc = new;
        }
    }

    fn accumulator(&mut self, op: RmwOp) {
        self.a = self.apply_rmw(op, self.a);
    }

    fn implied<B: Bus>(&mut self, bus: &mut B) {
        self.read(bus, self.pc);
    }

    fn read_op<B: Bus>(&mut self, bus: &mut B, mode: Mode, op: ReadOp) {
        let v = self.read_addr(bus, mode);
        self.apply_read(op, v);
    }

    fn execute<B: Bus>(&mut self, bus: &mut B, opcode: u8) {
        use Mode::*;
        use ReadOp::*;
        use RmwOp::*;
        match opcode {
            0x00 => {
                self.fetch(bus);
                self.push(bus, (self.pc >> 8) as u8);
                self.push(bus, self.pc as u8);
                self.push(bus, self.status.0 | 0x30);
                self.set_flag(Status::INTERRUPT_DISABLE, true);
                let lo = self.read(bus, 0xfffe);
                let hi = self.read(bus, 0xffff);
                self.pc = u16::from_le_bytes([lo, hi]);
            }
            0x01 => self.read_op(bus, IndX, Ora),
            0x05 => self.read_op(bus, Zp, Ora),
            0x09 => self.read_op(bus, Imm, Ora),
            0x0d => self.read_op(bus, Abs, Ora),
            0x11 => self.read_op(bus, IndY, Ora),
            0x15 => self.read_op(bus, ZpX, Ora),
            0x19 => self.read_op(bus, AbsY, Ora),
            0x1d => self.read_op(bus, AbsX, Ora),
            0x21 => self.read_op(bus, IndX, And),
            0x25 => self.read_op(bus, Zp, And),
            0x29 => self.read_op(bus, Imm, And),
            0x2d => self.read_op(bus, Abs, And),
            0x31 => self.read_op(bus, IndY, And),
            0x35 => self.read_op(bus, ZpX, And),
            0x39 => self.read_op(bus, AbsY, And),
            0x3d => self.read_op(bus, AbsX, And),
            0x41 => self.read_op(bus, IndX, Eor),
            0x45 => self.read_op(bus, Zp, Eor),
            0x49 => self.read_op(bus, Imm, Eor),
            0x4d => self.read_op(bus, Abs, Eor),
            0x51 => self.read_op(bus, IndY, Eor),
            0x55 => self.read_op(bus, ZpX, Eor),
            0x59 => self.read_op(bus, AbsY, Eor),
            0x5d => self.read_op(bus, AbsX, Eor),
            0x61 => self.read_op(bus, IndX, Adc),
            0x65 => self.read_op(bus, Zp, Adc),
            0x69 => self.read_op(bus, Imm, Adc),
            0x6d => self.read_op(bus, Abs, Adc),
            0x71 => self.read_op(bus, IndY, Adc),
            0x75 => self.read_op(bus, ZpX, Adc),
            0x79 => self.read_op(bus, AbsY, Adc),
            0x7d => self.read_op(bus, AbsX, Adc),
            0xa1 => self.read_op(bus, IndX, Lda),
            0xa5 => self.read_op(bus, Zp, Lda),
            0xa9 => self.read_op(bus, Imm, Lda),
            0xad => self.read_op(bus, Abs, Lda),
            0xb1 => self.read_op(bus, IndY, Lda),
            0xb5 => self.read_op(bus, ZpX, Lda),
            0xb9 => self.read_op(bus, AbsY, Lda),
            0xbd => self.read_op(bus, AbsX, Lda),
            0xc1 => self.read_op(bus, IndX, Cmp),
            0xc5 => self.read_op(bus, Zp, Cmp),
            0xc9 => self.read_op(bus, Imm, Cmp),
            0xcd => self.read_op(bus, Abs, Cmp),
            0xd1 => self.read_op(bus, IndY, Cmp),
            0xd5 => self.read_op(bus, ZpX, Cmp),
            0xd9 => self.read_op(bus, AbsY, Cmp),
            0xdd => self.read_op(bus, AbsX, Cmp),
            0xe1 => self.read_op(bus, IndX, Sbc),
            0xe5 => self.read_op(bus, Zp, Sbc),
            0xe9 | 0xeb => self.read_op(bus, Imm, Sbc),
            0xed => self.read_op(bus, Abs, Sbc),
            0xf1 => self.read_op(bus, IndY, Sbc),
            0xf5 => self.read_op(bus, ZpX, Sbc),
            0xf9 => self.read_op(bus, AbsY, Sbc),
            0xfd => self.read_op(bus, AbsX, Sbc),

            0xa0 => self.read_op(bus, Imm, Ldy),
            0xa4 => self.read_op(bus, Zp, Ldy),
            0xac => self.read_op(bus, Abs, Ldy),
            0xb4 => self.read_op(bus, ZpX, Ldy),
            0xbc => self.read_op(bus, AbsX, Ldy),
            0xa2 => self.read_op(bus, Imm, Ldx),
            0xa6 => self.read_op(bus, Zp, Ldx),
            0xae => self.read_op(bus, Abs, Ldx),
            0xb6 => self.read_op(bus, ZpY, Ldx),
            0xbe => self.read_op(bus, AbsY, Ldx),
            0xc0 => self.read_op(bus, Imm, Cpy),
            0xc4 => self.read_op(bus, Zp, Cpy),
            0xcc => self.read_op(bus, Abs, Cpy),
            0xe0 => self.read_op(bus, Imm, Cpx),
            0xe4 => self.read_op(bus, Zp, Cpx),
            0xec => self.read_op(bus, Abs, Cpx),
            0x24 => self.read_op(bus, Zp, Bit),
            0x2c => self.read_op(bus, Abs, Bit),

            0x81 => self.write_addr(bus, IndX, self.a),
            0x85 => self.write_addr(bus, Zp, self.a),
            0x8d => self.write_addr(bus, Abs, self.a),
            0x91 => self.write_addr(bus, IndY, self.a),
            0x95 => self.write_addr(bus, ZpX, self.a),
            0x99 => self.write_addr(bus, AbsY, self.a),
            0x9d => self.write_addr(bus, AbsX, self.a),
            0x84 => self.write_addr(bus, Zp, self.y),
            0x8c => self.write_addr(bus, Abs, self.y),
            0x94 => self.write_addr(bus, ZpX, self.y),
            0x86 => self.write_addr(bus, Zp, self.x),
            0x8e => self.write_addr(bus, Abs, self.x),
            0x96 => self.write_addr(bus, ZpY, self.x),

            0x06 => self.rmw_addr(bus, Zp, Asl),
            0x0e => self.rmw_addr(bus, Abs, Asl),
            0x16 => self.rmw_addr(bus, ZpX, Asl),
            0x1e => self.rmw_addr(bus, AbsX, Asl),
            0x0a => {
                self.implied(bus);
                self.accumulator(Asl)
            }
            0x26 => self.rmw_addr(bus, Zp, Rol),
            0x2e => self.rmw_addr(bus, Abs, Rol),
            0x36 => self.rmw_addr(bus, ZpX, Rol),
            0x3e => self.rmw_addr(bus, AbsX, Rol),
            0x2a => {
                self.implied(bus);
                self.accumulator(Rol)
            }
            0x46 => self.rmw_addr(bus, Zp, Lsr),
            0x4e => self.rmw_addr(bus, Abs, Lsr),
            0x56 => self.rmw_addr(bus, ZpX, Lsr),
            0x5e => self.rmw_addr(bus, AbsX, Lsr),
            0x4a => {
                self.implied(bus);
                self.accumulator(Lsr)
            }
            0x66 => self.rmw_addr(bus, Zp, Ror),
            0x6e => self.rmw_addr(bus, Abs, Ror),
            0x76 => self.rmw_addr(bus, ZpX, Ror),
            0x7e => self.rmw_addr(bus, AbsX, Ror),
            0x6a => {
                self.implied(bus);
                self.accumulator(Ror)
            }
            0xc6 => self.rmw_addr(bus, Zp, Dec),
            0xce => self.rmw_addr(bus, Abs, Dec),
            0xd6 => self.rmw_addr(bus, ZpX, Dec),
            0xde => self.rmw_addr(bus, AbsX, Dec),
            0xe6 => self.rmw_addr(bus, Zp, Inc),
            0xee => self.rmw_addr(bus, Abs, Inc),
            0xf6 => self.rmw_addr(bus, ZpX, Inc),
            0xfe => self.rmw_addr(bus, AbsX, Inc),

            0x10 => self.branch(bus, !self.flag(Status::NEGATIVE)),
            0x30 => self.branch(bus, self.flag(Status::NEGATIVE)),
            0x50 => self.branch(bus, !self.flag(Status::OVERFLOW)),
            0x70 => self.branch(bus, self.flag(Status::OVERFLOW)),
            0x90 => self.branch(bus, !self.flag(Status::CARRY)),
            0xb0 => self.branch(bus, self.flag(Status::CARRY)),
            0xd0 => self.branch(bus, !self.flag(Status::ZERO)),
            0xf0 => self.branch(bus, self.flag(Status::ZERO)),
            0x18 => {
                self.implied(bus);
                self.set_flag(Status::CARRY, false)
            }
            0x38 => {
                self.implied(bus);
                self.set_flag(Status::CARRY, true)
            }
            0x58 => {
                self.implied(bus);
                self.set_flag(Status::INTERRUPT_DISABLE, false)
            }
            0x78 => {
                self.implied(bus);
                self.set_flag(Status::INTERRUPT_DISABLE, true)
            }
            0xb8 => {
                self.implied(bus);
                self.set_flag(Status::OVERFLOW, false)
            }
            0xd8 => {
                self.implied(bus);
                self.set_flag(Status::DECIMAL, false)
            }
            0xf8 => {
                self.implied(bus);
                self.set_flag(Status::DECIMAL, true)
            }
            0x88 => {
                self.implied(bus);
                self.y = self.y.wrapping_sub(1);
                self.nz(self.y)
            }
            0xc8 => {
                self.implied(bus);
                self.y = self.y.wrapping_add(1);
                self.nz(self.y)
            }
            0xca => {
                self.implied(bus);
                self.x = self.x.wrapping_sub(1);
                self.nz(self.x)
            }
            0xe8 => {
                self.implied(bus);
                self.x = self.x.wrapping_add(1);
                self.nz(self.x)
            }
            0x8a => {
                self.implied(bus);
                self.a = self.x;
                self.nz(self.a)
            }
            0x98 => {
                self.implied(bus);
                self.a = self.y;
                self.nz(self.a)
            }
            0x9a => {
                self.implied(bus);
                self.sp = self.x
            }
            0xaa => {
                self.implied(bus);
                self.x = self.a;
                self.nz(self.x)
            }
            0xa8 => {
                self.implied(bus);
                self.y = self.a;
                self.nz(self.y)
            }
            0xba => {
                self.implied(bus);
                self.x = self.sp;
                self.nz(self.x)
            }
            0x4c => {
                let lo = self.fetch(bus);
                let hi = self.fetch(bus);
                self.pc = u16::from_le_bytes([lo, hi])
            }
            0x6c => {
                let lo = self.fetch(bus);
                let hi = self.fetch(bus);
                let ptr = u16::from_le_bytes([lo, hi]);
                let l = self.read(bus, ptr);
                let h = self.read(bus, (ptr & 0xff00) | ptr.wrapping_add(1) & 0xff);
                self.pc = u16::from_le_bytes([l, h])
            }
            0x20 => {
                let lo = self.fetch(bus);
                self.read(bus, 0x0100 | self.sp as u16);
                let ret = self.pc;
                self.push(bus, (ret >> 8) as u8);
                self.push(bus, ret as u8);
                let hi = self.fetch(bus);
                self.pc = u16::from_le_bytes([lo, hi])
            }
            0x40 => {
                self.implied(bus);
                self.read(bus, 0x0100 | self.sp as u16);
                self.status.0 = (self.pull(bus) & !Status::BREAK) | Status::UNUSED;
                let lo = self.pull(bus);
                let hi = self.pull(bus);
                self.pc = u16::from_le_bytes([lo, hi])
            }
            0x60 => {
                self.implied(bus);
                self.read(bus, 0x0100 | self.sp as u16);
                let lo = self.pull(bus);
                let hi = self.pull(bus);
                self.pc = u16::from_le_bytes([lo, hi]);
                self.read(bus, self.pc);
                self.pc = self.pc.wrapping_add(1)
            }
            0x08 => {
                self.implied(bus);
                self.push(bus, self.status.0 | 0x30)
            }
            0x28 => {
                self.implied(bus);
                self.read(bus, 0x0100 | self.sp as u16);
                self.status.0 = (self.pull(bus) & !Status::BREAK) | Status::UNUSED
            }
            0x48 => {
                self.implied(bus);
                self.push(bus, self.a)
            }
            0x68 => {
                self.implied(bus);
                self.read(bus, 0x0100 | self.sp as u16);
                self.a = self.pull(bus);
                self.nz(self.a)
            }
            0xea => self.implied(bus),

            // Stable undocumented read/modify/write families.
            0x03 => self.rmw_addr(bus, IndX, Slo),
            0x07 => self.rmw_addr(bus, Zp, Slo),
            0x0f => self.rmw_addr(bus, Abs, Slo),
            0x13 => self.rmw_addr(bus, IndY, Slo),
            0x17 => self.rmw_addr(bus, ZpX, Slo),
            0x1b => self.rmw_addr(bus, AbsY, Slo),
            0x1f => self.rmw_addr(bus, AbsX, Slo),
            0x23 => self.rmw_addr(bus, IndX, Rla),
            0x27 => self.rmw_addr(bus, Zp, Rla),
            0x2f => self.rmw_addr(bus, Abs, Rla),
            0x33 => self.rmw_addr(bus, IndY, Rla),
            0x37 => self.rmw_addr(bus, ZpX, Rla),
            0x3b => self.rmw_addr(bus, AbsY, Rla),
            0x3f => self.rmw_addr(bus, AbsX, Rla),
            0x43 => self.rmw_addr(bus, IndX, Sre),
            0x47 => self.rmw_addr(bus, Zp, Sre),
            0x4f => self.rmw_addr(bus, Abs, Sre),
            0x53 => self.rmw_addr(bus, IndY, Sre),
            0x57 => self.rmw_addr(bus, ZpX, Sre),
            0x5b => self.rmw_addr(bus, AbsY, Sre),
            0x5f => self.rmw_addr(bus, AbsX, Sre),
            0x63 => self.rmw_addr(bus, IndX, Rra),
            0x67 => self.rmw_addr(bus, Zp, Rra),
            0x6f => self.rmw_addr(bus, Abs, Rra),
            0x73 => self.rmw_addr(bus, IndY, Rra),
            0x77 => self.rmw_addr(bus, ZpX, Rra),
            0x7b => self.rmw_addr(bus, AbsY, Rra),
            0x7f => self.rmw_addr(bus, AbsX, Rra),
            0xc3 => self.rmw_addr(bus, IndX, Dcp),
            0xc7 => self.rmw_addr(bus, Zp, Dcp),
            0xcf => self.rmw_addr(bus, Abs, Dcp),
            0xd3 => self.rmw_addr(bus, IndY, Dcp),
            0xd7 => self.rmw_addr(bus, ZpX, Dcp),
            0xdb => self.rmw_addr(bus, AbsY, Dcp),
            0xdf => self.rmw_addr(bus, AbsX, Dcp),
            0xe3 => self.rmw_addr(bus, IndX, Isc),
            0xe7 => self.rmw_addr(bus, Zp, Isc),
            0xef => self.rmw_addr(bus, Abs, Isc),
            0xf3 => self.rmw_addr(bus, IndY, Isc),
            0xf7 => self.rmw_addr(bus, ZpX, Isc),
            0xfb => self.rmw_addr(bus, AbsY, Isc),
            0xff => self.rmw_addr(bus, AbsX, Isc),
            0x83 => self.write_addr(bus, IndX, self.a & self.x),
            0x87 => self.write_addr(bus, Zp, self.a & self.x),
            0x8f => self.write_addr(bus, Abs, self.a & self.x),
            0x97 => self.write_addr(bus, ZpY, self.a & self.x),
            0xa3 => self.read_op(bus, IndX, Lax),
            0xa7 => self.read_op(bus, Zp, Lax),
            0xab => {
                let value = self.fetch(bus);
                let result = (self.a | 0xee) & value;
                self.a = result;
                self.x = result;
                self.nz(result);
            }
            0xaf => self.read_op(bus, Abs, Lax),
            0xb3 => self.read_op(bus, IndY, Lax),
            0xb7 => self.read_op(bus, ZpY, Lax),
            0xbf => self.read_op(bus, AbsY, Lax),
            0x0b | 0x2b => self.read_op(bus, Imm, Anc),
            0x4b => self.read_op(bus, Imm, Alr),
            0x6b => self.read_op(bus, Imm, Arr),
            0x8b => self.read_op(bus, Imm, Xaa),
            0xcb => self.read_op(bus, Imm, Axs),
            0xbb => self.read_op(bus, AbsY, Las),

            // Undocumented NOPs still perform their documented bus accesses.
            0x1a | 0x3a | 0x5a | 0x7a | 0xda | 0xfa => self.implied(bus),
            0x80 | 0x82 | 0x89 | 0xc2 | 0xe2 => {
                self.fetch(bus);
            }
            0x04 | 0x44 | 0x64 => {
                self.read_addr(bus, Zp);
            }
            0x14 | 0x34 | 0x54 | 0x74 | 0xd4 | 0xf4 => {
                self.read_addr(bus, ZpX);
            }
            0x0c => {
                self.read_addr(bus, Abs);
            }
            0x1c | 0x3c | 0x5c | 0x7c | 0xdc | 0xfc => {
                self.read_addr(bus, AbsX);
            }

            // Unstable stores: the value and, on page crossing, high address byte
            // are both driven by the indexed base high byte plus one.
            0x93 => self.unstable_store(bus, IndY, self.a & self.x),
            0x9f => self.unstable_store(bus, AbsY, self.a & self.x),
            0x9b => {
                self.sp = self.a & self.x;
                self.unstable_store(bus, AbsY, self.sp)
            }
            0x9c => self.unstable_store(bus, AbsX, self.y),
            0x9e => self.unstable_store(bus, AbsY, self.x),

            // NMOS KIL/JAM bus behavior used by the SingleStepTests suite.
            0x02 | 0x12 | 0x22 | 0x32 | 0x42 | 0x52 | 0x62 | 0x72 | 0x92 | 0xb2 | 0xd2 | 0xf2 => {
                self.fetch(bus);
                let hi = self.read(bus, 0xffff);
                let lo = self.read(bus, 0xfffe);
                self.read(bus, 0xfffe);
                self.read(bus, 0xffff);
                let _ = (hi, lo);
                for _ in 0..5 {
                    self.read(bus, 0xffff);
                }
                self.pc = self.pc.wrapping_sub(1);
            }
        }
    }

    fn unstable_store<B: Bus>(&mut self, bus: &mut B, mode: Mode, source: u8) {
        match mode {
            Mode::AbsX | Mode::AbsY => {
                let index = if matches!(mode, Mode::AbsX) { self.x } else { self.y };
                let lo = self.fetch(bus);
                let hi = self.fetch(bus);
                let base = u16::from_le_bytes([lo, hi]);
                let indexed = base.wrapping_add(index as u16);
                let wrong = (base & 0xff00) | (indexed & 0xff);
                self.read(bus, wrong);
                let value = source & hi.wrapping_add(1);
                let addr = if base & 0xff00 != indexed & 0xff00 {
                    (value as u16) << 8 | (indexed & 0xff)
                } else {
                    indexed
                };
                self.write(bus, addr, value);
            }
            Mode::IndY => {
                let zp = self.fetch(bus);
                let lo = self.read(bus, zp as u16);
                let hi = self.read(bus, zp.wrapping_add(1) as u16);
                let base = u16::from_le_bytes([lo, hi]);
                let indexed = base.wrapping_add(self.y as u16);
                let wrong = (base & 0xff00) | (indexed & 0xff);
                self.read(bus, wrong);
                let value = source & hi.wrapping_add(1);
                let addr = if base & 0xff00 != indexed & 0xff00 {
                    (value as u16) << 8 | (indexed & 0xff)
                } else {
                    indexed
                };
                self.write(bus, addr, value);
            }
            _ => unreachable!(),
        }
    }
}

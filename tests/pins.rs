use fanticon::{Bus, Cpu, Pins, Status};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cycle {
    Read(u16, u8),
    Write(u16, u8),
}

struct PinBus {
    ram: Box<[u8; 65536]>,
    pins: Pins,
    trace: Vec<Cycle>,
}

impl PinBus {
    fn new() -> Self {
        Self { ram: Box::new([0; 65536]), pins: Pins::default(), trace: Vec::new() }
    }
}

impl Bus for PinBus {
    fn read(&mut self, address: u16) -> u8 {
        let value = self.ram[address as usize];
        self.trace.push(Cycle::Read(address, value));
        value
    }

    fn write(&mut self, address: u16, value: u8) {
        self.trace.push(Cycle::Write(address, value));
        self.ram[address as usize] = value;
    }

    fn pins(&self) -> Pins {
        self.pins
    }
}

#[test]
fn reset_is_seven_read_cycles_and_decrements_stack() {
    let mut bus = PinBus::new();
    bus.ram[0xfffc] = 0x00;
    bus.ram[0xfffd] = 0x80;
    let mut cpu = Cpu::default();
    cpu.pc = 0x1234;
    cpu.status = Status(Status::UNUSED);
    cpu.request_reset();

    assert_eq!(cpu.step(&mut bus), 7);
    assert_eq!(cpu.pc, 0x8000);
    assert_eq!(cpu.sp, 0xfa);
    assert_ne!(cpu.status.0 & Status::INTERRUPT_DISABLE, 0);
    assert_eq!(
        bus.trace,
        [
            Cycle::Read(0x1234, 0),
            Cycle::Read(0x1234, 0),
            Cycle::Read(0x01fd, 0),
            Cycle::Read(0x01fc, 0),
            Cycle::Read(0x01fb, 0),
            Cycle::Read(0xfffc, 0x00),
            Cycle::Read(0xfffd, 0x80),
        ]
    );
}

#[test]
fn reset_pin_starts_reset_and_sync_marks_only_opcode_fetches() {
    let mut bus = PinBus::new();
    bus.ram[0xfffc] = 0x00;
    bus.ram[0xfffd] = 0x80;
    bus.ram[0x8000] = 0xea;
    let mut cpu = Cpu::default();
    cpu.pc = 0x1111;

    bus.pins.reset = true;
    let first = cpu.clock(&mut bus);
    assert!(!first.sync);
    bus.pins.reset = false;
    for _ in 0..5 {
        assert!(!cpu.clock(&mut bus).instruction_complete);
    }
    assert!(cpu.clock(&mut bus).instruction_complete);
    let fetch = cpu.clock(&mut bus);
    assert!(fetch.sync);
    assert!(!fetch.instruction_complete);
    let execute = cpu.clock(&mut bus);
    assert!(!execute.sync);
    assert!(execute.instruction_complete);
}

#[test]
fn irq_has_exact_stack_and_vector_bus_sequence() {
    let mut bus = PinBus::new();
    bus.ram[0x2000] = 0xea;
    bus.ram[0xfffe] = 0x34;
    bus.ram[0xffff] = 0x12;
    bus.pins.irq = true;
    let mut cpu = Cpu::default();
    cpu.pc = 0x2000;
    cpu.status = Status(Status::UNUSED);

    assert_eq!(cpu.step(&mut bus), 2); // IRQ is polled on NOP's final cycle.
    bus.trace.clear();
    assert_eq!(cpu.step(&mut bus), 7);
    assert_eq!(cpu.pc, 0x1234);
    assert_eq!(cpu.sp, 0xfa);
    assert_eq!(bus.ram[0x01fd], 0x20);
    assert_eq!(bus.ram[0x01fc], 0x01);
    assert_eq!(bus.ram[0x01fb] & Status::BREAK, 0);
    assert_eq!(
        bus.trace,
        [
            Cycle::Read(0x2001, 0),
            Cycle::Read(0x2001, 0),
            Cycle::Write(0x01fd, 0x20),
            Cycle::Write(0x01fc, 0x01),
            Cycle::Write(0x01fb, Status::UNUSED),
            Cycle::Read(0xfffe, 0x34),
            Cycle::Read(0xffff, 0x12),
        ]
    );
}

#[test]
fn nmi_is_edge_latched_even_when_interrupts_are_disabled() {
    let mut bus = PinBus::new();
    bus.ram[0x3000] = 0xea;
    bus.ram[0xfffa] = 0x78;
    bus.ram[0xfffb] = 0x56;
    let mut cpu = Cpu::default();
    cpu.pc = 0x3000;

    cpu.clock(&mut bus); // NOP opcode fetch.
    bus.pins.nmi = true;
    assert!(cpu.clock(&mut bus).instruction_complete);
    bus.pins.nmi = false;
    bus.trace.clear();
    assert_eq!(cpu.step(&mut bus), 7);
    assert_eq!(cpu.pc, 0x5678);
    assert_eq!(bus.trace[5], Cycle::Read(0xfffa, 0x78));
    assert_eq!(bus.trace[6], Cycle::Read(0xfffb, 0x56));
}

#[test]
fn nmi_level_does_not_retrigger_without_a_new_edge() {
    let mut bus = PinBus::new();
    bus.ram[0x3000] = 0xea;
    bus.ram[0x9000] = 0xea;
    bus.ram[0xfffa] = 0x00;
    bus.ram[0xfffb] = 0x90;
    let mut cpu = Cpu::default();
    cpu.pc = 0x3000;

    cpu.clock(&mut bus);
    bus.pins.nmi = true;
    cpu.clock(&mut bus);
    assert_eq!(cpu.step(&mut bus), 7);
    assert_eq!(cpu.pc, 0x9000);
    assert_eq!(cpu.step(&mut bus), 2);
    assert_eq!(cpu.pc, 0x9001);
}

#[test]
fn irq_polling_uses_interrupt_disable_state_before_final_cycle() {
    let mut bus = PinBus::new();
    bus.ram[0x8000] = 0x58; // CLI
    bus.ram[0x8001] = 0xea; // NOP
    bus.ram[0x8100] = 0x78; // SEI
    bus.ram[0xfffe] = 0x00;
    bus.ram[0xffff] = 0x90;
    bus.pins.irq = true;

    let mut cli = Cpu::default();
    cli.pc = 0x8000;
    cli.status = Status(Status::UNUSED | Status::INTERRUPT_DISABLE);
    assert_eq!(cli.step(&mut bus), 2);
    assert_eq!(cli.step(&mut bus), 2); // CLI delays recognition through one instruction.
    assert_eq!(cli.pc, 0x8002);
    assert_eq!(cli.step(&mut bus), 7);
    assert_eq!(cli.pc, 0x9000);

    let mut sei = Cpu::default();
    sei.pc = 0x8100;
    sei.status = Status(Status::UNUSED);
    assert_eq!(sei.step(&mut bus), 2);
    assert_ne!(sei.status.0 & Status::INTERRUPT_DISABLE, 0);
    assert_eq!(sei.step(&mut bus), 7); // Pending IRQ still wins immediately after SEI.
    assert_eq!(sei.pc, 0x9000);
}

#[test]
fn rdy_repeats_reads_without_advancing_cpu_state() {
    let mut bus = PinBus::new();
    bus.ram[0x8000] = 0xea;
    bus.pins.ready = false;
    let mut cpu = Cpu::default();
    cpu.pc = 0x8000;

    assert!(!cpu.clock(&mut bus).instruction_complete);
    assert!(!cpu.clock(&mut bus).instruction_complete);
    assert_eq!(cpu.pc, 0x8000);
    bus.pins.ready = true;
    cpu.clock(&mut bus);
    assert_eq!(cpu.pc, 0x8001);
    assert!(cpu.clock(&mut bus).instruction_complete);
    assert_eq!(cpu.cycles, 4);
    assert_eq!(bus.trace[0..3], [Cycle::Read(0x8000, 0xea); 3]);
}

#[test]
fn rdy_does_not_stall_write_cycles() {
    let mut bus = PinBus::new();
    bus.ram[0x8000] = 0x48; // PHA
    let mut cpu = Cpu::default();
    cpu.pc = 0x8000;
    cpu.a = 0xa5;

    cpu.clock(&mut bus);
    cpu.clock(&mut bus);
    bus.pins.ready = false;
    assert!(cpu.clock(&mut bus).instruction_complete);
    assert_eq!(bus.ram[0x01fd], 0xa5);
}

#[test]
fn so_sets_overflow_only_on_assertion_edge() {
    let mut bus = PinBus::new();
    bus.ram[0x8000] = 0xea;
    bus.ram[0x8001] = 0xea;
    let mut cpu = Cpu::default();
    cpu.pc = 0x8000;

    bus.pins.set_overflow = true;
    cpu.clock(&mut bus);
    assert_ne!(cpu.status.0 & Status::OVERFLOW, 0);
    cpu.status.0 &= !Status::OVERFLOW;
    cpu.clock(&mut bus);
    assert_eq!(cpu.status.0 & Status::OVERFLOW, 0);
    bus.pins.set_overflow = false;
    cpu.clock(&mut bus);
    bus.pins.set_overflow = true;
    cpu.clock(&mut bus);
    assert_ne!(cpu.status.0 & Status::OVERFLOW, 0);
}

#[test]
fn kil_jams_forever_until_reset() {
    let mut bus = PinBus::new();
    bus.ram[0x4000] = 0x02;
    bus.ram[0xfffc] = 0x00;
    bus.ram[0xfffd] = 0x80;
    let mut cpu = Cpu::default();
    cpu.pc = 0x4000;

    assert_eq!(cpu.step(&mut bus), 11);
    assert!(cpu.jammed());
    let pc = cpu.pc;
    bus.trace.clear();
    for _ in 0..4 {
        cpu.clock(&mut bus);
    }
    assert_eq!(cpu.pc, pc);
    assert_eq!(bus.trace, [Cycle::Read(0xffff, 0); 4]);

    cpu.request_reset();
    assert_eq!(cpu.step(&mut bus), 7);
    assert!(!cpu.jammed());
    assert_eq!(cpu.pc, 0x8000);
}

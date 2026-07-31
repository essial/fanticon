use fanticon::{Cpu, Ram, Status};

#[test]
fn lda_adc_and_store_program() {
    let mut ram = Ram::new();
    ram.0[0x8000..0x8007].copy_from_slice(&[0xa9, 0x7f, 0x69, 0x01, 0x85, 0x10, 0xea]);
    let mut cpu = Cpu::default();
    cpu.pc = 0x8000;
    assert_eq!(cpu.step(&mut ram), 2);
    assert_eq!(cpu.a, 0x7f);
    assert_eq!(cpu.step(&mut ram), 2);
    assert_eq!(cpu.a, 0x80);
    assert_ne!(cpu.status.0 & Status::OVERFLOW, 0);
    assert_eq!(cpu.step(&mut ram), 3);
    assert_eq!(ram.0[0x10], 0x80);
}

#[test]
fn indexed_read_page_cross_costs_a_cycle() {
    let mut ram = Ram::new();
    ram.0[0x8000..0x8003].copy_from_slice(&[0xbd, 0xff, 0x20]);
    ram.0[0x2100] = 0x42;
    let mut cpu = Cpu::default();
    cpu.pc = 0x8000;
    cpu.x = 1;
    assert_eq!(cpu.step(&mut ram), 5);
    assert_eq!(cpu.a, 0x42);
}

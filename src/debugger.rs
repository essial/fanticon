//! Deterministic debugger control for a paused Fanticon machine.

use std::collections::BTreeSet;

use crate::system::{BusAccessKind, FanticonMachine};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreakReason {
    Instruction(u16),
    MemoryRead(u16),
    MemoryWrite(u16),
    Raster { dot: u16, line: u16 },
    Jammed,
}

pub struct Debugger {
    pub machine: FanticonMachine,
    instruction_breakpoints: BTreeSet<u16>,
    read_watchpoints: BTreeSet<u16>,
    write_watchpoints: BTreeSet<u16>,
    raster_breakpoints: BTreeSet<(u16, u16)>,
    paused: bool,
    reason: Option<BreakReason>,
}

impl Debugger {
    pub fn new(machine: FanticonMachine) -> Self {
        Self {
            machine,
            instruction_breakpoints: BTreeSet::new(),
            read_watchpoints: BTreeSet::new(),
            write_watchpoints: BTreeSet::new(),
            raster_breakpoints: BTreeSet::new(),
            paused: true,
            reason: None,
        }
    }

    pub const fn paused(&self) -> bool {
        self.paused
    }
    pub const fn reason(&self) -> Option<BreakReason> {
        self.reason
    }
    pub fn pause(&mut self) {
        self.paused = true;
        self.reason = None;
    }
    pub fn resume(&mut self) {
        self.paused = false;
        self.reason = None;
    }
    pub fn add_instruction_breakpoint(&mut self, address: u16) {
        self.instruction_breakpoints.insert(address);
    }
    pub fn add_read_watchpoint(&mut self, address: u16) {
        self.read_watchpoints.insert(address);
    }
    pub fn add_write_watchpoint(&mut self, address: u16) {
        self.write_watchpoints.insert(address);
    }
    pub fn add_raster_breakpoint(&mut self, dot: u16, line: u16) {
        self.raster_breakpoints.insert((dot, line));
    }
    pub fn clear_breakpoints(&mut self) {
        self.instruction_breakpoints.clear();
        self.read_watchpoints.clear();
        self.write_watchpoints.clear();
        self.raster_breakpoints.clear();
        self.reason = None;
    }

    pub fn step_cycle(&mut self) {
        self.paused = true;
        self.reason = None;
        self.machine.cpu.clock(&mut self.machine.bus);
    }

    pub fn step_instruction(&mut self) -> u8 {
        self.paused = true;
        self.reason = None;
        self.machine.cpu.step(&mut self.machine.bus)
    }

    pub fn run_cycles(&mut self, maximum: u64) -> Option<BreakReason> {
        self.paused = false;
        self.reason = None;
        for _ in 0..maximum {
            if self.instruction_breakpoints.contains(&self.machine.cpu.pc) {
                return self.stop(BreakReason::Instruction(self.machine.cpu.pc));
            }
            let (dot, line) = self.machine.bus.raster_position();
            if self.raster_breakpoints.contains(&(dot, line)) {
                return self.stop(BreakReason::Raster { dot, line });
            }
            let result = self.machine.cpu.clock(&mut self.machine.bus);
            if let Some((address, kind)) = self.machine.bus.last_access() {
                let watched = match kind {
                    BusAccessKind::Read => self
                        .read_watchpoints
                        .contains(&address)
                        .then_some(BreakReason::MemoryRead(address)),
                    BusAccessKind::Write => self
                        .write_watchpoints
                        .contains(&address)
                        .then_some(BreakReason::MemoryWrite(address)),
                };
                if let Some(reason) = watched {
                    return self.stop(reason);
                }
            }
            if result.jammed {
                return self.stop(BreakReason::Jammed);
            }
        }
        None
    }

    fn stop(&mut self, reason: BreakReason) -> Option<BreakReason> {
        self.paused = true;
        self.reason = Some(reason);
        Some(reason)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{cartridge::Cartridge, machine::BANK_SIZE};

    fn machine() -> FanticonMachine {
        let mut fixed = [0xff; BANK_SIZE];
        fixed[0x100..0x108].copy_from_slice(&[0xa9, 1, 0x85, 0x20, 0x4c, 0x04, 0xc1, 0xea]);
        fixed[0x3ffa..].copy_from_slice(&[0x00, 0xc1, 0x00, 0xc1, 0x00, 0xc1]);
        FanticonMachine::new(Cartridge::new("DEBUG", 1, 0, fixed, Vec::new()).unwrap(), None)
    }

    #[test]
    fn instruction_memory_and_raster_breakpoints_pause_without_advancing_further() {
        let mut debugger = Debugger::new(machine());
        debugger.add_write_watchpoint(0x20);
        debugger.resume();
        assert_eq!(debugger.run_cycles(30), Some(BreakReason::MemoryWrite(0x20)));
        assert!(debugger.paused());
        assert_eq!(debugger.machine.bus.peek(0x20), 1);
        debugger.clear_breakpoints();
        debugger.add_instruction_breakpoint(0xc104);
        debugger.resume();
        assert_eq!(debugger.run_cycles(20), Some(BreakReason::Instruction(0xc104)));
        let before = debugger.machine.cpu.cycles;
        assert_eq!(debugger.run_cycles(0), None);
        assert_eq!(debugger.machine.cpu.cycles, before);
    }
}

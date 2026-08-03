//! Deterministic debugger control for a paused Fanticon machine.

use std::collections::{BTreeSet, VecDeque};

use crate::{
    assembler::SymbolSection,
    machine::bank_kind,
    system::{ApuDebugState, BusAccessKind, FanticonMachine, VideoDebugState},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreakReason {
    Instruction(u16),
    MemoryRead(u16),
    MemoryWrite(u16),
    Raster { dot: u16, line: u16 },
    Jammed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugStop {
    Instruction(u16),
    Source { section: SymbolSection, address: u16 },
    MemoryRead(u16),
    MemoryWrite(u16),
    Raster { dot: u16, line: u16 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugSnapshot {
    pub pc: u16,
    pub instruction_boundary: bool,
    pub sp: u8,
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub status: u8,
    pub cycles: u64,
    pub bank_kind: u8,
    pub bank_number: u8,
    pub irq_pending: u8,
    pub irq_enable: u8,
    pub raster_dot: u16,
    pub raster_line: u16,
    pub audio_master: u8,
    pub audio_sample: u16,
    pub apu: ApuDebugState,
    pub stack: [u8; 16],
    pub memory_start: u16,
    pub memory: [u8; 16],
    pub instruction_bytes: [u8; 3],
    pub address_space: Box<[u8; 65_536]>,
    pub video: VideoDebugState,
    pub stops: Vec<DebugStop>,
    pub trace: Vec<TraceEntry>,
    pub reason: Option<BreakReason>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceEntry {
    pub section: Option<SymbolSection>,
    pub address: u16,
    pub bytes: [u8; 3],
}

pub struct Debugger {
    pub machine: FanticonMachine,
    instruction_breakpoints: BTreeSet<u16>,
    source_breakpoints: BTreeSet<(SymbolSection, u16)>,
    read_watchpoints: BTreeSet<u16>,
    write_watchpoints: BTreeSet<u16>,
    raster_breakpoints: BTreeSet<(u16, u16)>,
    paused: bool,
    reason: Option<BreakReason>,
    ignore_instruction_breakpoint_once: Option<(Option<SymbolSection>, u16)>,
    trace: VecDeque<TraceEntry>,
}

impl Debugger {
    pub fn new(machine: FanticonMachine) -> Self {
        Self {
            machine,
            instruction_breakpoints: BTreeSet::new(),
            source_breakpoints: BTreeSet::new(),
            read_watchpoints: BTreeSet::new(),
            write_watchpoints: BTreeSet::new(),
            raster_breakpoints: BTreeSet::new(),
            paused: true,
            reason: None,
            ignore_instruction_breakpoint_once: None,
            trace: VecDeque::with_capacity(8),
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
    pub fn remove_instruction_breakpoint(&mut self, address: u16) {
        self.instruction_breakpoints.remove(&address);
    }
    pub fn add_source_breakpoint(&mut self, section: SymbolSection, address: u16) {
        self.source_breakpoints.insert((section, address));
    }
    pub fn remove_source_breakpoint(&mut self, section: SymbolSection, address: u16) {
        self.source_breakpoints.remove(&(section, address));
    }
    pub fn source_breakpoints(&self) -> &BTreeSet<(SymbolSection, u16)> {
        &self.source_breakpoints
    }
    pub fn set_source_breakpoints(
        &mut self,
        breakpoints: impl IntoIterator<Item = (SymbolSection, u16)>,
    ) {
        self.source_breakpoints.clear();
        self.source_breakpoints.extend(breakpoints);
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
        self.source_breakpoints.clear();
        self.read_watchpoints.clear();
        self.write_watchpoints.clear();
        self.raster_breakpoints.clear();
        self.reason = None;
    }

    pub fn remove_stop(&mut self, stop: DebugStop) {
        match stop {
            DebugStop::Instruction(address) => self.remove_instruction_breakpoint(address),
            DebugStop::Source { section, address } => {
                self.remove_source_breakpoint(section, address)
            }
            DebugStop::MemoryRead(address) => {
                self.read_watchpoints.remove(&address);
            }
            DebugStop::MemoryWrite(address) => {
                self.write_watchpoints.remove(&address);
            }
            DebugStop::Raster { dot, line } => {
                self.raster_breakpoints.remove(&(dot, line));
            }
        }
    }

    pub fn write_memory(&mut self, address: u16, value: u8) -> Result<(), String> {
        self.machine.bus.debug_poke(address, value)
    }

    pub fn step_cycle(&mut self) {
        self.paused = true;
        self.reason = None;
        if self.machine.cpu.instruction_boundary() {
            self.record_trace();
        }
        self.machine.cpu.clock(&mut self.machine.bus);
    }

    pub fn step_instruction(&mut self) -> u8 {
        self.paused = true;
        self.reason = None;
        if self.machine.cpu.instruction_boundary() {
            self.record_trace();
        }
        self.machine.cpu.step(&mut self.machine.bus)
    }

    pub fn step_over(&mut self, maximum_cycles: u64) -> Option<BreakReason> {
        if !self.machine.cpu.instruction_boundary() {
            self.step_instruction();
            return None;
        }
        let start_pc = self.machine.cpu.pc;
        let start_sp = self.machine.cpu.sp;
        if self.machine.bus.peek(start_pc) != 0x20 {
            self.step_instruction();
            return None;
        }
        let target = start_pc.wrapping_add(3);
        self.run_until(maximum_cycles, |machine| {
            machine.cpu.instruction_boundary()
                && machine.cpu.pc == target
                && machine.cpu.sp == start_sp
        })
    }

    pub fn step_out(&mut self, maximum_cycles: u64) -> Option<BreakReason> {
        if !self.machine.cpu.instruction_boundary() {
            self.step_instruction();
        }
        let sp = self.machine.cpu.sp;
        if sp > 0xfd {
            self.step_instruction();
            return None;
        }
        let low = self.machine.bus.peek(0x0101u16.wrapping_add(u16::from(sp)));
        let high = self.machine.bus.peek(0x0102u16.wrapping_add(u16::from(sp)));
        let target = u16::from_le_bytes([low, high]).wrapping_add(1);
        let target_sp = sp.wrapping_add(2);
        self.run_until(maximum_cycles, |machine| {
            machine.cpu.instruction_boundary()
                && machine.cpu.pc == target
                && machine.cpu.sp == target_sp
        })
    }

    pub fn run_cycles(&mut self, maximum: u64) -> Option<BreakReason> {
        self.paused = false;
        self.reason = None;
        for _ in 0..maximum {
            if self.machine.cpu.instruction_boundary() && self.instruction_breakpoint_hit() {
                return self.stop(BreakReason::Instruction(self.machine.cpu.pc));
            }
            let (dot, line) = self.machine.bus.raster_position();
            if self.raster_breakpoints.contains(&(dot, line)) {
                return self.stop(BreakReason::Raster { dot, line });
            }
            if self.machine.cpu.instruction_boundary() {
                self.record_trace();
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

    pub fn snapshot(&self) -> DebugSnapshot {
        let cpu = &self.machine.cpu;
        let bus = &self.machine.bus;
        let (raster_dot, raster_line) = bus.raster_position();
        let stack = core::array::from_fn(|offset| {
            bus.peek(0x0100 | u16::from(cpu.sp.wrapping_add(1 + offset as u8)))
        });
        let instruction_bytes =
            core::array::from_fn(|offset| bus.peek(cpu.pc.wrapping_add(offset as u16)));
        let memory_start = cpu.pc & 0xfff0;
        let memory = core::array::from_fn(|offset| bus.peek(memory_start + offset as u16));
        let address_space = Box::new(core::array::from_fn(|address| bus.peek(address as u16)));
        let mut stops = Vec::new();
        stops.extend(self.instruction_breakpoints.iter().copied().map(DebugStop::Instruction));
        stops.extend(
            self.source_breakpoints
                .iter()
                .copied()
                .map(|(section, address)| DebugStop::Source { section, address }),
        );
        stops.extend(self.read_watchpoints.iter().copied().map(DebugStop::MemoryRead));
        stops.extend(self.write_watchpoints.iter().copied().map(DebugStop::MemoryWrite));
        stops.extend(
            self.raster_breakpoints
                .iter()
                .copied()
                .map(|(dot, line)| DebugStop::Raster { dot, line }),
        );
        DebugSnapshot {
            pc: cpu.pc,
            instruction_boundary: cpu.instruction_boundary(),
            sp: cpu.sp,
            a: cpu.a,
            x: cpu.x,
            y: cpu.y,
            status: cpu.status.0,
            cycles: cpu.cycles,
            bank_kind: bus.bank_kind(),
            bank_number: bus.bank_number(),
            irq_pending: bus.irq_pending(),
            irq_enable: bus.irq_enable(),
            raster_dot,
            raster_line,
            audio_master: bus.audio_master(),
            audio_sample: bus.current_audio_sample(),
            apu: bus.apu_debug_state(),
            stack,
            memory_start,
            memory,
            instruction_bytes,
            address_space,
            video: bus.video_debug_state(),
            stops,
            trace: self.trace.iter().cloned().collect(),
            reason: self.reason,
        }
    }

    fn run_until(
        &mut self,
        maximum: u64,
        predicate: impl Fn(&FanticonMachine) -> bool,
    ) -> Option<BreakReason> {
        self.paused = false;
        self.reason = None;
        self.ignore_instruction_breakpoint_once =
            Some((self.execution_section(), self.machine.cpu.pc));
        for _ in 0..maximum {
            if predicate(&self.machine) {
                self.paused = true;
                return None;
            }
            if self.machine.cpu.instruction_boundary() && self.instruction_breakpoint_hit() {
                return self.stop(BreakReason::Instruction(self.machine.cpu.pc));
            }
            let (dot, line) = self.machine.bus.raster_position();
            if self.raster_breakpoints.contains(&(dot, line)) {
                return self.stop(BreakReason::Raster { dot, line });
            }
            if self.machine.cpu.instruction_boundary() {
                self.record_trace();
            }
            let result = self.machine.cpu.clock(&mut self.machine.bus);
            if let Some((address, kind)) = self.machine.bus.last_access() {
                let reason = match kind {
                    BusAccessKind::Read if self.read_watchpoints.contains(&address) => {
                        Some(BreakReason::MemoryRead(address))
                    }
                    BusAccessKind::Write if self.write_watchpoints.contains(&address) => {
                        Some(BreakReason::MemoryWrite(address))
                    }
                    _ => None,
                };
                if let Some(reason) = reason {
                    return self.stop(reason);
                }
            }
            if result.jammed {
                return self.stop(BreakReason::Jammed);
            }
        }
        self.paused = true;
        None
    }

    fn instruction_breakpoint_hit(&mut self) -> bool {
        let current = (self.execution_section(), self.machine.cpu.pc);
        if self.ignore_instruction_breakpoint_once == Some(current) {
            self.ignore_instruction_breakpoint_once = None;
            return false;
        }
        self.instruction_breakpoints.contains(&current.1)
            || current
                .0
                .is_some_and(|section| self.source_breakpoints.contains(&(section, current.1)))
    }

    fn execution_section(&self) -> Option<SymbolSection> {
        match self.machine.cpu.pc {
            0x8000..=0xbfff if self.machine.bus.bank_kind() == bank_kind::CARTRIDGE_ROM => {
                Some(SymbolSection::Bank(self.machine.bus.bank_number()))
            }
            0xc100..=0xffff => Some(SymbolSection::Fixed),
            _ => None,
        }
    }

    fn record_trace(&mut self) {
        let address = self.machine.cpu.pc;
        let entry = TraceEntry {
            section: self.execution_section(),
            address,
            bytes: core::array::from_fn(|offset| {
                self.machine.bus.peek(address.wrapping_add(offset as u16))
            }),
        };
        if self.trace.back() == Some(&entry) {
            return;
        }
        if self.trace.len() == 8 {
            self.trace.pop_front();
        }
        self.trace.push_back(entry);
    }

    fn stop(&mut self, reason: BreakReason) -> Option<BreakReason> {
        self.paused = true;
        self.reason = Some(reason);
        if matches!(reason, BreakReason::Instruction(_)) {
            self.ignore_instruction_breakpoint_once =
                Some((self.execution_section(), self.machine.cpu.pc));
        }
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

    fn call_machine() -> FanticonMachine {
        let mut fixed = [0xff; BANK_SIZE];
        fixed[0x100..0x10b].copy_from_slice(&[
            0x20, 0x08, 0xc1, // JSR $C108
            0xa9, 0x42, // LDA #$42
            0xea, 0xea, 0xea, // padding
            0xa2, 0x11, // LDX #$11
            0x60, // RTS
        ]);
        fixed[0x3ffa..].copy_from_slice(&[0x00, 0xc1, 0x00, 0xc1, 0x00, 0xc1]);
        FanticonMachine::new(Cartridge::new("CALL", 2, 0, fixed, Vec::new()).unwrap(), None)
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

    #[test]
    fn breakpoints_stop_only_at_boundaries_and_continue_past_the_current_stop() {
        let mut debugger = Debugger::new(machine());
        debugger.add_source_breakpoint(SymbolSection::Fixed, 0xc102);
        debugger.resume();

        assert_eq!(debugger.run_cycles(30), Some(BreakReason::Instruction(0xc102)));
        assert_eq!(debugger.machine.cpu.a, 1);
        let stopped_cycles = debugger.machine.cpu.cycles;

        debugger.resume();
        assert_eq!(debugger.run_cycles(1), None);
        assert!(debugger.machine.cpu.cycles > stopped_cycles);
    }

    #[test]
    fn step_over_and_step_out_follow_real_jsr_stack_semantics() {
        let mut debugger = Debugger::new(call_machine());
        debugger.step_instruction(); // reset
        assert_eq!(debugger.machine.cpu.pc, 0xc100);

        assert_eq!(debugger.step_over(1_000), None);
        assert_eq!(debugger.machine.cpu.pc, 0xc103);
        assert_eq!(debugger.machine.cpu.x, 0x11);

        debugger.machine.cpu.pc = 0xc100;
        debugger.machine.cpu.sp = 0xff;
        debugger.step_instruction(); // JSR
        assert_eq!(debugger.machine.cpu.pc, 0xc108);
        assert_eq!(debugger.step_out(1_000), None);
        assert_eq!(debugger.machine.cpu.pc, 0xc103);
        assert_eq!(debugger.machine.cpu.sp, 0xff);
    }

    #[test]
    fn instruction_steps_from_a_mid_cycle_pause_finish_the_current_opcode() {
        let mut debugger = Debugger::new(machine());
        debugger.step_instruction(); // reset
        debugger.step_cycle(); // opcode fetch for LDA #$01
        assert!(!debugger.machine.cpu.instruction_boundary());

        assert_eq!(debugger.step_over(1_000), None);
        assert_eq!(debugger.machine.cpu.pc, 0xc102);
        assert_eq!(debugger.machine.cpu.a, 1);
        assert_eq!(debugger.snapshot().trace.last().unwrap().address, 0xc100);
    }

    #[test]
    fn snapshots_include_banks_stack_memory_apu_and_instruction_trace() {
        let mut debugger = Debugger::new(machine());
        debugger.step_instruction();
        debugger.step_instruction();
        let snapshot = debugger.snapshot();

        assert_eq!(snapshot.pc, 0xc102);
        assert_eq!(snapshot.bank_kind, bank_kind::CARTRIDGE_ROM);
        assert_eq!(snapshot.memory_start, 0xc100);
        assert_eq!(snapshot.memory[0..3], [0xa9, 1, 0x85]);
        assert_eq!(snapshot.address_space[0xc100..0xc103], [0xa9, 1, 0x85]);
        assert_eq!(snapshot.video.video_ram.len(), crate::machine::VIDEO_RAM_SIZE);
        assert!(!snapshot.trace.is_empty());
        assert_eq!(snapshot.apu.master, 0);
    }

    #[test]
    fn snapshots_list_managed_stops_and_the_debugger_can_remove_them() {
        let mut debugger = Debugger::new(machine());
        debugger.add_instruction_breakpoint(0xc100);
        debugger.add_source_breakpoint(SymbolSection::Fixed, 0xc102);
        debugger.add_read_watchpoint(0x20);
        debugger.add_write_watchpoint(0x21);
        debugger.add_raster_breakpoint(12, 34);

        let stops = debugger.snapshot().stops;
        assert!(stops.contains(&DebugStop::Instruction(0xc100)));
        assert!(
            stops.contains(&DebugStop::Source { section: SymbolSection::Fixed, address: 0xc102 })
        );
        assert!(stops.contains(&DebugStop::MemoryRead(0x20)));
        assert!(stops.contains(&DebugStop::MemoryWrite(0x21)));
        assert!(stops.contains(&DebugStop::Raster { dot: 12, line: 34 }));

        for stop in stops {
            debugger.remove_stop(stop);
        }
        assert!(debugger.snapshot().stops.is_empty());
    }

    #[test]
    fn paused_memory_edits_change_ram_and_reject_rom() {
        let mut debugger = Debugger::new(machine());
        debugger.write_memory(0x2345, 0xa5).unwrap();
        assert_eq!(debugger.snapshot().address_space[0x2345], 0xa5);
        assert!(debugger.write_memory(0xc100, 0).is_err());
        assert_eq!(debugger.machine.bus.peek(0xc100), 0xa9);
    }
}

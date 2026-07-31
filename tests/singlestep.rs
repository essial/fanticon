use fanticon::{Bus, Cpu, Status};
use serde::Deserialize;
use std::{env, fs, path::PathBuf, time::Instant};

#[derive(Deserialize)]
struct Case {
    name: String,
    initial: State,
    #[serde(rename = "final")]
    expected: State,
    cycles: Vec<(u16, u8, Access)>,
}

#[derive(Deserialize)]
struct State {
    pc: u16,
    s: u8,
    a: u8,
    x: u8,
    y: u8,
    p: u8,
    ram: Vec<(u16, u8)>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Access {
    Read,
    Write,
}

struct TraceBus {
    ram: Box<[u8; 65536]>,
    trace: Vec<(u16, u8, Access)>,
}

impl TraceBus {
    fn new() -> Self {
        Self { ram: Box::new([0; 65536]), trace: Vec::with_capacity(16) }
    }
}

impl Bus for TraceBus {
    fn read(&mut self, address: u16) -> u8 {
        let value = self.ram[address as usize];
        self.trace.push((address, value, Access::Read));
        value
    }

    fn write(&mut self, address: u16, value: u8) {
        self.trace.push((address, value, Access::Write));
        self.ram[address as usize] = value;
    }
}

fn run_file(path: &PathBuf, limit: usize) -> usize {
    let contents = fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let cases: Vec<Case> =
        serde_json::from_slice(&contents).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut bus = TraceBus::new();

    for case in cases.iter().take(limit) {
        for &(address, value) in &case.initial.ram {
            bus.ram[address as usize] = value;
        }
        bus.trace.clear();
        let mut cpu = Cpu::default();
        cpu.pc = case.initial.pc;
        cpu.sp = case.initial.s;
        cpu.a = case.initial.a;
        cpu.x = case.initial.x;
        cpu.y = case.initial.y;
        cpu.status = Status(case.initial.p);
        cpu.cycles = 0;
        cpu.step(&mut bus);

        assert_eq!(bus.trace, case.cycles, "{} bus trace", case.name);
        assert_eq!(cpu.pc, case.expected.pc, "{} pc", case.name);
        assert_eq!(cpu.sp, case.expected.s, "{} s", case.name);
        assert_eq!(cpu.a, case.expected.a, "{} a", case.name);
        assert_eq!(cpu.x, case.expected.x, "{} x", case.name);
        assert_eq!(cpu.y, case.expected.y, "{} y", case.name);
        assert_eq!(cpu.status.0, case.expected.p, "{} p", case.name);
        for &(address, value) in &case.expected.ram {
            assert_eq!(bus.ram[address as usize], value, "{} ram[{address:#06x}]", case.name);
        }
    }
    cases.len().min(limit)
}

/// Uses the checked-out SingleStepTests submodule automatically. An explicit
/// `SINGLESTEP_6502_DIR` can still override it for development.
fn fixture_root() -> PathBuf {
    let root = env::var_os("SINGLESTEP_6502_DIR").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/SingleStepTests/6502/v1")
    });
    assert!(
        root.join("00.json").is_file(),
        "SingleStepTests fixtures are missing at {}. Run: git submodule update --init --depth 1",
        root.display()
    );
    root
}

fn case_limit() -> usize {
    env::var("SINGLESTEP_LIMIT").ok().and_then(|v| v.parse().ok()).unwrap_or(usize::MAX)
}

macro_rules! opcode_tests {
    ($($name:ident: $opcode:literal),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                let started = Instant::now();
                let count = run_file(
                    &fixture_root().join(concat!($opcode, ".json")),
                    case_limit(),
                );
                eprintln!(
                    "opcode {}: passed {count} cases in {:.2?}",
                    $opcode,
                    started.elapsed()
                );
            }
        )+
    };
}

opcode_tests! {
    opcode_00: "00",
    opcode_01: "01",
    opcode_02: "02",
    opcode_03: "03",
    opcode_04: "04",
    opcode_05: "05",
    opcode_06: "06",
    opcode_07: "07",
    opcode_08: "08",
    opcode_09: "09",
    opcode_0a: "0a",
    opcode_0b: "0b",
    opcode_0c: "0c",
    opcode_0d: "0d",
    opcode_0e: "0e",
    opcode_0f: "0f",
    opcode_10: "10",
    opcode_11: "11",
    opcode_12: "12",
    opcode_13: "13",
    opcode_14: "14",
    opcode_15: "15",
    opcode_16: "16",
    opcode_17: "17",
    opcode_18: "18",
    opcode_19: "19",
    opcode_1a: "1a",
    opcode_1b: "1b",
    opcode_1c: "1c",
    opcode_1d: "1d",
    opcode_1e: "1e",
    opcode_1f: "1f",
    opcode_20: "20",
    opcode_21: "21",
    opcode_22: "22",
    opcode_23: "23",
    opcode_24: "24",
    opcode_25: "25",
    opcode_26: "26",
    opcode_27: "27",
    opcode_28: "28",
    opcode_29: "29",
    opcode_2a: "2a",
    opcode_2b: "2b",
    opcode_2c: "2c",
    opcode_2d: "2d",
    opcode_2e: "2e",
    opcode_2f: "2f",
    opcode_30: "30",
    opcode_31: "31",
    opcode_32: "32",
    opcode_33: "33",
    opcode_34: "34",
    opcode_35: "35",
    opcode_36: "36",
    opcode_37: "37",
    opcode_38: "38",
    opcode_39: "39",
    opcode_3a: "3a",
    opcode_3b: "3b",
    opcode_3c: "3c",
    opcode_3d: "3d",
    opcode_3e: "3e",
    opcode_3f: "3f",
    opcode_40: "40",
    opcode_41: "41",
    opcode_42: "42",
    opcode_43: "43",
    opcode_44: "44",
    opcode_45: "45",
    opcode_46: "46",
    opcode_47: "47",
    opcode_48: "48",
    opcode_49: "49",
    opcode_4a: "4a",
    opcode_4b: "4b",
    opcode_4c: "4c",
    opcode_4d: "4d",
    opcode_4e: "4e",
    opcode_4f: "4f",
    opcode_50: "50",
    opcode_51: "51",
    opcode_52: "52",
    opcode_53: "53",
    opcode_54: "54",
    opcode_55: "55",
    opcode_56: "56",
    opcode_57: "57",
    opcode_58: "58",
    opcode_59: "59",
    opcode_5a: "5a",
    opcode_5b: "5b",
    opcode_5c: "5c",
    opcode_5d: "5d",
    opcode_5e: "5e",
    opcode_5f: "5f",
    opcode_60: "60",
    opcode_61: "61",
    opcode_62: "62",
    opcode_63: "63",
    opcode_64: "64",
    opcode_65: "65",
    opcode_66: "66",
    opcode_67: "67",
    opcode_68: "68",
    opcode_69: "69",
    opcode_6a: "6a",
    opcode_6b: "6b",
    opcode_6c: "6c",
    opcode_6d: "6d",
    opcode_6e: "6e",
    opcode_6f: "6f",
    opcode_70: "70",
    opcode_71: "71",
    opcode_72: "72",
    opcode_73: "73",
    opcode_74: "74",
    opcode_75: "75",
    opcode_76: "76",
    opcode_77: "77",
    opcode_78: "78",
    opcode_79: "79",
    opcode_7a: "7a",
    opcode_7b: "7b",
    opcode_7c: "7c",
    opcode_7d: "7d",
    opcode_7e: "7e",
    opcode_7f: "7f",
    opcode_80: "80",
    opcode_81: "81",
    opcode_82: "82",
    opcode_83: "83",
    opcode_84: "84",
    opcode_85: "85",
    opcode_86: "86",
    opcode_87: "87",
    opcode_88: "88",
    opcode_89: "89",
    opcode_8a: "8a",
    opcode_8b: "8b",
    opcode_8c: "8c",
    opcode_8d: "8d",
    opcode_8e: "8e",
    opcode_8f: "8f",
    opcode_90: "90",
    opcode_91: "91",
    opcode_92: "92",
    opcode_93: "93",
    opcode_94: "94",
    opcode_95: "95",
    opcode_96: "96",
    opcode_97: "97",
    opcode_98: "98",
    opcode_99: "99",
    opcode_9a: "9a",
    opcode_9b: "9b",
    opcode_9c: "9c",
    opcode_9d: "9d",
    opcode_9e: "9e",
    opcode_9f: "9f",
    opcode_a0: "a0",
    opcode_a1: "a1",
    opcode_a2: "a2",
    opcode_a3: "a3",
    opcode_a4: "a4",
    opcode_a5: "a5",
    opcode_a6: "a6",
    opcode_a7: "a7",
    opcode_a8: "a8",
    opcode_a9: "a9",
    opcode_aa: "aa",
    opcode_ab: "ab",
    opcode_ac: "ac",
    opcode_ad: "ad",
    opcode_ae: "ae",
    opcode_af: "af",
    opcode_b0: "b0",
    opcode_b1: "b1",
    opcode_b2: "b2",
    opcode_b3: "b3",
    opcode_b4: "b4",
    opcode_b5: "b5",
    opcode_b6: "b6",
    opcode_b7: "b7",
    opcode_b8: "b8",
    opcode_b9: "b9",
    opcode_ba: "ba",
    opcode_bb: "bb",
    opcode_bc: "bc",
    opcode_bd: "bd",
    opcode_be: "be",
    opcode_bf: "bf",
    opcode_c0: "c0",
    opcode_c1: "c1",
    opcode_c2: "c2",
    opcode_c3: "c3",
    opcode_c4: "c4",
    opcode_c5: "c5",
    opcode_c6: "c6",
    opcode_c7: "c7",
    opcode_c8: "c8",
    opcode_c9: "c9",
    opcode_ca: "ca",
    opcode_cb: "cb",
    opcode_cc: "cc",
    opcode_cd: "cd",
    opcode_ce: "ce",
    opcode_cf: "cf",
    opcode_d0: "d0",
    opcode_d1: "d1",
    opcode_d2: "d2",
    opcode_d3: "d3",
    opcode_d4: "d4",
    opcode_d5: "d5",
    opcode_d6: "d6",
    opcode_d7: "d7",
    opcode_d8: "d8",
    opcode_d9: "d9",
    opcode_da: "da",
    opcode_db: "db",
    opcode_dc: "dc",
    opcode_dd: "dd",
    opcode_de: "de",
    opcode_df: "df",
    opcode_e0: "e0",
    opcode_e1: "e1",
    opcode_e2: "e2",
    opcode_e3: "e3",
    opcode_e4: "e4",
    opcode_e5: "e5",
    opcode_e6: "e6",
    opcode_e7: "e7",
    opcode_e8: "e8",
    opcode_e9: "e9",
    opcode_ea: "ea",
    opcode_eb: "eb",
    opcode_ec: "ec",
    opcode_ed: "ed",
    opcode_ee: "ee",
    opcode_ef: "ef",
    opcode_f0: "f0",
    opcode_f1: "f1",
    opcode_f2: "f2",
    opcode_f3: "f3",
    opcode_f4: "f4",
    opcode_f5: "f5",
    opcode_f6: "f6",
    opcode_f7: "f7",
    opcode_f8: "f8",
    opcode_f9: "f9",
    opcode_fa: "fa",
    opcode_fb: "fb",
    opcode_fc: "fc",
    opcode_fd: "fd",
    opcode_fe: "fe",
    opcode_ff: "ff",
}

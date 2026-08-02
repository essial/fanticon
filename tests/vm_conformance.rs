use std::path::{Path, PathBuf};

use fanticon::{project::build_project_with_loader, system::FanticonMachine, video::Video};

const DEMOS: [(&str, u64); 7] = [
    ("audio", 0xcdba_2792_82b0_2f8c),
    ("bitmap", 0x16a9_1eb2_9ae1_e086),
    ("graphics", 0x9631_20a2_45c2_069d),
    ("raster", 0xf4bc_2c90_7eff_04c5),
    ("sprites", 0x3ff7_98c4_eb46_d6f1),
    ("tiles", 0x6428_1b7b_5efd_639e),
    ("wave", 0x38bd_759c_ea2d_d1ed),
];

#[test]
fn demo_cartridges_match_whole_machine_golden_states() {
    for (name, expected) in DEMOS {
        let mut machine = build_demo(name);
        for _ in 0..8 {
            machine.run_frame();
        }
        let actual = machine_hash(&machine);
        assert_eq!(actual, expected, "{name} VM state changed; actual hash is {actual:#018x}");
    }
}

fn build_demo(name: &str) -> FanticonMachine {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("code-assets/demos").join(name);
    let manifest = std::fs::read_to_string(directory.join("fanticon.cfg")).unwrap();
    let build = build_project_with_loader(&manifest, |path| read_source(&directory, path))
        .unwrap_or_else(|diagnostics| panic!("{name} failed to assemble: {diagnostics:?}"));
    FanticonMachine::new(build.cartridge, None)
}

fn read_source(directory: &Path, path: &str) -> Result<String, String> {
    std::fs::read_to_string(directory.join(path.to_ascii_lowercase()))
        .map_err(|error| error.to_string())
}

fn machine_hash(machine: &FanticonMachine) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    let mut add = |byte| {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    };

    for value in [machine.cpu.a, machine.cpu.x, machine.cpu.y, machine.cpu.sp, machine.cpu.status.0]
    {
        add(value);
    }
    for byte in machine.cpu.pc.to_le_bytes().into_iter().chain(machine.cpu.cycles.to_le_bytes()) {
        add(byte);
    }
    for address in 0..=0x7fff {
        add(machine.bus.peek(address));
    }
    let mut video = Video::new();
    machine.bus.present(&mut video);
    for &pixel in video.pixels() {
        add(pixel);
    }
    for color in video.palette() {
        for &component in color {
            add(component);
        }
    }
    for &sample in machine.bus.audio_frame() {
        for byte in sample.to_le_bytes() {
            add(byte);
        }
    }
    hash
}

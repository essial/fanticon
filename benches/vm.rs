use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use fanticon::{
    machine::CPU_CYCLES_PER_FRAME, project::build_project_with_loader, system::FanticonMachine,
};

const WARMUP_FRAMES: usize = 30;
const MEASURE_FRAMES: usize = 180;
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

struct CountingAllocator;

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(pointer, layout, size) }
    }
}

fn main() {
    println!("Fanticon whole-machine benchmark ({MEASURE_FRAMES} measured frames)");
    println!("workload       frames/s      cycles/s   real-time   alloc/frame");
    for name in ["audio", "bitmap", "raster", "sprites", "tiles", "wave"] {
        let mut machine = build_demo(name);
        for _ in 0..WARMUP_FRAMES {
            machine.run_frame();
        }

        ALLOCATIONS.store(0, Ordering::Relaxed);
        let start = Instant::now();
        for _ in 0..MEASURE_FRAMES {
            machine.run_frame();
            black_box(machine.bus.current_audio_sample());
        }
        let elapsed = start.elapsed().as_secs_f64();
        let allocations = ALLOCATIONS.load(Ordering::Relaxed);
        let frames_per_second = MEASURE_FRAMES as f64 / elapsed;
        let cycles_per_second = frames_per_second * f64::from(CPU_CYCLES_PER_FRAME);
        println!(
            "{name:<10} {frames_per_second:>11.1} {cycles_per_second:>13.0} {multiple:>10.1}x {allocations_per_frame:>13.3}",
            multiple = frames_per_second / 60.0,
            allocations_per_frame = allocations as f64 / MEASURE_FRAMES as f64,
        );
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

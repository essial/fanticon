//! Cycle-driven Fanticon v0.1 mapped machine.

use crate::{
    Bus, Cpu, Pins,
    audio::{NOISE_PERIODS, PULSE_DUTY_TABLE, TRIANGLE_SEQUENCE, mix_sample, step_noise_lfsr},
    cartridge::Cartridge,
    machine::{
        BANK_SIZE, CPU_CYCLES_PER_FRAME, MAIN_RAM_SIZE, VIDEO_RAM_SIZE, WORK_RAM_BANKS, bank_kind,
        register,
    },
    video::{DISPLAY_HEIGHT, DISPLAY_WIDTH, DOTS_PER_FRAME, DOTS_PER_SCANLINE, Video},
};

const IRQ_VBLANK: u8 = 1;
const IRQ_RASTER: u8 = 2;
const IRQ_TIMER0: u8 = 4;
const IRQ_TIMER1: u8 = 8;
const TILE_PATTERNS: usize = 0x0000;
const TILE_MAP: usize = 0x2000;
const TILE_ATTRIBUTES: usize = 0x2400;
const SPRITE_TABLE: usize = 0x2800;
const BITMAP: usize = 0x4000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ControllerState(pub u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusAccessKind {
    Read,
    Write,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApuDebugState {
    pub pulse_control: [u8; 2],
    pub pulse_timer: [u16; 2],
    pub triangle_control: u8,
    pub triangle_timer: u16,
    pub noise_control: u8,
    pub noise_period: u8,
    pub master: u8,
    pub sample: u16,
}

impl ControllerState {
    pub const UP: u8 = 1 << 0;
    pub const DOWN: u8 = 1 << 1;
    pub const LEFT: u8 = 1 << 2;
    pub const RIGHT: u8 = 1 << 3;
    pub const A: u8 = 1 << 4;
    pub const B: u8 = 1 << 5;
    pub const SELECT: u8 = 1 << 6;
    pub const START: u8 = 1 << 7;
}

#[derive(Clone, Copy, Debug, Default)]
struct Controller {
    host: u8,
    state: u8,
    pressed: u8,
}

impl Controller {
    fn sample(&mut self) {
        self.pressed |= self.host & !self.state;
        self.state = self.host;
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Timer {
    reload: u16,
    count: u32,
    enabled: bool,
    automatic: bool,
    start_delay: bool,
    high_latch: Option<u8>,
}

impl Timer {
    fn period(&self) -> u32 {
        if self.reload == 0 { 65_536 } else { u32::from(self.reload) }
    }

    fn control(&self) -> u8 {
        u8::from(self.enabled) | (u8::from(self.automatic) << 1)
    }

    fn write_control(&mut self, value: u8) {
        let enable = value & 1 != 0;
        self.automatic = value & 2 != 0;
        if enable && !self.enabled {
            self.count = self.period();
            self.start_delay = true;
        }
        self.enabled = enable;
    }

    fn tick(&mut self) -> bool {
        if !self.enabled {
            return false;
        }
        if self.start_delay {
            self.start_delay = false;
            return false;
        }
        self.count -= 1;
        if self.count != 0 {
            return false;
        }
        if self.automatic {
            self.count = self.period();
        } else {
            self.enabled = false;
        }
        true
    }

    fn visible_count(&self) -> u16 {
        self.count as u16
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Pulse {
    control: u8,
    timer: u16,
    divider: u16,
    phase: u8,
}

impl Pulse {
    fn tick(&mut self, even_cycle: bool) {
        if !even_cycle {
            return;
        }
        if self.divider == 0 {
            self.phase = (self.phase + 1) & 7;
            self.divider = self.timer;
        } else {
            self.divider -= 1;
        }
    }

    fn level(&self) -> u8 {
        if self.control & 0x80 == 0 {
            return 0;
        }
        let duty = (self.control >> 5) & 3;
        PULSE_DUTY_TABLE[duty as usize][self.phase as usize] * (self.control & 0x0f)
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Triangle {
    control: u8,
    timer: u16,
    divider: u16,
    phase: u8,
}

impl Triangle {
    fn tick(&mut self) {
        if self.divider == 0 {
            self.phase = (self.phase + 1) & 31;
            self.divider = self.timer;
        } else {
            self.divider -= 1;
        }
    }

    fn level(&self) -> u8 {
        if self.control & 0x80 == 0 { 0 } else { TRIANGLE_SEQUENCE[self.phase as usize] }
    }
}

#[derive(Clone, Copy, Debug)]
struct Noise {
    control: u8,
    period: u8,
    divider: u16,
    lfsr: u16,
}

impl Default for Noise {
    fn default() -> Self {
        Self { control: 0, period: 0, divider: 0, lfsr: 1 }
    }
}

impl Noise {
    fn tick(&mut self) {
        if self.divider == 0 {
            self.lfsr = step_noise_lfsr(self.lfsr, self.control & 0x40 != 0);
            self.divider = NOISE_PERIODS[self.period as usize] - 1;
        } else {
            self.divider -= 1;
        }
    }

    fn level(&self) -> u8 {
        if self.control & 0x80 == 0 || self.lfsr & 1 != 0 { 0 } else { self.control & 0x0f }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Apu {
    pulse: [Pulse; 2],
    triangle: Triangle,
    noise: Noise,
    master: u8,
    cycle: u64,
    sample: u16,
}

impl Apu {
    fn tick(&mut self) {
        let even = self.cycle.is_multiple_of(2);
        self.pulse[0].tick(even);
        self.pulse[1].tick(even);
        self.triangle.tick();
        self.noise.tick();
        self.cycle = self.cycle.wrapping_add(1);
        self.sample = if self.master & 0x80 == 0 {
            0
        } else {
            mix_sample(
                self.pulse[0].level(),
                self.pulse[1].level(),
                self.triangle.level(),
                self.noise.level(),
                self.master & 0x0f,
            )
        };
    }
}

/// The complete mapped bus and all v0.1 devices. A `Bus` call is one CPU cycle.
pub struct FanticonBus {
    cartridge: Cartridge,
    main_ram: Box<[u8; MAIN_RAM_SIZE]>,
    work_ram: Vec<u8>,
    video_ram: Box<[u8; VIDEO_RAM_SIZE]>,
    save_ram: Vec<u8>,
    save_dirty: bool,
    save_generation: u64,
    save_writable: bool,
    bank_kind: u8,
    bank_number: u8,
    irq_pending: u8,
    irq_enable: u8,
    cycle_irq_events: u8,
    input_sample_due: bool,
    frame_counter: u16,
    frame_high_latch: Option<u8>,
    raster_tick: u32,
    raster_matched: bool,
    video_mode: u8,
    video_control: u8,
    backdrop: u8,
    scroll_x: u16,
    scroll_y: u16,
    raster_x: u16,
    raster_y: u16,
    palette_index: u8,
    palette: [u8; 256],
    bitmap_palette: u8,
    sprite_overflow: bool,
    scanline_sprites: [u8; 8],
    scanline_sprite_count: usize,
    sprite_snapshot: [u8; 256],
    pixels: Box<[u8; DISPLAY_WIDTH * DISPLAY_HEIGHT]>,
    controllers: [Controller; 2],
    timers: [Timer; 2],
    apu: Apu,
    audio_frame: Vec<u16>,
    last_access: Option<(u16, BusAccessKind)>,
}

impl FanticonBus {
    pub fn new(cartridge: Cartridge, save_ram: Option<Vec<u8>>) -> Self {
        let expected_save = usize::from(cartridge.save_banks) * BANK_SIZE;
        let save_ram = save_ram
            .filter(|ram| ram.len() == expected_save)
            .unwrap_or_else(|| vec![0; expected_save]);
        let mut bus = Self {
            cartridge,
            main_ram: Box::new([0; MAIN_RAM_SIZE]),
            work_ram: vec![0; WORK_RAM_BANKS * BANK_SIZE],
            video_ram: Box::new([0; VIDEO_RAM_SIZE]),
            save_ram,
            save_dirty: false,
            save_generation: 0,
            save_writable: true,
            bank_kind: 0,
            bank_number: 0,
            irq_pending: 0,
            irq_enable: 0,
            cycle_irq_events: 0,
            input_sample_due: false,
            frame_counter: 0,
            frame_high_latch: None,
            raster_tick: 0,
            raster_matched: false,
            video_mode: 0,
            video_control: 0,
            backdrop: 0,
            scroll_x: 0,
            scroll_y: 0,
            raster_x: 511,
            raster_y: 511,
            palette_index: 0,
            palette: core::array::from_fn(|index| index as u8),
            bitmap_palette: 0,
            sprite_overflow: false,
            scanline_sprites: [0; 8],
            scanline_sprite_count: 0,
            sprite_snapshot: [0; 256],
            pixels: Box::new([0; DISPLAY_WIDTH * DISPLAY_HEIGHT]),
            controllers: [Controller::default(); 2],
            timers: [Timer::default(); 2],
            apu: Apu::default(),
            audio_frame: Vec::with_capacity(CPU_CYCLES_PER_FRAME as usize),
            last_access: None,
        };
        bus.reset_devices();
        bus
    }

    pub fn reset_devices(&mut self) {
        self.bank_kind = bank_kind::CARTRIDGE_ROM;
        self.bank_number = 0;
        self.irq_pending = 0;
        self.irq_enable = 0;
        self.cycle_irq_events = 0;
        self.input_sample_due = false;
        self.frame_counter = 0;
        self.frame_high_latch = None;
        self.raster_tick = 0;
        self.raster_matched = false;
        self.video_mode = 0;
        self.video_control = 0;
        self.backdrop = 0;
        self.scroll_x = 0;
        self.scroll_y = 0;
        self.raster_x = 511;
        self.raster_y = 511;
        self.palette_index = 0;
        self.palette = core::array::from_fn(|index| index as u8);
        self.bitmap_palette = 0;
        self.sprite_overflow = false;
        self.timers = [Timer::default(); 2];
        self.apu = Apu::default();
        self.audio_frame.clear();
    }

    pub fn set_controller(&mut self, controller: usize, state: ControllerState) {
        if let Some(pad) = self.controllers.get_mut(controller) {
            pad.host = state.0;
        }
    }

    pub fn controller_host_state(&self, controller: usize) -> u8 {
        self.controllers.get(controller).map_or(0, |pad| pad.host)
    }

    pub const fn cartridge_id(&self) -> u64 {
        self.cartridge.id
    }

    pub fn raster_position(&self) -> (u16, u16) {
        (
            (self.raster_tick % u32::from(DOTS_PER_SCANLINE)) as u16,
            (self.raster_tick / u32::from(DOTS_PER_SCANLINE)) as u16,
        )
    }

    pub const fn frame_counter(&self) -> u16 {
        self.frame_counter
    }
    pub const fn bank_kind(&self) -> u8 {
        self.bank_kind
    }
    pub const fn bank_number(&self) -> u8 {
        self.bank_number
    }
    pub const fn irq_pending(&self) -> u8 {
        self.irq_pending
    }
    pub const fn irq_enable(&self) -> u8 {
        self.irq_enable
    }
    pub const fn audio_master(&self) -> u8 {
        self.apu.master
    }
    pub fn apu_debug_state(&self) -> ApuDebugState {
        ApuDebugState {
            pulse_control: [self.apu.pulse[0].control, self.apu.pulse[1].control],
            pulse_timer: [self.apu.pulse[0].timer, self.apu.pulse[1].timer],
            triangle_control: self.apu.triangle.control,
            triangle_timer: self.apu.triangle.timer,
            noise_control: self.apu.noise.control,
            noise_period: self.apu.noise.period,
            master: self.apu.master,
            sample: self.apu.sample,
        }
    }
    pub const fn current_audio_sample(&self) -> u16 {
        self.apu.sample
    }
    pub fn audio_frame(&self) -> &[u16] {
        &self.audio_frame
    }
    pub const fn last_access(&self) -> Option<(u16, BusAccessKind)> {
        self.last_access
    }
    pub fn save_ram(&self) -> &[u8] {
        &self.save_ram
    }
    pub fn save_dirty(&self) -> bool {
        self.save_dirty
    }
    pub const fn save_generation(&self) -> u64 {
        self.save_generation
    }
    pub fn mark_save_clean(&mut self) {
        self.save_dirty = false;
    }
    pub fn set_save_writable(&mut self, writable: bool) {
        self.save_writable = writable;
    }

    pub fn present(&self, video: &mut Video) {
        assert_eq!(video.dimensions(), (DISPLAY_WIDTH, DISPLAY_HEIGHT));
        video.pixels_mut().copy_from_slice(self.pixels.as_slice());
        for value in 0..=u8::MAX {
            video.set_palette(value, crate::video::rgb332_to_rgba(value));
        }
    }

    pub fn peek(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x7fff => self.main_ram[address as usize],
            0x8000..=0xbfff => self.read_bank(address),
            0xc100..=0xffff => self.cartridge.fixed_rom[address as usize - 0xc000],
            _ => 0xff,
        }
    }

    fn begin_cpu_cycle(&mut self) {
        self.cycle_irq_events = 0;
        self.fetch_dot();
    }

    fn end_cpu_cycle(&mut self) {
        let mut new_irqs = 0;
        if self.timers[0].tick() {
            new_irqs |= IRQ_TIMER0;
        }
        if self.timers[1].tick() {
            new_irqs |= IRQ_TIMER1;
        }
        self.apu.tick();
        self.audio_frame.push(self.apu.sample);
        self.fetch_dot();
        self.cycle_irq_events |= new_irqs;
        self.irq_pending |= self.cycle_irq_events;
        if self.input_sample_due {
            self.controllers[0].sample();
            self.controllers[1].sample();
            self.input_sample_due = false;
        }
    }

    fn fetch_dot(&mut self) {
        let (dot, line) = self.raster_position();
        if dot == 0 {
            if line == 0 {
                self.frame_counter = self.frame_counter.wrapping_add(1);
                self.sprite_overflow = false;
                self.input_sample_due = true;
                self.audio_frame.clear();
            }
            self.evaluate_scanline_sprites(line);
        }
        if line < DISPLAY_HEIGHT as u16 && dot < DISPLAY_WIDTH as u16 {
            let pixel = self.fetch_pixel(dot as usize, line as usize);
            self.pixels[line as usize * DISPLAY_WIDTH + dot as usize] =
                self.palette[pixel as usize];
        }
        let matches = dot == self.raster_x && line == self.raster_y;
        if matches && !self.raster_matched {
            self.cycle_irq_events |= IRQ_RASTER;
        }
        self.raster_matched = matches;
        if dot == 0 && line == DISPLAY_HEIGHT as u16 {
            self.cycle_irq_events |= IRQ_VBLANK;
        }
        self.raster_tick += 1;
        if self.raster_tick == DOTS_PER_FRAME {
            self.raster_tick = 0;
        }
    }

    fn evaluate_scanline_sprites(&mut self, line: u16) {
        self.sprite_snapshot.copy_from_slice(&self.video_ram[SPRITE_TABLE..SPRITE_TABLE + 256]);
        self.scanline_sprite_count = 0;
        for index in 0..32u8 {
            let base = usize::from(index) * 8;
            let attr = self.sprite_snapshot[base + 4];
            if attr & 0x80 == 0 {
                continue;
            }
            let height = if attr & 0x40 != 0 { 16 } else { 8 };
            let y_raw = self.sprite_snapshot[base + 2];
            let y = if y_raw >= 0xf0 { i16::from(y_raw) - 256 } else { i16::from(y_raw) };
            let line = line as i16;
            if line < y || line >= y + height {
                continue;
            }
            if self.scanline_sprite_count < 8 {
                self.scanline_sprites[self.scanline_sprite_count] = index;
                self.scanline_sprite_count += 1;
            } else {
                self.sprite_overflow = true;
                break;
            }
        }
    }

    fn fetch_pixel(&self, x: usize, y: usize) -> u8 {
        let (background_index, background_nonzero, foreground) = self.background_pixel(x, y);
        if self.video_control & 2 == 0 {
            return background_index;
        }
        for &index in &self.scanline_sprites[..self.scanline_sprite_count] {
            if let Some((index, behind)) = self.sprite_pixel(index, x as i16, y as i16) {
                if foreground || (behind && background_nonzero) {
                    continue;
                }
                return index;
            }
        }
        background_index
    }

    fn background_pixel(&self, x: usize, y: usize) -> (u8, bool, bool) {
        if self.video_control & 1 == 0 || self.video_mode == 0 {
            return (self.backdrop, false, false);
        }
        match self.video_mode {
            1 => {
                let sx = (x + usize::from(self.scroll_x) % DISPLAY_WIDTH) % DISPLAY_WIDTH;
                let sy = (y + usize::from(self.scroll_y) % DISPLAY_HEIGHT) % DISPLAY_HEIGHT;
                let cell = (sy / 8) * 40 + sx / 8;
                let tile = self.video_ram[TILE_MAP + cell] as usize;
                let attr = self.video_ram[TILE_ATTRIBUTES + cell];
                let mut px = sx & 7;
                let mut py = sy & 7;
                if attr & 0x10 != 0 {
                    px = 7 - px;
                }
                if attr & 0x20 != 0 {
                    py = 7 - py;
                }
                let packed = self.video_ram[TILE_PATTERNS + tile * 32 + py * 4 + px / 2];
                let color = if px.is_multiple_of(2) { packed >> 4 } else { packed & 0x0f };
                (((attr & 0x0f) << 4) | color, color != 0, attr & 0x40 != 0 && color != 0)
            }
            2 => {
                let offset = BITMAP + y * (DISPLAY_WIDTH / 2) + x / 2;
                let packed = self.video_ram[offset];
                let color = if x.is_multiple_of(2) { packed >> 4 } else { packed & 0x0f };
                ((self.bitmap_palette << 4) | color, color != 0, false)
            }
            _ => (self.backdrop, false, false),
        }
    }

    fn sprite_pixel(&self, sprite: u8, x: i16, y: i16) -> Option<(u8, bool)> {
        let base = usize::from(sprite) * 8;
        let x_raw = u16::from(self.sprite_snapshot[base])
            | (u16::from(self.sprite_snapshot[base + 1] & 1) << 8);
        let sx = if x_raw >= 0x1f0 { x_raw as i16 - 512 } else { x_raw as i16 };
        let y_raw = self.sprite_snapshot[base + 2];
        let sy = if y_raw >= 0xf0 { i16::from(y_raw) - 256 } else { i16::from(y_raw) };
        let attr = self.sprite_snapshot[base + 4];
        let size = if attr & 0x40 != 0 { 16 } else { 8 };
        if x < sx || y < sy || x >= sx + size || y >= sy + size {
            return None;
        }
        let mut px = (x - sx) as usize;
        let mut py = (y - sy) as usize;
        if attr & 0x10 != 0 {
            px = size as usize - 1 - px;
        }
        if attr & 0x20 != 0 {
            py = size as usize - 1 - py;
        }
        let first = self.sprite_snapshot[base + 3] as usize;
        let tile = if size == 16 { first + (py / 8) * 2 + px / 8 } else { first };
        let local_x = px & 7;
        let local_y = py & 7;
        let packed = self.video_ram[tile * 32 + local_y * 4 + local_x / 2];
        let color = if local_x.is_multiple_of(2) { packed >> 4 } else { packed & 0x0f };
        (color != 0)
            .then_some((((attr & 0x0f) << 4) | color, self.sprite_snapshot[base + 1] & 2 != 0))
    }

    fn read_bank(&self, address: u16) -> u8 {
        let offset = address as usize - 0x8000;
        let bank = usize::from(self.bank_number);
        match self.bank_kind {
            bank_kind::CARTRIDGE_ROM if bank < self.cartridge.bank_count() => {
                self.cartridge.rom_banks[bank * BANK_SIZE + offset]
            }
            bank_kind::CARTRIDGE_ROM => 0xff,
            bank_kind::WORK_RAM if bank < WORK_RAM_BANKS => {
                self.work_ram[bank * BANK_SIZE + offset]
            }
            bank_kind::VIDEO_RAM if bank < 3 => self.video_ram[bank * BANK_SIZE + offset],
            bank_kind::SAVE_RAM
                if self.save_writable && bank < usize::from(self.cartridge.save_banks) =>
            {
                self.save_ram[bank * BANK_SIZE + offset]
            }
            _ => 0xff,
        }
    }

    fn write_bank(&mut self, address: u16, value: u8) {
        let offset = address as usize - 0x8000;
        let bank = usize::from(self.bank_number);
        match self.bank_kind {
            bank_kind::WORK_RAM if bank < WORK_RAM_BANKS => {
                self.work_ram[bank * BANK_SIZE + offset] = value
            }
            bank_kind::VIDEO_RAM if bank < 3 => self.video_ram[bank * BANK_SIZE + offset] = value,
            bank_kind::SAVE_RAM if bank < usize::from(self.cartridge.save_banks) => {
                self.save_ram[bank * BANK_SIZE + offset] = value;
                self.save_dirty = true;
                self.save_generation = self.save_generation.wrapping_add(1);
            }
            _ => {}
        }
    }

    fn read_io(&mut self, address: u16) -> u8 {
        match address {
            register::BANK_KIND => self.bank_kind,
            register::BANK_NUMBER => self.bank_number,
            register::IRQ_PENDING => self.irq_pending,
            register::IRQ_ENABLE => self.irq_enable,
            register::FRAME_LOW => {
                self.frame_high_latch = Some((self.frame_counter >> 8) as u8);
                self.frame_counter as u8
            }
            register::FRAME_HIGH => {
                self.frame_high_latch.take().unwrap_or((self.frame_counter >> 8) as u8)
            }
            register::MACHINE_MAJOR => 1,
            register::MACHINE_MINOR => 0,
            register::VIDEO_MODE => self.video_mode,
            register::VIDEO_CONTROL => self.video_control,
            register::BACKDROP_COLOR => self.backdrop,
            register::SCROLL_X_LOW => self.scroll_x as u8,
            register::SCROLL_X_HIGH => (self.scroll_x >> 8) as u8,
            register::SCROLL_Y_LOW => self.scroll_y as u8,
            register::SCROLL_Y_HIGH => (self.scroll_y >> 8) as u8,
            register::RASTER_X_LOW => self.raster_x as u8,
            register::RASTER_X_HIGH => ((self.raster_x >> 8) & 1) as u8,
            register::RASTER_Y_LOW => self.raster_y as u8,
            register::RASTER_Y_HIGH => ((self.raster_y >> 8) & 1) as u8,
            register::PALETTE_INDEX => self.palette_index,
            register::PALETTE_DATA => {
                let value = self.palette[self.palette_index as usize];
                self.palette_index = self.palette_index.wrapping_add(1);
                value
            }
            register::BITMAP_PALETTE => self.bitmap_palette,
            register::VIDEO_STATUS => {
                let (dot, line) = self.raster_position();
                u8::from(line >= 200)
                    | (u8::from(dot >= 320) << 1)
                    | (u8::from(self.sprite_overflow) << 2)
            }
            0xc030..=0xc03e | 0xc040 => self.read_audio(address),
            0xc03f => 0,
            register::PAD0_STATE => self.controllers[0].state,
            register::PAD0_PRESSED => {
                let value = self.controllers[0].pressed;
                self.controllers[0].pressed = 0;
                value
            }
            register::PAD1_STATE => self.controllers[1].state,
            register::PAD1_PRESSED => {
                let value = self.controllers[1].pressed;
                self.controllers[1].pressed = 0;
                value
            }
            0xc060..=0xc064 => self.read_timer(0, address - 0xc060),
            0xc068..=0xc06c => self.read_timer(1, address - 0xc068),
            _ => 0xff,
        }
    }

    fn write_io(&mut self, address: u16, value: u8) {
        match address {
            register::BANK_KIND => self.bank_kind = value,
            register::BANK_NUMBER => self.bank_number = value,
            register::IRQ_PENDING => self.irq_pending &= !(value & 0x0f),
            register::IRQ_ENABLE => self.irq_enable = value & 0x0f,
            register::VIDEO_MODE => self.video_mode = value,
            register::VIDEO_CONTROL => self.video_control = value & 3,
            register::BACKDROP_COLOR => self.backdrop = value,
            register::SCROLL_X_LOW => self.scroll_x = (self.scroll_x & 0xff00) | u16::from(value),
            register::SCROLL_X_HIGH => {
                self.scroll_x = (self.scroll_x & 0x00ff) | (u16::from(value) << 8)
            }
            register::SCROLL_Y_LOW => self.scroll_y = (self.scroll_y & 0xff00) | u16::from(value),
            register::SCROLL_Y_HIGH => {
                self.scroll_y = (self.scroll_y & 0x00ff) | (u16::from(value) << 8)
            }
            register::RASTER_X_LOW => self.raster_x = (self.raster_x & 0x100) | u16::from(value),
            register::RASTER_X_HIGH => {
                self.raster_x = (self.raster_x & 0x0ff) | (u16::from(value & 1) << 8)
            }
            register::RASTER_Y_LOW => self.raster_y = (self.raster_y & 0x100) | u16::from(value),
            register::RASTER_Y_HIGH => {
                self.raster_y = (self.raster_y & 0x0ff) | (u16::from(value & 1) << 8)
            }
            register::PALETTE_INDEX => self.palette_index = value,
            register::PALETTE_DATA => {
                self.palette[self.palette_index as usize] = value;
                self.palette_index = self.palette_index.wrapping_add(1);
            }
            register::BITMAP_PALETTE => self.bitmap_palette = value & 0x0f,
            0xc030..=0xc03e | 0xc040 => self.write_audio(address, value),
            0xc060..=0xc064 => self.write_timer(0, address - 0xc060, value),
            0xc068..=0xc06c => self.write_timer(1, address - 0xc068, value),
            _ => {}
        }
    }

    fn read_audio(&self, address: u16) -> u8 {
        match address {
            0xc030 | 0xc034 => self.apu.pulse[((address - 0xc030) / 4) as usize].control & 0xef,
            0xc031 | 0xc035 => self.apu.pulse[((address - 0xc031) / 4) as usize].timer as u8,
            0xc032 | 0xc036 => {
                (self.apu.pulse[((address - 0xc032) / 4) as usize].timer >> 8) as u8 & 7
            }
            0xc033 | 0xc037 | 0xc03b | 0xc03e => 0,
            0xc038 => self.apu.triangle.control & 0x80,
            0xc039 => self.apu.triangle.timer as u8,
            0xc03a => (self.apu.triangle.timer >> 8) as u8 & 7,
            0xc03c => self.apu.noise.control & 0xcf,
            0xc03d => self.apu.noise.period,
            0xc040 => self.apu.master & 0x8f,
            _ => 0,
        }
    }

    fn write_audio(&mut self, address: u16, value: u8) {
        match address {
            0xc030 | 0xc034 => {
                self.apu.pulse[((address - 0xc030) / 4) as usize].control = value & 0xef
            }
            0xc031 | 0xc035 => {
                let p = &mut self.apu.pulse[((address - 0xc031) / 4) as usize];
                p.timer = (p.timer & 0x700) | u16::from(value);
            }
            0xc032 | 0xc036 => {
                let p = &mut self.apu.pulse[((address - 0xc032) / 4) as usize];
                p.timer = (p.timer & 0xff) | (u16::from(value & 7) << 8);
            }
            0xc033 | 0xc037 => {
                let p = &mut self.apu.pulse[((address - 0xc033) / 4) as usize];
                p.phase = 0;
                p.divider = p.timer;
            }
            0xc038 => self.apu.triangle.control = value & 0x80,
            0xc039 => {
                self.apu.triangle.timer = (self.apu.triangle.timer & 0x700) | u16::from(value)
            }
            0xc03a => {
                self.apu.triangle.timer =
                    (self.apu.triangle.timer & 0xff) | (u16::from(value & 7) << 8)
            }
            0xc03b => {
                self.apu.triangle.phase = 0;
                self.apu.triangle.divider = self.apu.triangle.timer;
            }
            0xc03c => self.apu.noise.control = value & 0xcf,
            0xc03d => self.apu.noise.period = value & 0x0f,
            0xc03e => {
                self.apu.noise.lfsr = 1;
                self.apu.noise.divider = NOISE_PERIODS[self.apu.noise.period as usize];
            }
            0xc040 => self.apu.master = value & 0x8f,
            _ => {}
        }
    }

    fn read_timer(&mut self, index: usize, offset: u16) -> u8 {
        let timer = &mut self.timers[index];
        match offset {
            0 => timer.reload as u8,
            1 => (timer.reload >> 8) as u8,
            2 => timer.control(),
            3 => {
                let count = timer.visible_count();
                timer.high_latch = Some((count >> 8) as u8);
                count as u8
            }
            4 => timer.high_latch.take().unwrap_or((timer.visible_count() >> 8) as u8),
            _ => 0xff,
        }
    }

    fn write_timer(&mut self, index: usize, offset: u16, value: u8) {
        let timer = &mut self.timers[index];
        match offset {
            0 => timer.reload = (timer.reload & 0xff00) | u16::from(value),
            1 => timer.reload = (timer.reload & 0x00ff) | (u16::from(value) << 8),
            2 => timer.write_control(value),
            _ => {}
        }
    }
}

impl Bus for FanticonBus {
    fn read(&mut self, address: u16) -> u8 {
        self.last_access = Some((address, BusAccessKind::Read));
        self.begin_cpu_cycle();
        let value = match address {
            0x0000..=0x7fff => self.main_ram[address as usize],
            0x8000..=0xbfff => self.read_bank(address),
            0xc000..=0xc0ff => self.read_io(address),
            0xc100..=0xffff => self.cartridge.fixed_rom[address as usize - 0xc000],
        };
        self.end_cpu_cycle();
        value
    }

    fn write(&mut self, address: u16, value: u8) {
        self.last_access = Some((address, BusAccessKind::Write));
        self.begin_cpu_cycle();
        match address {
            0x0000..=0x7fff => self.main_ram[address as usize] = value,
            0x8000..=0xbfff => self.write_bank(address, value),
            0xc000..=0xc0ff => self.write_io(address, value),
            0xc100..=0xffff => {}
        }
        self.end_cpu_cycle();
    }

    fn pins(&self) -> Pins {
        Pins { irq: self.irq_pending & self.irq_enable != 0, ..Pins::default() }
    }
}

pub struct FanticonMachine {
    pub cpu: Cpu,
    pub bus: FanticonBus,
}

impl FanticonMachine {
    pub fn new(cartridge: Cartridge, save_ram: Option<Vec<u8>>) -> Self {
        let mut cpu = Cpu::default();
        cpu.request_reset();
        Self { cpu, bus: FanticonBus::new(cartridge, save_ram) }
    }

    pub fn reset(&mut self) {
        self.bus.reset_devices();
        self.cpu.request_reset();
    }

    pub fn run_frame(&mut self) {
        for _ in 0..CPU_CYCLES_PER_FRAME {
            self.cpu.clock(&mut self.bus);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cartridge(program: &[u8], save_banks: u8) -> Cartridge {
        let mut fixed = [0xff; BANK_SIZE];
        fixed[0x100..0x100 + program.len()].copy_from_slice(program);
        fixed[0x3ffa..0x4000].copy_from_slice(&[0x00, 0xc1, 0x00, 0xc1, 0x00, 0xc1]);
        Cartridge::new("SYSTEM TEST", 1, save_banks, fixed, vec![[0xa5; BANK_SIZE]]).unwrap()
    }

    #[test]
    fn bank_window_maps_rom_work_video_and_save() {
        let mut bus = FanticonBus::new(test_cartridge(&[0xea], 1), None);
        assert_eq!(bus.read(0x8000), 0xa5);
        bus.write(register::BANK_KIND, bank_kind::WORK_RAM);
        bus.write(0x8000, 0x11);
        assert_eq!(bus.read(0x8000), 0x11);
        bus.write(register::BANK_KIND, bank_kind::VIDEO_RAM);
        bus.write(0x8000, 0x22);
        assert_eq!(bus.read(0x8000), 0x22);
        bus.write(register::BANK_KIND, bank_kind::SAVE_RAM);
        bus.write(0x8000, 0x33);
        assert_eq!(bus.read(0x8000), 0x33);
        assert!(bus.save_dirty());
        bus.write(register::BANK_NUMBER, 1);
        assert_eq!(bus.read(0x8000), 0xff);
    }

    #[test]
    fn reset_sequence_executes_cartridge_code() {
        // LDA #$42; STA $20; JMP $C105
        let mut machine = FanticonMachine::new(
            test_cartridge(&[0xa9, 0x42, 0x85, 0x20, 0x4c, 0x05, 0xc1], 0),
            None,
        );
        for _ in 0..20 {
            machine.cpu.clock(&mut machine.bus);
        }
        assert_eq!(machine.bus.peek(0x20), 0x42);
    }

    #[test]
    fn timer_irq_and_read_to_clear_controller_edges_work() {
        let mut bus = FanticonBus::new(test_cartridge(&[0xea], 0), None);
        bus.write(0xc060, 2);
        bus.write(0xc062, 1);
        bus.write(register::IRQ_ENABLE, IRQ_TIMER0);
        assert!(!bus.pins().irq);
        bus.read(0);
        assert!(bus.pins().irq);

        bus.set_controller(0, ControllerState(ControllerState::A));
        while bus.raster_tick != 0 {
            bus.read(0);
        }
        bus.read(0);
        assert_eq!(bus.read(register::PAD0_PRESSED), ControllerState::A);
        assert_eq!(bus.read(register::PAD0_PRESSED), 0);
    }

    #[test]
    fn same_cycle_irq_clear_and_pressed_read_cannot_erase_new_events() {
        let mut bus = FanticonBus::new(test_cartridge(&[0xea], 0), None);
        bus.raster_tick = u32::from(DOTS_PER_SCANLINE) * DISPLAY_HEIGHT as u32;
        bus.write(register::IRQ_PENDING, IRQ_VBLANK);
        assert_eq!(bus.irq_pending & IRQ_VBLANK, IRQ_VBLANK);

        let mut bus = FanticonBus::new(test_cartridge(&[0xea], 0), None);
        bus.set_controller(0, ControllerState(ControllerState::A));
        assert_eq!(bus.read(register::PAD0_PRESSED), 0);
        assert_eq!(bus.read(register::PAD0_PRESSED), ControllerState::A);
    }

    #[test]
    fn tile_and_bitmap_pixels_are_fetched_from_vram() {
        let mut bus = FanticonBus::new(test_cartridge(&[0xea], 0), None);
        bus.video_mode = 1;
        bus.video_control = 1;
        bus.video_ram[0] = 0xa0;
        bus.video_ram[TILE_MAP] = 0;
        bus.video_ram[TILE_ATTRIBUTES] = 0x20;
        assert_eq!(bus.background_pixel(0, 7).0, 0x0a);
        bus.video_mode = 2;
        bus.bitmap_palette = 3;
        bus.video_ram[BITMAP] = 0xc5;
        assert_eq!(bus.background_pixel(0, 0).0, 0x3c);
        assert_eq!(bus.background_pixel(1, 0).0, 0x35);
    }

    #[test]
    fn one_machine_frame_is_exactly_52400_cpu_cycles_and_one_raster() {
        let mut machine = FanticonMachine::new(test_cartridge(&[0x4c, 0x00, 0xc1], 0), None);
        let start = machine.cpu.cycles;
        machine.run_frame();
        assert_eq!(machine.cpu.cycles - start, u64::from(CPU_CYCLES_PER_FRAME));
        assert_eq!(machine.bus.raster_tick, 0);
        assert_eq!(machine.bus.audio_frame().len(), CPU_CYCLES_PER_FRAME as usize);
    }

    #[test]
    fn cpu_reset_preserves_ram_but_resets_devices_and_identity_palette() {
        let mut bus = FanticonBus::new(test_cartridge(&[0xea], 1), None);
        bus.write(0x20, 0x11);
        bus.write(register::BANK_KIND, bank_kind::WORK_RAM);
        bus.write(0x8000, 0x22);
        bus.write(register::BANK_KIND, bank_kind::VIDEO_RAM);
        bus.write(0x8000, 0x33);
        bus.write(register::VIDEO_MODE, 2);
        bus.write(register::PALETTE_INDEX, 7);
        bus.write(register::PALETTE_DATA, 0xaa);
        bus.reset_devices();
        assert_eq!(bus.peek(0x20), 0x11);
        bus.bank_kind = bank_kind::WORK_RAM;
        assert_eq!(bus.read_bank(0x8000), 0x22);
        bus.bank_kind = bank_kind::VIDEO_RAM;
        assert_eq!(bus.read_bank(0x8000), 0x33);
        assert_eq!(bus.video_mode, 0);
        assert_eq!(bus.palette[7], 7);
        assert_eq!((bus.raster_x, bus.raster_y), (511, 511));
    }

    #[test]
    fn palette_auto_increment_and_sprite_scanline_snapshot_are_hardware_timed() {
        let mut bus = FanticonBus::new(test_cartridge(&[0xea], 0), None);
        bus.write(register::PALETTE_INDEX, 0xfe);
        bus.write(register::PALETTE_DATA, 0x12);
        bus.write(register::PALETTE_DATA, 0x34);
        assert_eq!(bus.palette[0xfe], 0x12);
        assert_eq!(bus.palette[0xff], 0x34);
        assert_eq!(bus.palette_index, 0);

        bus.video_ram[SPRITE_TABLE] = 5;
        bus.video_ram[SPRITE_TABLE + 4] = 0x80;
        bus.evaluate_scanline_sprites(0);
        bus.video_ram[SPRITE_TABLE] = 20;
        assert_eq!(bus.sprite_snapshot[0], 5);
        assert_eq!(bus.sprite_pixel(0, 5, 0).map(|pixel| pixel.1), None);
        assert_eq!(bus.sprite_snapshot[0], 5);
    }

    #[test]
    fn ninth_scanline_sprite_latches_overflow_until_frame_start() {
        let mut bus = FanticonBus::new(test_cartridge(&[0xea], 0), None);
        for index in 0..9 {
            bus.video_ram[SPRITE_TABLE + index * 8 + 4] = 0x80;
        }
        bus.evaluate_scanline_sprites(0);
        assert_eq!(bus.scanline_sprite_count, 8);
        assert!(bus.sprite_overflow);
        bus.video_ram[SPRITE_TABLE..SPRITE_TABLE + 256].fill(0);
        bus.raster_tick = 0;
        bus.fetch_dot();
        assert!(!bus.sprite_overflow);
    }
}

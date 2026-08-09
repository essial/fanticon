use std::{
    collections::VecDeque,
    f32::consts::PI,
    sync::{Arc, Mutex},
};

use cpal::{
    SampleFormat, Stream, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use fanticon::machine::CPU_CLOCK_HZ;

use super::{AudioFilter, AudioHighPass, AudioSettings};

const INV_U16_MAX: f32 = 1.0 / u16::MAX as f32;

pub struct AudioOutput {
    _stream: Stream,
    presenter: Arc<Mutex<AudioPresenter>>,
}

impl AudioOutput {
    pub fn new(settings: &AudioSettings) -> Result<Self, String> {
        let host = cpal::default_host();
        let device =
            host.default_output_device().ok_or_else(|| "no audio output device".to_owned())?;
        let supported = device.default_output_config().map_err(|error| error.to_string())?;
        let sample_format = supported.sample_format();
        #[cfg(not(target_arch = "wasm32"))]
        let buffer_frames = settings.buffer_size.frames().map(|requested| {
            use cpal::SupportedBufferSize;
            match supported.buffer_size() {
                SupportedBufferSize::Range { min, max } => requested.clamp(*min, *max),
                SupportedBufferSize::Unknown => requested,
            }
        });
        // Web Audio controls its own callback quantum. Keep the preference in
        // shared settings, but never let it prevent an exported web game from
        // opening its audio device.
        #[cfg(target_arch = "wasm32")]
        let buffer_frames = None;
        let mut config: StreamConfig = supported.into();
        if let Some(frames) = buffer_frames {
            config.buffer_size = cpal::BufferSize::Fixed(frames);
        }
        let presenter = Arc::new(Mutex::new(AudioPresenter::new(
            config.sample_rate.0,
            config.channels,
            settings,
        )));
        let stream = match build_stream(&device, &config, sample_format, &presenter) {
            Ok(stream) => stream,
            Err(_) if buffer_frames.is_some() => {
                config.buffer_size = cpal::BufferSize::Default;
                build_stream(&device, &config, sample_format, &presenter)?
            }
            Err(error) => return Err(error),
        };
        stream.play().map_err(|error| error.to_string())?;
        Ok(Self { _stream: stream, presenter })
    }

    pub fn submit(&self, cycle_samples: &[u16]) {
        self.submit_at_rate(cycle_samples, CPU_CLOCK_HZ);
    }

    pub fn submit_at_rate(&self, cycle_samples: &[u16], source_rate: u32) {
        if let Ok(mut presenter) = self.presenter.lock() {
            presenter.submit(cycle_samples, source_rate);
        }
    }

    pub fn clear(&self) {
        if let Ok(mut presenter) = self.presenter.lock() {
            presenter.clear();
        }
    }

    pub fn apply_processing(&self, settings: &AudioSettings) {
        if let Ok(mut presenter) = self.presenter.lock() {
            presenter.apply_settings(settings);
        }
    }
}

fn build_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    sample_format: SampleFormat,
    presenter: &Arc<Mutex<AudioPresenter>>,
) -> Result<Stream, String> {
    let callback_state = Arc::clone(presenter);
    let error_callback = |error| eprintln!("Fanticon audio output error: {error}");
    match sample_format {
        SampleFormat::F32 => device.build_output_stream(
            config,
            move |output: &mut [f32], _| fill_f32(output, &callback_state),
            error_callback,
            None,
        ),
        SampleFormat::I16 => device.build_output_stream(
            config,
            move |output: &mut [i16], _| fill_i16(output, &callback_state),
            error_callback,
            None,
        ),
        SampleFormat::U16 => device.build_output_stream(
            config,
            move |output: &mut [u16], _| fill_u16(output, &callback_state),
            error_callback,
            None,
        ),
        format => return Err(format!("unsupported audio sample format {format:?}")),
    }
    .map_err(|error| error.to_string())
}

struct CombFilter {
    buffer: Vec<f32>,
    index: usize,
    filtered: f32,
}

impl CombFilter {
    fn new(length: usize) -> Self {
        Self { buffer: vec![0.0; length.max(1)], index: 0, filtered: 0.0 }
    }

    fn process(&mut self, input: f32, feedback: f32, damping: f32) -> f32 {
        let output = self.buffer[self.index];
        self.filtered = output * (1.0 - damping) + self.filtered * damping;
        self.buffer[self.index] = input + self.filtered * feedback;
        self.index = (self.index + 1) % self.buffer.len();
        output
    }

    fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.index = 0;
        self.filtered = 0.0;
    }
}

struct AllPassFilter {
    buffer: Vec<f32>,
    index: usize,
}

impl AllPassFilter {
    fn new(length: usize) -> Self {
        Self { buffer: vec![0.0; length.max(1)], index: 0 }
    }

    fn process(&mut self, input: f32) -> f32 {
        let delayed = self.buffer[self.index];
        let output = delayed - input;
        self.buffer[self.index] = input + delayed * 0.5;
        self.index = (self.index + 1) % self.buffer.len();
        output
    }

    fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.index = 0;
    }
}

struct HallReverb {
    predelay: Vec<f32>,
    predelay_index: usize,
    left_combs: [CombFilter; 6],
    right_combs: [CombFilter; 6],
    left_diffusers: [AllPassFilter; 4],
    right_diffusers: [AllPassFilter; 4],
}

impl HallReverb {
    fn new(sample_rate: u32) -> Self {
        let samples = |milliseconds: f32| {
            (sample_rate.max(1) as f32 * milliseconds / 1_000.0).round().max(1.0) as usize
        };
        let comb_times = [29.7, 32.8, 37.1, 41.1, 43.7, 47.3];
        let diffuser_times = [5.1, 3.7, 1.7, 0.9];
        Self {
            predelay: vec![0.0; samples(22.0)],
            predelay_index: 0,
            left_combs: comb_times.map(|time| CombFilter::new(samples(time))),
            right_combs: comb_times.map(|time| CombFilter::new(samples(time + 0.7))),
            left_diffusers: diffuser_times.map(|time| AllPassFilter::new(samples(time))),
            right_diffusers: diffuser_times.map(|time| AllPassFilter::new(samples(time + 0.23))),
        }
    }

    fn process(&mut self, input: f32, amount: f32, stereo_width: f32) -> (f32, f32) {
        let predelayed = self.predelay[self.predelay_index];
        self.predelay[self.predelay_index] = input;
        self.predelay_index = (self.predelay_index + 1) % self.predelay.len();

        let feedback = 0.78 + amount * 0.13;
        // A hall loses treble much faster than bass. This fairly strong
        // one-pole damping prevents sustained chip tones from exciting narrow,
        // bell-like resonances in the comb bank.
        let damping = 0.55;
        let mut left = self
            .left_combs
            .iter_mut()
            .map(|comb| comb.process(predelayed, feedback, damping))
            .sum::<f32>()
            / self.left_combs.len() as f32;
        let mut right = self
            .right_combs
            .iter_mut()
            .map(|comb| comb.process(predelayed, feedback, damping))
            .sum::<f32>()
            / self.right_combs.len() as f32;
        for diffuser in &mut self.left_diffusers {
            left = diffuser.process(left);
        }
        for diffuser in &mut self.right_diffusers {
            right = diffuser.process(right);
        }

        let center = (left + right) * 0.5;
        let side = (left - right) * 0.5 * stereo_width;
        (center + side, center - side)
    }

    fn clear(&mut self) {
        self.predelay.fill(0.0);
        self.predelay_index = 0;
        for comb in self.left_combs.iter_mut().chain(&mut self.right_combs) {
            comb.clear();
        }
        for diffuser in self.left_diffusers.iter_mut().chain(&mut self.right_diffusers) {
            diffuser.clear();
        }
    }
}

struct AudioPresenter {
    output_rate: u32,
    channels: usize,
    source_rate: u32,
    resample_phase: u32,
    previous_input: f32,
    high_pass: f32,
    low_pass: f32,
    low_pass_2: f32,
    high_pass_coefficient: Option<f32>,
    reconstruction_alpha: f32,
    hall: HallReverb,
    queue: VecDeque<(f32, f32)>,
    queue_limit: usize,
    startup_frames: usize,
    buffering: bool,
    attack_gain: f32,
    attack_step: f32,
    release_coefficient: f32,
    last_output: (f32, f32),
    filter: AudioFilter,
    high_pass_filter: AudioHighPass,
    master_volume: f32,
    stereo_width: f32,
    reverb_amount: f32,
}

impl AudioPresenter {
    fn new(output_rate: u32, channels: u16, settings: &AudioSettings) -> Self {
        Self {
            output_rate,
            channels: usize::from(channels),
            source_rate: CPU_CLOCK_HZ,
            resample_phase: 0,
            previous_input: 0.0,
            high_pass: 0.0,
            low_pass: 0.0,
            low_pass_2: 0.0,
            high_pass_coefficient: high_pass_coefficient(CPU_CLOCK_HZ, settings.high_pass),
            reconstruction_alpha: reconstruction_alpha(CPU_CLOCK_HZ, settings.filter),
            hall: HallReverb::new(output_rate),
            queue: VecDeque::with_capacity(output_rate as usize / 10),
            queue_limit: output_rate as usize / 4,
            // Two video frames absorb ordinary scheduler jitter without making
            // tracker-key auditioning feel sluggish.
            startup_frames: (output_rate / 30).max(1) as usize,
            buffering: true,
            attack_gain: 0.0,
            attack_step: 1.0 / (output_rate as f32 * 0.003).max(1.0),
            release_coefficient: (-1.0 / (output_rate as f32 * 0.004).max(1.0)).exp(),
            last_output: (0.0, 0.0),
            filter: settings.filter,
            high_pass_filter: settings.high_pass,
            master_volume: settings.master_volume,
            stereo_width: settings.stereo_width,
            reverb_amount: settings.reverb,
        }
    }

    fn apply_settings(&mut self, settings: &AudioSettings) {
        self.filter = settings.filter;
        self.high_pass_filter = settings.high_pass;
        self.master_volume = settings.master_volume;
        self.stereo_width = settings.stereo_width;
        self.reverb_amount = settings.reverb;
        self.reconstruction_alpha = reconstruction_alpha(self.source_rate, self.filter);
        self.high_pass_coefficient = high_pass_coefficient(self.source_rate, self.high_pass_filter);
    }

    fn submit(&mut self, cycle_samples: &[u16], source_rate: u32) {
        if self.source_rate != source_rate {
            self.source_rate = source_rate;
            self.resample_phase = 0;
            self.high_pass_coefficient = high_pass_coefficient(source_rate, self.high_pass_filter);
            self.reconstruction_alpha = reconstruction_alpha(source_rate, self.filter);
        }
        for &sample in cycle_samples {
            let input = f32::from(sample) * INV_U16_MAX;
            self.high_pass = self.high_pass_coefficient.map_or(input, |coefficient| {
                coefficient * (self.high_pass + input - self.previous_input)
            });
            self.previous_input = input;
            // Reconstruct the held chip level before decimation. Two gentle poles
            // suppress CPU-rate edges and aliases without dulling audible notes.
            self.low_pass += (self.high_pass - self.low_pass) * self.reconstruction_alpha;
            self.low_pass_2 += (self.low_pass - self.low_pass_2) * self.reconstruction_alpha;
            self.resample_phase += self.output_rate;
            if self.resample_phase < self.source_rate {
                continue;
            }
            self.resample_phase -= self.source_rate;
            let mut dry = (self.low_pass_2 * 1.8).clamp(-1.0, 1.0);
            if self.filter == AudioFilter::Vintage {
                dry = (dry * 1.3).tanh() / 1.3_f32.tanh();
            }
            let (wet_left, wet_right) =
                self.hall.process(dry, self.reverb_amount, self.stereo_width);
            if self.queue.len() < self.queue_limit {
                // Zero remains genuinely dry. At higher values the parallel,
                // damped room network moves forward without burying attacks.
                // Preserve the hall's original decay and tone, but keep its
                // return at quarter strength so it stays behind the direct signal.
                let dry_gain = 1.0;
                let wet_gain = self.reverb_amount * 0.2125;
                self.queue.push_back((
                    ((dry * dry_gain + wet_left * wet_gain) * self.master_volume).clamp(-1.0, 1.0),
                    ((dry * dry_gain + wet_right * wet_gain) * self.master_volume).clamp(-1.0, 1.0),
                ));
            }
        }
    }

    fn clear(&mut self) {
        self.queue.clear();
        self.previous_input = 0.0;
        self.high_pass = 0.0;
        self.low_pass = 0.0;
        self.low_pass_2 = 0.0;
        self.hall.clear();
        self.resample_phase = 0;
        self.buffering = true;
        self.attack_gain = 0.0;
    }

    #[cfg(test)]
    fn fill(&mut self, output: &mut [f32]) {
        for frame in output.chunks_mut(self.channels) {
            let (left, right) = self.queue.pop_front().unwrap_or((0.0, 0.0));
            if let Some(sample) = frame.first_mut() {
                *sample = left;
            }
            if let Some(sample) = frame.get_mut(1) {
                *sample = right;
            }
            for sample in frame.iter_mut().skip(2) {
                *sample = (left + right) * 0.5;
            }
        }
    }

    fn next_frame(&mut self) -> (f32, f32) {
        if self.buffering {
            if self.queue.len() < self.startup_frames {
                return self.release_frame();
            }
            self.buffering = false;
            self.attack_gain = 0.0;
        }
        let Some((left, right)) = self.queue.pop_front() else {
            self.buffering = true;
            self.attack_gain = 0.0;
            return self.release_frame();
        };
        self.attack_gain = (self.attack_gain + self.attack_step).min(1.0);
        self.last_output = (left * self.attack_gain, right * self.attack_gain);
        self.last_output
    }

    fn release_frame(&mut self) -> (f32, f32) {
        self.last_output.0 *= self.release_coefficient;
        self.last_output.1 *= self.release_coefficient;
        if self.last_output.0.abs() < 1.0e-6 {
            self.last_output.0 = 0.0;
        }
        if self.last_output.1.abs() < 1.0e-6 {
            self.last_output.1 = 0.0;
        }
        self.last_output
    }
}

fn high_pass_coefficient(source_rate: u32, filter: AudioHighPass) -> Option<f32> {
    filter.cutoff_hz().map(|cutoff| (-2.0 * PI * cutoff / source_rate.max(1) as f32).exp())
}

fn reconstruction_alpha(source_rate: u32, filter: AudioFilter) -> f32 {
    1.0 - (-2.0 * PI * filter.cutoff_hz() / source_rate.max(1) as f32).exp()
}

fn fill_f32(output: &mut [f32], state: &Arc<Mutex<AudioPresenter>>) {
    let Ok(mut presenter) = state.lock() else {
        output.fill(0.0);
        return;
    };
    let channels = presenter.channels;
    for frame in output.chunks_mut(channels) {
        let (left, right) = presenter.next_frame();
        write_frame(frame, left, right, |sample| sample);
    }
}
fn fill_i16(output: &mut [i16], state: &Arc<Mutex<AudioPresenter>>) {
    let Ok(mut presenter) = state.lock() else {
        output.fill(0);
        return;
    };
    let channels = presenter.channels;
    for frame in output.chunks_mut(channels) {
        let (left, right) = presenter.next_frame();
        write_frame(frame, left, right, |sample| (sample * f32::from(i16::MAX)) as i16);
    }
}
fn fill_u16(output: &mut [u16], state: &Arc<Mutex<AudioPresenter>>) {
    let Ok(mut presenter) = state.lock() else {
        output.fill(u16::MAX / 2);
        return;
    };
    let channels = presenter.channels;
    for frame in output.chunks_mut(channels) {
        let (left, right) = presenter.next_frame();
        write_frame(frame, left, right, |sample| {
            ((sample * 0.5 + 0.5) * f32::from(u16::MAX)) as u16
        });
    }
}

fn write_frame<T: Copy>(frame: &mut [T], left: f32, right: f32, convert: impl Fn(f32) -> T) {
    if let Some(sample) = frame.first_mut() {
        *sample = convert(left);
    }
    if let Some(sample) = frame.get_mut(1) {
        *sample = convert(right);
    }
    let center = convert((left + right) * 0.5);
    for sample in frame.iter_mut().skip(2) {
        *sample = center;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presenter_resamples_one_vm_frame_to_one_host_frame_interval() {
        let mut presenter = AudioPresenter::new(48_000, 2, &AudioSettings::default());
        presenter.submit(&vec![u16::MAX / 2; 52_400], CPU_CLOCK_HZ);
        assert!((799..=801).contains(&presenter.queue.len()));
        presenter.buffering = false;
        presenter.attack_gain = 1.0;
        let mut output = vec![0.0; 1_600];
        presenter.fill(&mut output);
        assert_eq!(presenter.queue.len(), 0);
        assert!(output.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn presenter_accepts_nes_rate_music_through_the_same_effect_chain() {
        let mut presenter = AudioPresenter::new(48_000, 2, &AudioSettings::default());
        presenter.submit(&vec![u16::MAX / 2; NTSC_TEST_FRAME], 1_789_773);
        assert!((799..=801).contains(&presenter.queue.len()));
        presenter.clear();
        assert!(presenter.queue.is_empty());
    }

    #[test]
    fn post_processor_decorrelates_stereo_and_keeps_a_short_tail() {
        let mut presenter = AudioPresenter::new(48_000, 2, &AudioSettings::default());
        let mut impulse = vec![0; 52_400 * 10];
        impulse[..2_000].fill(u16::MAX);
        presenter.submit(&impulse, CPU_CLOCK_HZ);
        presenter.buffering = false;
        presenter.attack_gain = 1.0;
        let mut output = vec![0.0; 16_000];
        presenter.fill(&mut output);
        assert!(output.chunks_exact(2).any(|frame| (frame[0] - frame[1]).abs() > 0.0001));
        assert!(output[12_000..].iter().any(|sample| sample.abs() > 0.0001));
    }

    #[test]
    fn reconstruction_filter_rejects_cpu_rate_alias_energy() {
        let mut presenter = AudioPresenter::new(48_000, 2, &AudioSettings::default());
        presenter.submit(&vec![u16::MAX / 2; 1_789_773 / 4], 1_789_773);
        presenter.queue.clear();
        presenter.hall.clear();
        let source = (0..NTSC_TEST_FRAME)
            .map(|index| if index & 1 == 0 { 0 } else { u16::MAX })
            .collect::<Vec<_>>();
        presenter.submit(&source, 1_789_773);
        presenter.buffering = false;
        presenter.attack_gain = 1.0;
        let mut output = vec![0.0; 1_600];
        presenter.fill(&mut output);
        let late_peak = output[800..].iter().copied().map(f32::abs).fold(0.0, f32::max);
        assert!(late_peak < 0.02, "aliased CPU-rate energy was {late_peak}");
        assert!(presenter.high_pass_coefficient.is_some_and(|coefficient| coefficient > 0.999));
        assert!(presenter.reconstruction_alpha < 0.1);
    }

    #[test]
    fn playback_waits_for_jitter_margin_and_underruns_release_smoothly() {
        let mut presenter = AudioPresenter::new(48_000, 2, &AudioSettings::default());
        let pulse = (0..NTSC_TEST_FRAME)
            .map(|index| if index % 64 < 32 { u16::MAX } else { 0 })
            .collect::<Vec<_>>();
        presenter.submit(&pulse, 1_789_773);
        assert_eq!(presenter.next_frame(), (0.0, 0.0));
        assert!(!presenter.queue.is_empty(), "buffering must not consume the safety margin");

        presenter.submit(&pulse, 1_789_773);
        presenter.submit(&pulse, 1_789_773);
        let mut last = (0.0, 0.0);
        for _ in 0..presenter.startup_frames {
            last = presenter.next_frame();
        }
        assert_ne!(last, (0.0, 0.0));

        presenter.clear();
        let released = presenter.next_frame();
        assert!(released.0.abs() <= last.0.abs());
        assert!(released.1.abs() <= last.1.abs());
        for _ in 0..2_000 {
            presenter.next_frame();
        }
        assert!(presenter.last_output.0.abs() < 1.0e-5);
        assert!(presenter.last_output.1.abs() < 1.0e-5);
    }

    #[test]
    fn audio_profiles_change_filtering_and_zero_volume_is_silent() {
        assert!(
            reconstruction_alpha(CPU_CLOCK_HZ, AudioFilter::Crisp)
                > reconstruction_alpha(CPU_CLOCK_HZ, AudioFilter::Warm)
        );
        assert!(
            reconstruction_alpha(CPU_CLOCK_HZ, AudioFilter::Warm)
                > reconstruction_alpha(CPU_CLOCK_HZ, AudioFilter::Vintage)
        );
        assert_eq!(high_pass_coefficient(CPU_CLOCK_HZ, AudioHighPass::Off), None);
        assert!(
            high_pass_coefficient(CPU_CLOCK_HZ, AudioHighPass::Hz20)
                > high_pass_coefficient(CPU_CLOCK_HZ, AudioHighPass::Hz120)
        );

        let settings = AudioSettings { master_volume: 0.0, ..AudioSettings::default() };
        let mut presenter = AudioPresenter::new(48_000, 2, &settings);
        presenter.submit(&vec![u16::MAX; 52_400], CPU_CLOCK_HZ);
        assert!(!presenter.queue.is_empty());
        assert!(presenter.queue.iter().all(|&(left, right)| left == 0.0 && right == 0.0));
    }

    #[test]
    fn reverb_range_goes_from_dry_to_an_audible_tail() {
        let mut source = vec![0; 12_000];
        source[..64].fill(u16::MAX);

        let dry_settings = AudioSettings { reverb: 0.0, ..AudioSettings::default() };
        let mut dry = AudioPresenter::new(48_000, 2, &dry_settings);
        dry.submit(&source, 48_000);

        let wet_settings = AudioSettings { reverb: 1.0, ..AudioSettings::default() };
        let mut wet = AudioPresenter::new(48_000, 2, &wet_settings);
        wet.submit(&source, 48_000);

        let tail_energy = |presenter: &AudioPresenter| {
            presenter
                .queue
                .iter()
                .skip(7_200)
                .map(|(left, right)| left.abs() + right.abs())
                .sum::<f32>()
        };
        let dry_tail = tail_energy(&dry);
        let wet_tail = tail_energy(&wet);
        assert!(wet_tail > dry_tail * 5.0 + 0.1, "dry={dry_tail}, wet={wet_tail}");
        assert!(dry.queue.iter().all(|(left, right)| (left - right).abs() < f32::EPSILON));
    }

    const NTSC_TEST_FRAME: usize = 1_789_773 / 60;
}

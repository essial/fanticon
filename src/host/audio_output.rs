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

pub struct AudioOutput {
    _stream: Stream,
    presenter: Arc<Mutex<AudioPresenter>>,
}

impl AudioOutput {
    pub fn new() -> Result<Self, String> {
        let host = cpal::default_host();
        let device =
            host.default_output_device().ok_or_else(|| "no audio output device".to_owned())?;
        let supported = device.default_output_config().map_err(|error| error.to_string())?;
        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.into();
        let presenter =
            Arc::new(Mutex::new(AudioPresenter::new(config.sample_rate.0, config.channels)));
        let callback_state = Arc::clone(&presenter);
        let error_callback = |error| eprintln!("Fanticon audio output error: {error}");
        let stream = match sample_format {
            SampleFormat::F32 => device.build_output_stream(
                &config,
                move |output: &mut [f32], _| fill_f32(output, &callback_state),
                error_callback,
                None,
            ),
            SampleFormat::I16 => device.build_output_stream(
                &config,
                move |output: &mut [i16], _| fill_i16(output, &callback_state),
                error_callback,
                None,
            ),
            SampleFormat::U16 => device.build_output_stream(
                &config,
                move |output: &mut [u16], _| fill_u16(output, &callback_state),
                error_callback,
                None,
            ),
            format => return Err(format!("unsupported audio sample format {format:?}")),
        }
        .map_err(|error| error.to_string())?;
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
    high_pass_coefficient: f32,
    reconstruction_alpha: f32,
    reverb: Vec<f32>,
    reverb_index: usize,
    left_delay: usize,
    right_delay: usize,
    queue: VecDeque<(f32, f32)>,
    queue_limit: usize,
}

impl AudioPresenter {
    fn new(output_rate: u32, channels: u16) -> Self {
        let reverb_len = (output_rate / 20).max(1) as usize;
        Self {
            output_rate,
            channels: usize::from(channels),
            source_rate: CPU_CLOCK_HZ,
            resample_phase: 0,
            previous_input: 0.0,
            high_pass: 0.0,
            low_pass: 0.0,
            low_pass_2: 0.0,
            high_pass_coefficient: high_pass_coefficient(CPU_CLOCK_HZ),
            reconstruction_alpha: reconstruction_alpha(CPU_CLOCK_HZ),
            reverb: vec![0.0; reverb_len],
            reverb_index: 0,
            left_delay: (output_rate as f32 * 0.013) as usize,
            right_delay: (output_rate as f32 * 0.019) as usize,
            queue: VecDeque::with_capacity(output_rate as usize / 10),
            queue_limit: output_rate as usize / 4,
        }
    }

    fn submit(&mut self, cycle_samples: &[u16], source_rate: u32) {
        if self.source_rate != source_rate {
            self.source_rate = source_rate;
            self.resample_phase = 0;
            self.high_pass_coefficient = high_pass_coefficient(source_rate);
            self.reconstruction_alpha = reconstruction_alpha(source_rate);
        }
        for &sample in cycle_samples {
            let input = f32::from(sample) / f32::from(u16::MAX);
            self.high_pass =
                self.high_pass_coefficient * (self.high_pass + input - self.previous_input);
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
            let dry = (self.low_pass_2 * 1.8).clamp(-1.0, 1.0);
            let length = self.reverb.len();
            let left = self.reverb
                [(self.reverb_index + length - self.left_delay.min(length - 1)) % length];
            let right = self.reverb
                [(self.reverb_index + length - self.right_delay.min(length - 1)) % length];
            self.reverb[self.reverb_index] = dry + (left + right) * 0.11;
            self.reverb_index = (self.reverb_index + 1) % length;
            if self.queue.len() < self.queue_limit {
                // Keep attacks centered, then add differently delayed taps to each
                // side. The small opposite-side subtraction widens steady chip
                // tones without moving a voice to one speaker.
                let center = dry * 0.86;
                self.queue.push_back((
                    (center + left * 0.22 - right * 0.04).clamp(-1.0, 1.0),
                    (center + right * 0.22 - left * 0.04).clamp(-1.0, 1.0),
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
        self.reverb.fill(0.0);
        self.reverb_index = 0;
        self.resample_phase = 0;
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
        self.queue.pop_front().unwrap_or((0.0, 0.0))
    }
}

fn high_pass_coefficient(source_rate: u32) -> f32 {
    (-2.0 * PI * 20.0 / source_rate.max(1) as f32).exp()
}

fn reconstruction_alpha(source_rate: u32) -> f32 {
    1.0 - (-2.0 * PI * 14_000.0 / source_rate.max(1) as f32).exp()
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
        let mut presenter = AudioPresenter::new(48_000, 2);
        presenter.submit(&vec![u16::MAX / 2; 52_400], CPU_CLOCK_HZ);
        assert!((799..=801).contains(&presenter.queue.len()));
        let mut output = vec![0.0; 1_600];
        presenter.fill(&mut output);
        assert_eq!(presenter.queue.len(), 0);
        assert!(output.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn presenter_accepts_nes_rate_music_through_the_same_effect_chain() {
        let mut presenter = AudioPresenter::new(48_000, 2);
        presenter.submit(&vec![u16::MAX / 2; NTSC_TEST_FRAME], 1_789_773);
        assert!((799..=801).contains(&presenter.queue.len()));
        presenter.clear();
        assert!(presenter.queue.is_empty());
    }

    #[test]
    fn post_processor_decorrelates_stereo_and_keeps_a_short_tail() {
        let mut presenter = AudioPresenter::new(48_000, 2);
        let mut impulse = vec![0; 52_400];
        impulse[..2_000].fill(u16::MAX);
        presenter.submit(&impulse, CPU_CLOCK_HZ);
        let mut output = vec![0.0; 1_600];
        presenter.fill(&mut output);
        assert!(output.chunks_exact(2).any(|frame| (frame[0] - frame[1]).abs() > 0.0001));
        assert!(output[1_200..].iter().any(|sample| sample.abs() > 0.0001));
    }

    #[test]
    fn reconstruction_filter_rejects_cpu_rate_alias_energy() {
        let mut presenter = AudioPresenter::new(48_000, 2);
        presenter.submit(&vec![u16::MAX / 2; 1_789_773 / 4], 1_789_773);
        presenter.queue.clear();
        presenter.reverb.fill(0.0);
        let source = (0..NTSC_TEST_FRAME)
            .map(|index| if index & 1 == 0 { 0 } else { u16::MAX })
            .collect::<Vec<_>>();
        presenter.submit(&source, 1_789_773);
        let mut output = vec![0.0; 1_600];
        presenter.fill(&mut output);
        let late_peak = output[800..].iter().copied().map(f32::abs).fold(0.0, f32::max);
        assert!(late_peak < 0.02, "aliased CPU-rate energy was {late_peak}");
        assert!(presenter.high_pass_coefficient > 0.999);
        assert!(presenter.reconstruction_alpha < 0.1);
    }

    const NTSC_TEST_FRAME: usize = 1_789_773 / 60;
}

use std::{
    collections::VecDeque,
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
        if let Ok(mut presenter) = self.presenter.lock() {
            presenter.submit(cycle_samples);
        }
    }
}

struct AudioPresenter {
    output_rate: u32,
    channels: usize,
    resample_phase: u32,
    previous_input: f32,
    high_pass: f32,
    low_pass: f32,
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
            resample_phase: 0,
            previous_input: 0.0,
            high_pass: 0.0,
            low_pass: 0.0,
            reverb: vec![0.0; reverb_len],
            reverb_index: 0,
            left_delay: (output_rate as f32 * 0.013) as usize,
            right_delay: (output_rate as f32 * 0.019) as usize,
            queue: VecDeque::with_capacity(output_rate as usize / 10),
            queue_limit: output_rate as usize / 4,
        }
    }

    fn submit(&mut self, cycle_samples: &[u16]) {
        for &sample in cycle_samples {
            let input = f32::from(sample) / f32::from(u16::MAX);
            self.high_pass = input - self.previous_input + 0.996 * self.high_pass;
            self.previous_input = input;
            self.low_pass += (self.high_pass - self.low_pass) * 0.18;
            self.resample_phase += self.output_rate;
            if self.resample_phase < CPU_CLOCK_HZ {
                continue;
            }
            self.resample_phase -= CPU_CLOCK_HZ;
            let dry = (self.low_pass * 1.8).clamp(-1.0, 1.0);
            let length = self.reverb.len();
            let left = self.reverb
                [(self.reverb_index + length - self.left_delay.min(length - 1)) % length];
            let right = self.reverb
                [(self.reverb_index + length - self.right_delay.min(length - 1)) % length];
            self.reverb[self.reverb_index] = dry + (left + right) * 0.12;
            self.reverb_index = (self.reverb_index + 1) % length;
            if self.queue.len() < self.queue_limit {
                self.queue.push_back((
                    (dry + left * 0.08).clamp(-1.0, 1.0),
                    (dry + right * 0.08).clamp(-1.0, 1.0),
                ));
            }
        }
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
        presenter.submit(&vec![u16::MAX / 2; 52_400]);
        assert!((799..=801).contains(&presenter.queue.len()));
        let mut output = vec![0.0; 1_600];
        presenter.fill(&mut output);
        assert_eq!(presenter.queue.len(), 0);
        assert!(output.iter().all(|sample| sample.is_finite()));
    }
}

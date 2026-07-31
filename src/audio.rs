//! Deterministic waveform and mixer contracts for the Fanticon APU.
//!
//! The mapped device will drive these primitives from emulated CPU cycles. Host
//! sample rates and buffering must never affect their state.

use crate::machine::CPU_CLOCK_HZ;

pub const CHANNEL_LEVEL_MAX: u8 = 15;
pub const TIMER_MAX: u16 = 0x07ff;

/// NES-shaped pulse sequences. Phase reset selects element zero.
pub const PULSE_DUTY_TABLE: [[u8; 8]; 4] = [
    [0, 1, 0, 0, 0, 0, 0, 0],
    [0, 1, 1, 0, 0, 0, 0, 0],
    [0, 1, 1, 1, 1, 0, 0, 0],
    [1, 0, 0, 1, 1, 1, 1, 1],
];

/// NES-shaped 4-bit triangle sequence. Phase reset selects element zero.
pub const TRIANGLE_SEQUENCE: [u8; 32] = [
    15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12,
    13, 14, 15,
];

/// Noise shift intervals in Fanticon CPU cycles. The familiar NTSC NES table
/// is clock-scaled and rounded to preserve its approximate pitches at 3.144 MHz.
pub const NOISE_PERIODS: [u16; 16] =
    [7, 14, 28, 56, 112, 169, 225, 281, 355, 446, 668, 892, 1_339, 1_785, 3_573, 7_146];

#[inline]
pub const fn pulse_frequency_millihz(timer: u16) -> u32 {
    let timer = if timer > TIMER_MAX { TIMER_MAX } else { timer };
    CPU_CLOCK_HZ * 1_000 / (16 * (timer as u32 + 1))
}

#[inline]
pub const fn triangle_frequency_millihz(timer: u16) -> u32 {
    let timer = if timer > TIMER_MAX { TIMER_MAX } else { timer };
    CPU_CLOCK_HZ * 1_000 / (32 * (timer as u32 + 1))
}

#[inline]
pub const fn noise_clock_millihz(period_index: u8) -> u32 {
    let index = (period_index & 0x0f) as usize;
    CPU_CLOCK_HZ * 1_000 / NOISE_PERIODS[index] as u32
}

/// Advances the 15-bit noise generator once. Long mode feeds back bits 0 and 1;
/// short mode uses bits 0 and 6. The all-zero state is repaired to the reset seed.
#[inline]
pub const fn step_noise_lfsr(lfsr: u16, short_mode: bool) -> u16 {
    let lfsr = lfsr & 0x7fff;
    let lfsr = if lfsr == 0 { 1 } else { lfsr };
    let tap = if short_mode { 6 } else { 1 };
    let feedback = (lfsr ^ (lfsr >> tap)) & 1;
    (lfsr >> 1) | (feedback << 14)
}

const MIX_SCALE: u64 = u16::MAX as u64;

#[inline]
const fn clamp_level(level: u8) -> u64 {
    if level > CHANNEL_LEVEL_MAX { CHANNEL_LEVEL_MAX as u64 } else { level as u64 }
}

#[inline]
const fn rounded_ratio(numerator: u64, denominator: u64) -> u64 {
    (numerator + denominator / 2) / denominator
}

/// Mixes the four current 4-bit DAC levels to unsigned Q0.16 output.
///
/// This uses the NES two-path nonlinear mixer approximation with the DMC term
/// omitted. `master_volume` applies a final linear 0/15 through 15/15 scale.
#[inline]
pub const fn mix_sample(
    pulse_1: u8,
    pulse_2: u8,
    triangle: u8,
    noise: u8,
    master_volume: u8,
) -> u16 {
    let pulse_1 = clamp_level(pulse_1);
    let pulse_2 = clamp_level(pulse_2);
    let triangle = clamp_level(triangle);
    let noise = clamp_level(noise);
    let master = clamp_level(master_volume);

    if master == 0 {
        return 0;
    }

    let pulse_sum = pulse_1 + pulse_2;
    let pulse = if pulse_sum == 0 {
        0
    } else {
        rounded_ratio(MIX_SCALE * 9_588 * pulse_sum, 100 * (8_128 + 100 * pulse_sum))
    };

    // q = triangle / 8227 + noise / 12241, represented exactly as a ratio.
    let tnd_numerator = triangle * 12_241 + noise * 8_227;
    let tnd_denominator = 8_227_u64 * 12_241;
    let tnd = if tnd_numerator == 0 {
        0
    } else {
        rounded_ratio(
            MIX_SCALE * 15_979 * tnd_numerator,
            100 * (tnd_denominator + 100 * tnd_numerator),
        )
    };

    rounded_ratio((pulse + tnd) * master, CHANNEL_LEVEL_MAX as u64) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duty_sequences_have_the_promised_high_counts() {
        let high_counts = PULSE_DUTY_TABLE.map(|row| row.into_iter().sum::<u8>());
        assert_eq!(high_counts, [1, 2, 4, 6]);
    }

    #[test]
    fn triangle_is_the_exact_mirrored_four_bit_sequence() {
        assert_eq!(
            &TRIANGLE_SEQUENCE[..16],
            &[15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0]
        );
        assert_eq!(
            &TRIANGLE_SEQUENCE[16..],
            &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
        );
    }

    #[test]
    fn long_noise_visits_every_nonzero_fifteen_bit_state() {
        let mut state = 1;
        for _ in 0..32_767 {
            state = step_noise_lfsr(state, false);
            assert_ne!(state, 0);
        }
        assert_eq!(state, 1);
    }

    #[test]
    fn short_noise_repeats_after_ninety_three_steps_from_reset() {
        let mut state = 1;
        for _ in 0..93 {
            state = step_noise_lfsr(state, true);
        }
        assert_eq!(state, 1);
    }

    #[test]
    fn frequency_formulas_and_noise_table_are_stable() {
        assert_eq!(pulse_frequency_millihz(0), 196_500_000);
        assert_eq!(triangle_frequency_millihz(0), 98_250_000);
        assert_eq!(NOISE_PERIODS[0], 7);
        assert_eq!(NOISE_PERIODS[15], 7_146);
        assert!(NOISE_PERIODS.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn mixer_is_silent_at_zero_and_monotonic_per_channel() {
        assert_eq!(mix_sample(0, 0, 0, 0, 15), 0);
        assert_eq!(mix_sample(15, 15, 15, 15, 0), 0);

        for level in 0..15 {
            assert!(mix_sample(level, 0, 0, 0, 15) < mix_sample(level + 1, 0, 0, 0, 15));
            assert!(mix_sample(0, 0, level, 0, 15) < mix_sample(0, 0, level + 1, 0, 15));
            assert!(mix_sample(0, 0, 0, level, 15) < mix_sample(0, 0, 0, level + 1, 15));
        }
    }

    #[test]
    fn mixer_golden_levels_freeze_the_integer_rounding_contract() {
        assert_eq!(mix_sample(15, 0, 0, 0, 15), 9_789);
        assert_eq!(mix_sample(15, 15, 0, 0, 15), 16_940);
        assert_eq!(mix_sample(0, 0, 15, 0, 15), 16_149);
        assert_eq!(mix_sample(0, 0, 0, 15, 15), 11_431);
        assert_eq!(mix_sample(15, 15, 15, 15, 15), 41_406);
        assert_eq!(mix_sample(15, 15, 15, 15, 8), 22_083);
    }
}

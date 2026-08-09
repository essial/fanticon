use web_time::{Duration, Instant};

pub const EMULATION_HZ: u32 = 60;

const NANOS_PER_SECOND: u32 = 1_000_000_000;
const FRAME_NANOS: u32 = NANOS_PER_SECOND / EMULATION_HZ;
const FRAME_REMAINDER: u32 = NANOS_PER_SECOND % EMULATION_HZ;
const MAXIMUM_LAG: Duration = Duration::from_millis(250);

/// Produces an exact average of 60 deadlines per second without accumulating
/// integer-nanosecond rounding error.
pub struct FramePacer {
    next_deadline: Instant,
    remainder: u32,
    skipped_frames: u64,
    last_lateness: Duration,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FramePacerDiagnostics {
    pub skipped_frames: u64,
    pub last_lateness: Duration,
}

impl FramePacer {
    pub fn new(now: Instant) -> Self {
        Self { next_deadline: now, remainder: 0, skipped_frames: 0, last_lateness: Duration::ZERO }
    }

    pub fn reset(&mut self, now: Instant) {
        self.next_deadline = now;
        self.remainder = 0;
        self.last_lateness = Duration::ZERO;
    }

    pub fn is_due(&self, now: Instant) -> bool {
        now >= self.next_deadline
    }

    pub fn next_deadline(&self) -> Instant {
        self.next_deadline
    }

    /// Advance after running one emulation frame. Missed deadlines are skipped
    /// rather than executed in a burst, and a long suspension rebases the clock.
    pub fn advance_after_frame(&mut self, now: Instant) {
        self.last_lateness = now.saturating_duration_since(self.next_deadline);
        if now.saturating_duration_since(self.next_deadline) > MAXIMUM_LAG {
            self.next_deadline = now;
            self.remainder = 0;
        }

        self.advance_one_deadline();
        while self.next_deadline <= now {
            self.skipped_frames = self.skipped_frames.saturating_add(1);
            self.advance_one_deadline();
        }
    }

    pub const fn diagnostics(&self) -> FramePacerDiagnostics {
        FramePacerDiagnostics {
            skipped_frames: self.skipped_frames,
            last_lateness: self.last_lateness,
        }
    }

    fn advance_one_deadline(&mut self) {
        let mut nanos = u64::from(FRAME_NANOS);
        self.remainder += FRAME_REMAINDER;
        if self.remainder >= EMULATION_HZ {
            self.remainder -= EMULATION_HZ;
            nanos += 1;
        }
        self.next_deadline += Duration::from_nanos(nanos);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sixty_deadlines_span_exactly_one_second() {
        let start = Instant::now();
        let mut pacer = FramePacer::new(start);
        for _ in 0..60 {
            let deadline = pacer.next_deadline();
            pacer.advance_after_frame(deadline);
        }
        assert_eq!(pacer.next_deadline().duration_since(start), Duration::from_secs(1));
    }

    #[test]
    fn missed_frames_are_skipped_without_a_catch_up_burst() {
        let start = Instant::now();
        let mut pacer = FramePacer::new(start);
        let late = start + Duration::from_millis(40);
        pacer.advance_after_frame(late);
        assert!(pacer.next_deadline() > late);
        assert!(pacer.next_deadline() <= start + Duration::from_millis(51));
        assert!(pacer.diagnostics().skipped_frames >= 1);
        assert_eq!(pacer.diagnostics().last_lateness, Duration::from_millis(40));
    }

    #[test]
    fn long_pause_rebases_the_schedule() {
        let start = Instant::now();
        let mut pacer = FramePacer::new(start);
        let resumed = start + Duration::from_secs(2);
        pacer.advance_after_frame(resumed);
        let wait = pacer.next_deadline().duration_since(resumed);
        assert!(wait >= Duration::from_millis(16));
        assert!(wait <= Duration::from_millis(17));
    }
}

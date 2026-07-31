use fanticon::video::{FRAMEBUFFER_LEN, Video};
use web_time::{Duration, Instant};

const BOOT_DURATION: Duration = Duration::from_secs(5);
const INPUT_GUARD: Duration = Duration::from_millis(500);
const BOOT_LOGO: &[u8; FRAMEBUFFER_LEN] =
    include_bytes!("../../assets/branding/fanticon-logo.rgb332");

pub struct BootSplash {
    started_at: Instant,
    dismissed: bool,
}

impl BootSplash {
    pub fn new(now: Instant) -> Self {
        Self { started_at: now, dismissed: false }
    }

    pub fn reset(&mut self, now: Instant) {
        self.started_at = now;
        self.dismissed = false;
    }

    pub fn is_active(&self, now: Instant) -> bool {
        !self.dismissed && now.saturating_duration_since(self.started_at) < BOOT_DURATION
    }

    /// Returns true only when this input dismisses an active, unlocked splash.
    pub fn try_dismiss(&mut self, now: Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.started_at);
        if !self.dismissed && elapsed >= INPUT_GUARD && elapsed < BOOT_DURATION {
            self.dismissed = true;
            true
        } else {
            false
        }
    }
}

pub fn draw_boot_logo(video: &mut Video) {
    video.pixels_mut().copy_from_slice(BOOT_LOGO);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_is_ignored_during_initial_guard() {
        let start = Instant::now();
        let mut splash = BootSplash::new(start);
        assert!(!splash.try_dismiss(start + Duration::from_millis(499)));
        assert!(splash.is_active(start + Duration::from_millis(499)));
    }

    #[test]
    fn input_dismisses_after_half_a_second() {
        let start = Instant::now();
        let mut splash = BootSplash::new(start);
        assert!(splash.try_dismiss(start + Duration::from_millis(500)));
        assert!(!splash.is_active(start + Duration::from_millis(500)));
        assert!(!splash.try_dismiss(start + Duration::from_secs(1)));
    }

    #[test]
    fn splash_expires_after_five_seconds() {
        let start = Instant::now();
        let splash = BootSplash::new(start);
        assert!(splash.is_active(start + Duration::from_millis(4_999)));
        assert!(!splash.is_active(start + Duration::from_secs(5)));
    }

    #[test]
    fn logo_exactly_fills_native_framebuffer() {
        let mut video = Video::new();
        draw_boot_logo(&mut video);
        assert_eq!(video.pixels(), BOOT_LOGO);
    }
}

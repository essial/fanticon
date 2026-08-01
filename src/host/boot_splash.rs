use fanticon::video::{DISPLAY_HEIGHT, DISPLAY_WIDTH, FRAMEBUFFER_LEN, Video};
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
    debug_assert_eq!(video.dimensions(), (DISPLAY_WIDTH, DISPLAY_HEIGHT));
    let logo_width = DISPLAY_WIDTH / 2;
    let logo_height = DISPLAY_HEIGHT / 2;
    let left = (DISPLAY_WIDTH - logo_width) / 2;
    let top = (DISPLAY_HEIGHT - logo_height) / 2;
    let pixels = video.pixels_mut();
    pixels.fill(0);
    for y in 0..logo_height {
        for x in 0..logo_width {
            pixels[(top + y) * DISPLAY_WIDTH + left + x] =
                BOOT_LOGO[(y * 2) * DISPLAY_WIDTH + x * 2];
        }
    }
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
    fn logo_is_half_size_and_centered_in_native_framebuffer() {
        let mut video = Video::new();
        draw_boot_logo(&mut video);
        let left = DISPLAY_WIDTH / 4;
        let top = DISPLAY_HEIGHT / 4;
        let right = left + DISPLAY_WIDTH / 2;
        let bottom = top + DISPLAY_HEIGHT / 2;
        for y in 0..DISPLAY_HEIGHT {
            for x in 0..DISPLAY_WIDTH {
                let pixel = video.pixels()[y * DISPLAY_WIDTH + x];
                if (left..right).contains(&x) && (top..bottom).contains(&y) {
                    assert_eq!(pixel, BOOT_LOGO[((y - top) * 2) * DISPLAY_WIDTH + (x - left) * 2]);
                } else {
                    assert_eq!(pixel, 0);
                }
            }
        }
        assert!(video.pixels().iter().any(|pixel| *pixel != 0));
    }
}

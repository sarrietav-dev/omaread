//! How fast you read, learned from your page turns, and what that makes of
//! the text left.
//!
//! foliate-js measures a book in bytes of text and estimates time at a fixed
//! 1600 bytes a minute. We keep that unit and learn the rate instead: each
//! page you turn after reading it (not skimming, not stepping away) moves the
//! estimate a little toward the pace of that page.

/// foliate-js's own rate, and our starting point (about 250 words a minute).
pub const DEFAULT_BYTES_PER_MINUTE: f64 = 1600.0;

/// A page read faster than this was skimmed; slower, you stepped away.
const MIN_DWELL_SECS: f64 = 4.0;
const MAX_DWELL_SECS: f64 = 600.0;
/// Pages with less text than this (headings, images) say nothing about pace.
const MIN_PAGE_BYTES: f64 = 200.0;
/// Rates outside this band are mistakes, not readers.
const MIN_RATE: f64 = 300.0;
const MAX_RATE: f64 = 12_000.0;
/// The first pages average evenly; after that, each moves the rate by 1/20.
const WARMUP: u32 = 20;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReadingSpeed {
    pub bytes_per_minute: f64,
    pub samples: u32,
}

impl Default for ReadingSpeed {
    fn default() -> ReadingSpeed {
        ReadingSpeed { bytes_per_minute: DEFAULT_BYTES_PER_MINUTE, samples: 0 }
    }
}

impl ReadingSpeed {
    /// Learn from one page: `bytes` of text read in `secs`. Returns whether
    /// the page counted.
    pub fn observe(&mut self, bytes: f64, secs: f64) -> bool {
        if !(MIN_DWELL_SECS..=MAX_DWELL_SECS).contains(&secs) || bytes < MIN_PAGE_BYTES {
            return false;
        }
        let rate = bytes / (secs / 60.0);
        if !(MIN_RATE..=MAX_RATE).contains(&rate) {
            return false;
        }
        self.samples = self.samples.saturating_add(1);
        let weight = 1.0 / self.samples.min(WARMUP) as f64;
        // The default counts as one sample, so the first page doesn't swing it all the way.
        let weight = if self.samples == 1 { 0.5 } else { weight };
        self.bytes_per_minute += (rate - self.bytes_per_minute) * weight;
        true
    }

    /// Minutes to read `bytes` at this pace.
    pub fn minutes_for(&self, bytes: f64) -> f64 {
        (bytes.max(0.0)) / self.bytes_per_minute.max(MIN_RATE)
    }

    pub fn to_setting(&self) -> String {
        format!("{:.1} {}", self.bytes_per_minute, self.samples)
    }

    pub fn from_setting(s: &str) -> Option<ReadingSpeed> {
        let (rate, samples) = s.split_once(' ')?;
        let bytes_per_minute: f64 = rate.parse().ok()?;
        (MIN_RATE..=MAX_RATE)
            .contains(&bytes_per_minute)
            .then_some(ReadingSpeed { bytes_per_minute, samples: samples.parse().ok()? })
    }
}

/// "45 s", "12 min", "1 h 05 min". Rounded the way a person would say it.
pub fn format_minutes(minutes: f64) -> String {
    if !minutes.is_finite() || minutes <= 0.0 {
        return "0 min".into();
    }
    if minutes < 1.0 {
        return "<1 min".into();
    }
    let m = minutes.round() as u64;
    if m < 60 {
        return format!("{m} min");
    }
    let (h, m) = (m / 60, m % 60);
    if h >= 10 { format!("{h} h") } else { format!("{h} h {m:02} min") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learns_toward_the_observed_pace() {
        let mut s = ReadingSpeed::default();
        // 2000 bytes a minute, page after page.
        for _ in 0..50 {
            assert!(s.observe(1000.0, 30.0));
        }
        assert!((s.bytes_per_minute - 2000.0).abs() < 20.0, "{}", s.bytes_per_minute);
    }

    #[test]
    fn ignores_skims_breaks_and_blank_pages() {
        let mut s = ReadingSpeed::default();
        assert!(!s.observe(1500.0, 1.0));
        assert!(!s.observe(1500.0, 3600.0));
        assert!(!s.observe(50.0, 30.0));
        assert_eq!(s, ReadingSpeed::default());
    }

    #[test]
    fn one_odd_page_does_not_swing_an_established_pace() {
        let mut s = ReadingSpeed { bytes_per_minute: 1600.0, samples: 100 };
        s.observe(5000.0, 30.0); // 10000/min
        assert!(s.bytes_per_minute < 2100.0);
    }

    #[test]
    fn round_trips_through_a_setting() {
        let s = ReadingSpeed { bytes_per_minute: 1834.25, samples: 7 };
        let back = ReadingSpeed::from_setting(&s.to_setting()).unwrap();
        assert_eq!(back.samples, 7);
        assert!((back.bytes_per_minute - 1834.2).abs() < 0.11);
        assert_eq!(ReadingSpeed::from_setting("garbage"), None);
    }

    #[test]
    fn formats_like_a_person() {
        assert_eq!(format_minutes(0.4), "<1 min");
        assert_eq!(format_minutes(12.4), "12 min");
        assert_eq!(format_minutes(65.0), "1 h 05 min");
        assert_eq!(format_minutes(725.0), "12 h");
    }
}

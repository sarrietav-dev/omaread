//! Just enough colour maths to derive the chrome from a palette: parsing,
//! mixing, and WCAG contrast.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Rgb {
        Rgb { r, g, b }
    }

    /// `#rrggbb`, `#rgb`, or `#rrggbbaa` (alpha dropped). `None` for anything else.
    pub fn parse(s: &str) -> Option<Rgb> {
        let hex = s.trim().strip_prefix('#')?;
        let digit = |i: usize| u8::from_str_radix(hex.get(i..i + 1)?, 16).ok();
        let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
        match hex.len() {
            3 => Some(Rgb::new(digit(0)? * 17, digit(1)? * 17, digit(2)? * 17)),
            6 | 8 => Some(Rgb::new(byte(0)?, byte(2)?, byte(4)?)),
            _ => None,
        }
    }

    /// `self` moved toward `other` by `t` (0 = self, 1 = other), in sRGB.
    pub fn mix(self, other: Rgb, t: f64) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        let m = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t).round() as u8;
        Rgb::new(m(self.r, other.r), m(self.g, other.g), m(self.b, other.b))
    }

    /// WCAG relative luminance.
    pub fn luminance(self) -> f64 {
        let lin = |c: u8| {
            let c = c as f64 / 255.0;
            if c <= 0.039_28 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * lin(self.r) + 0.7152 * lin(self.g) + 0.0722 * lin(self.b)
    }

    pub fn contrast(self, other: Rgb) -> f64 {
        let (a, b) = (self.luminance(), other.luminance());
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    pub fn is_dark(self) -> bool {
        self.luminance() < 0.18
    }

    /// The colour moved toward `toward` just far enough to reach `ratio`
    /// against `ground`, or all the way if it never gets there.
    pub fn lifted(self, ground: Rgb, toward: Rgb, ratio: f64) -> Rgb {
        if self.contrast(ground) >= ratio {
            return self;
        }
        (1..=20)
            .map(|step| self.mix(toward, step as f64 / 20.0))
            .find(|c| c.contrast(ground) >= ratio)
            .unwrap_or(toward)
    }

    /// Black or white, whichever reads better on this colour.
    pub fn readable_ink(self) -> Rgb {
        let (black, white) = (Rgb::new(0, 0, 0), Rgb::new(255, 255, 255));
        if self.contrast(black) >= self.contrast(white) { black } else { white }
    }

    /// `rgba(r, g, b, a)` for CSS.
    pub fn alpha(self, a: f64) -> String {
        format!("rgba({}, {}, {}, {:.3})", self.r, self.g, self.b, a.clamp(0.0, 1.0))
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_forms_omarchy_writes() {
        assert_eq!(Rgb::parse("#282828"), Some(Rgb::new(0x28, 0x28, 0x28)));
        assert_eq!(Rgb::parse("#fff"), Some(Rgb::new(255, 255, 255)));
        assert_eq!(Rgb::parse("#11223344"), Some(Rgb::new(0x11, 0x22, 0x33)));
        assert_eq!(Rgb::parse("hyprland.active-border"), None);
    }

    #[test]
    fn contrast_matches_wcag() {
        let black = Rgb::new(0, 0, 0);
        let white = Rgb::new(255, 255, 255);
        assert!((black.contrast(white) - 21.0).abs() < 0.01);
    }

    #[test]
    fn lifting_reaches_the_ratio() {
        let bg = Rgb::parse("#282828").unwrap();
        let muted = Rgb::parse("#665c54").unwrap();
        let fg = Rgb::parse("#d4be98").unwrap();
        assert!(muted.lifted(bg, fg, 4.5).contrast(bg) >= 4.5);
    }
}

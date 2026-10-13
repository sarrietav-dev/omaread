//! The Omarchy look.
//!
//! Omarchy keeps the live theme in `~/.local/state/omarchy/current/theme/`:
//! `colors.toml` is the palette, and `shell.toml` carries the shell's sizes
//! and control-state tokens (the same ones `qs.Commons/Style.qml` reads, which
//! is where Flea and the bar get their look). We read both, so Omaread's
//! chrome is drawn from exactly the numbers the desktop uses.
//!
//! The directory is swapped wholesale on a theme change, which is where file
//! watches go blind, so the app polls [`fingerprint`] instead: a few hundred
//! bytes read on a timer, and it never misses.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::color::Rgb;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Dark,
    Light,
}

/// Interactive control states, `[controls]` in shell.toml. Defaults are
/// Style.qml's own.
#[derive(Debug, Clone, PartialEq)]
pub struct Controls {
    pub normal_fill_alpha: f64,
    pub hover_fill_alpha: f64,
    pub selected_fill_alpha: f64,
    pub pressed_fill_alpha: f64,
    pub selection_fill_alpha: f64,
    pub normal_border_alpha: f64,
    pub hover_border_alpha: f64,
    pub normal_border_width: u32,
    pub hover_border_width: u32,
}

impl Default for Controls {
    fn default() -> Controls {
        Controls {
            normal_fill_alpha: 0.04,
            hover_fill_alpha: 0.08,
            selected_fill_alpha: 0.18,
            pressed_fill_alpha: 0.22,
            selection_fill_alpha: 0.35,
            normal_border_alpha: 0.4,
            hover_border_alpha: 0.25,
            normal_border_width: 1,
            hover_border_width: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub name: String,
    pub mode: Mode,
    pub background: Rgb,
    pub foreground: Rgb,
    pub accent: Rgb,
    pub selection: Rgb,
    /// Secondary text, lifted to 4.5:1 on the background the way Flea does.
    pub muted: Rgb,
    pub urgent: Rgb,
    /// A step off the background for panels and code blocks.
    pub surface: Rgb,
    /// Highlight colours: red, yellow, green, blue, magenta.
    pub highlights: [Rgb; 5],
    /// `[font] base-size`, the rem root of the type scale.
    pub font_base: f64,
    /// `[spacing] scale`, already multiplied by the font scale when the theme
    /// asks spacing to follow the font.
    pub spacing_scale: f64,
    pub controls: Controls,
}

/// Where Omarchy keeps the live theme.
#[derive(Debug, Clone)]
pub struct Sources {
    pub colors: PathBuf,
    pub shell: PathBuf,
    pub name: PathBuf,
}

impl Sources {
    pub fn under_home(home: &Path) -> Sources {
        let current = home.join(".local/state/omarchy/current");
        Sources {
            colors: current.join("theme/colors.toml"),
            shell: current.join("theme/shell.toml"),
            name: current.join("theme.name"),
        }
    }

    /// `OMAREAD_THEME_HOME` stands in for `$HOME`, so a theme can be tried
    /// without switching the desktop's.
    pub fn from_env() -> Sources {
        let home = std::env::var_os("OMAREAD_THEME_HOME")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .unwrap_or_default();
        Sources::under_home(&home)
    }

    pub fn available(&self) -> bool {
        self.colors.is_file()
    }

    /// Changes whenever the desktop theme does.
    pub fn fingerprint(&self) -> Option<u64> {
        let colors = std::fs::read(&self.colors).ok()?;
        let shell = std::fs::read(&self.shell).unwrap_or_default();
        let name = std::fs::read(&self.name).unwrap_or_default();
        Some(fnv1a([&colors[..], &shell[..], &name[..]]))
    }

    /// The desktop's shell.toml, for sizing a theme that isn't the desktop's.
    pub fn shell_text(&self) -> String {
        std::fs::read_to_string(&self.shell).unwrap_or_default()
    }

    /// The live Omarchy theme, or `None` when this isn't an Omarchy desktop.
    pub fn load(&self) -> Option<Theme> {
        let colors = std::fs::read_to_string(&self.colors).ok()?;
        let shell = std::fs::read_to_string(&self.shell).unwrap_or_default();
        let name = std::fs::read_to_string(&self.name)
            .map(|n| n.trim().to_string())
            .unwrap_or_else(|_| "omarchy".into());
        Some(Theme::from_omarchy(&name, &colors, &shell))
    }
}

fn fnv1a<'a>(parts: impl IntoIterator<Item = &'a [u8]>) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for b in part {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        h ^= 0xff;
    }
    h
}

/// `key = "value"` lines. colors.toml is flat, and a hand parser keeps a
/// half-written file from failing the whole palette.
fn parse_flat(text: &str) -> HashMap<String, String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('['))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().trim_matches(['"', '\'']).trim().to_string()))
        .collect()
}

impl Theme {
    /// Omarchy shell's own fallback palette (Color.qml), for a desktop with no theme.
    pub fn fallback() -> Theme {
        Theme::from_omarchy("omarchy", "background = \"#101315\"\nforeground = \"#cacccc\"\n", "")
    }

    pub fn from_omarchy(name: &str, colors_toml: &str, shell_toml: &str) -> Theme {
        let c = parse_flat(colors_toml);
        let get = |keys: &[&str]| keys.iter().find_map(|k| c.get(*k).and_then(|v| Rgb::parse(v)));

        let background = get(&["background", "color0"]).unwrap_or(Rgb::new(0x10, 0x13, 0x15));
        let foreground = get(&["foreground", "color7"]).unwrap_or(Rgb::new(0xca, 0xcc, 0xcc));
        let accent = get(&["accent", "color4"]).unwrap_or(foreground);
        let urgent = get(&["red", "color1"]).unwrap_or(Rgb::new(0xa5, 0x55, 0x55));
        let mode = match c.get("mode").map(String::as_str) {
            Some("light") => Mode::Light,
            Some("dark") => Mode::Dark,
            _ if background.is_dark() => Mode::Dark,
            _ => Mode::Light,
        };
        let selection = get(&["selection", "selection_background"]).unwrap_or(background.mix(accent, 0.3));
        let muted = get(&["muted", "color8"])
            .unwrap_or(background.mix(foreground, 0.5))
            .lifted(background, foreground, 4.5);
        let surface = get(&["lighter_background"]).unwrap_or(background.mix(foreground, 0.06));

        let hue = |keys: &[&str], fallback: Rgb| get(keys).unwrap_or(fallback);
        let highlights = [
            urgent,
            hue(&["yellow", "color3"], Rgb::new(0xd8, 0xa6, 0x57)),
            hue(&["green", "color2"], Rgb::new(0xa9, 0xb6, 0x65)),
            hue(&["blue", "color4"], accent),
            hue(&["magenta", "color5"], Rgb::new(0xd3, 0x86, 0x9b)),
        ];

        let shell: toml::Table = shell_toml.parse().unwrap_or_default();
        let num = |section: &str, key: &str| {
            shell.get(section)?.get(key).and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
        };
        let font_base = num("font", "base-size").filter(|n| *n > 0.0).unwrap_or(12.0);
        let follows_font = shell
            .get("spacing")
            .and_then(|s| s.get("scale-with-font"))
            .and_then(toml::Value::as_bool)
            .unwrap_or(true);
        let spacing_scale = num("spacing", "scale").filter(|n| *n > 0.0).unwrap_or(1.0)
            * if follows_font { (font_base / 12.0).max(1.0 / 12.0) } else { 1.0 };

        let d = Controls::default();
        let alpha = |key: &str, fallback: f64| num("controls", key).map(|a| a.clamp(0.0, 1.0)).unwrap_or(fallback);
        let width = |key: &str, fallback: u32| num("controls", key).map(|w| w.max(0.0).round() as u32).unwrap_or(fallback);
        let controls = Controls {
            normal_fill_alpha: alpha("normal-fill-alpha", d.normal_fill_alpha),
            hover_fill_alpha: alpha("hover-cursor-fill-alpha", d.hover_fill_alpha),
            selected_fill_alpha: alpha("selected-fill-alpha", d.selected_fill_alpha),
            pressed_fill_alpha: alpha("pressed-fill-alpha", d.pressed_fill_alpha),
            selection_fill_alpha: alpha("selection-fill-alpha", d.selection_fill_alpha),
            normal_border_alpha: alpha("normal-border-alpha", d.normal_border_alpha),
            hover_border_alpha: alpha("hover-cursor-border-alpha", d.hover_border_alpha),
            normal_border_width: width("normal-border-width", d.normal_border_width),
            hover_border_width: width("hover-cursor-border-width", d.hover_border_width),
        };

        Theme {
            name: name.to_string(),
            mode,
            background,
            foreground,
            accent,
            selection,
            muted,
            urgent,
            surface,
            highlights,
            font_base,
            spacing_scale,
            controls,
        }
    }

    /// Style.qml's `fontPx`: a step of the type scale, in px.
    pub fn font_px(&self, mult: f64) -> u32 {
        (self.font_base * mult).round().max(1.0) as u32
    }

    /// Style.qml's `space`: a spacing token at the current scale.
    pub fn space(&self, px: f64) -> u32 {
        (px * self.spacing_scale).round().max(0.0) as u32
    }

    /// What the book's stylesheet needs from the theme.
    pub fn book_palette(&self) -> BookPalette {
        BookPalette {
            mode: self.mode,
            bg: self.background.to_string(),
            fg: self.foreground.to_string(),
            link: self.accent.lifted(self.background, self.foreground, 4.5).to_string(),
            muted: self.muted.to_string(),
            surface: self.surface.to_string(),
            selection: self.foreground.alpha(self.controls.selection_fill_alpha),
            highlights: self.highlights.map(|c| c.to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BookPalette {
    pub mode: Mode,
    pub bg: String,
    pub fg: String,
    pub link: String,
    pub muted: String,
    pub surface: String,
    pub selection: String,
    pub highlights: [String; 5],
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRUVBOX: &str = r##"
mode = "dark"
accent = "#7daea3"
selection = "#504945"
muted = "#665c54"
background = "#282828"
lighter_background = "#3c3836"
foreground = "#d4be98"
red = "#ea6962"
yellow = "#d8a657"
green = "#a9b665"
blue = "#7daea3"
magenta = "#d3869b"
"##;

    #[test]
    fn reads_the_palette() {
        let t = Theme::from_omarchy("gruvbox", GRUVBOX, "");
        assert_eq!(t.mode, Mode::Dark);
        assert_eq!(t.background.to_string(), "#282828");
        assert_eq!(t.accent.to_string(), "#7daea3");
        assert_eq!(t.surface.to_string(), "#3c3836");
        assert_eq!(t.highlights[0].to_string(), "#ea6962");
        assert!(t.muted.contrast(t.background) >= 4.5);
    }

    #[test]
    fn shell_tokens_override_style_defaults() {
        let shell = "[font]\nbase-size = 14\n[controls]\nhover-cursor-fill-alpha = 0.12\nnormal-border-width = 2\n";
        let t = Theme::from_omarchy("x", GRUVBOX, shell);
        assert_eq!(t.font_px(1.0), 14);
        assert_eq!(t.font_px(0.917), 13);
        assert_eq!(t.controls.hover_fill_alpha, 0.12);
        assert_eq!(t.controls.normal_border_width, 2);
        assert_eq!(t.controls.selected_fill_alpha, 0.18);
        // Spacing follows the font by default.
        assert_eq!(t.space(12.0), 14);
    }

    #[test]
    fn mode_is_inferred_when_missing() {
        let t = Theme::from_omarchy("x", "background = \"#fafafa\"\nforeground = \"#333333\"", "");
        assert_eq!(t.mode, Mode::Light);
    }

    #[test]
    fn a_broken_shell_toml_falls_back_to_defaults() {
        let t = Theme::from_omarchy("x", GRUVBOX, "[font\nbase-size = ");
        assert_eq!(t.font_base, 12.0);
        assert_eq!(t.controls, Controls::default());
    }
}

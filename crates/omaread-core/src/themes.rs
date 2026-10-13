//! Themes a book can wear instead of the desktop's: a few built in, and every
//! Omarchy theme installed on the machine.

use std::path::{Path, PathBuf};

use crate::theme::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeChoice {
    /// What `ReaderPrefs::theme` stores: `light`, `omarchy:tokyo-night`, …
    pub id: String,
    pub name: String,
}

const LIGHT: &str = r##"
mode = "light"
background = "#fffcf0"
foreground = "#100f0f"
accent = "#205ea6"
muted = "#6f6e69"
lighter_background = "#f2f0e5"
red = "#af3029"
yellow = "#ad8301"
green = "#66800b"
blue = "#205ea6"
magenta = "#a02f6f"
"##;

const DARK: &str = r##"
mode = "dark"
background = "#101315"
foreground = "#cacccc"
accent = "#7aa2c7"
muted = "#707880"
lighter_background = "#1b1f22"
red = "#c26a6a"
yellow = "#c9a554"
green = "#8fae76"
blue = "#7aa2c7"
magenta = "#b08cc4"
"##;

const SEPIA: &str = r##"
mode = "light"
background = "#f4ecd8"
foreground = "#4a3b2a"
accent = "#8f5e15"
muted = "#7a6a55"
lighter_background = "#eadfc5"
red = "#a8432f"
yellow = "#a87a10"
green = "#5f7a2a"
blue = "#3f6a8f"
magenta = "#8f4a6a"
"##;

const BUILT_IN: [(&str, &str, &str); 3] = [("light", "Light", LIGHT), ("dark", "Dark", DARK), ("sepia", "Sepia", SEPIA)];

fn omarchy_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    vec![home.join(".config/omarchy/themes"), PathBuf::from("/usr/share/omarchy/themes")]
}

/// Every theme on offer, built-ins first, then Omarchy's by name.
pub fn choices() -> Vec<ThemeChoice> {
    let mut out: Vec<ThemeChoice> =
        BUILT_IN.iter().map(|(id, name, _)| ThemeChoice { id: id.to_string(), name: name.to_string() }).collect();
    let mut omarchy: Vec<ThemeChoice> = Vec::new();
    for dir in omarchy_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let slug = entry.file_name().to_string_lossy().into_owned();
            if entry.path().join("colors.toml").is_file() && !omarchy.iter().any(|c| c.id.ends_with(&format!(":{slug}"))) {
                omarchy.push(ThemeChoice { id: format!("omarchy:{slug}"), name: pretty(&slug) });
            }
        }
    }
    omarchy.sort_by(|a, b| a.name.cmp(&b.name));
    out.extend(omarchy);
    out
}

/// The theme for an id, sized by the desktop's `shell.toml`.
pub fn load(id: &str, shell_toml: &str) -> Option<Theme> {
    if let Some((_, _, colors)) = BUILT_IN.iter().find(|(i, _, _)| *i == id) {
        return Some(Theme::from_omarchy(id, colors, shell_toml));
    }
    let slug = id.strip_prefix("omarchy:")?;
    // A slug is a directory name, never a path.
    if slug.is_empty() || slug.contains(['/', '\\']) || slug.starts_with('.') {
        return None;
    }
    omarchy_dirs().iter().find_map(|dir| load_dir(&dir.join(slug), slug, shell_toml))
}

fn load_dir(dir: &Path, slug: &str, shell_toml: &str) -> Option<Theme> {
    let colors = std::fs::read_to_string(dir.join("colors.toml")).ok()?;
    Some(Theme::from_omarchy(slug, &colors, shell_toml))
}

pub fn name_of(id: Option<&str>) -> String {
    match id {
        None => "Omarchy".into(),
        Some(id) => BUILT_IN
            .iter()
            .find(|(i, _, _)| *i == id)
            .map(|(_, n, _)| n.to_string())
            .unwrap_or_else(|| pretty(id.strip_prefix("omarchy:").unwrap_or(id))),
    }
}

fn pretty(slug: &str) -> String {
    slug.split(['-', '_'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Mode;

    #[test]
    fn built_ins_load() {
        assert_eq!(load("light", "").unwrap().mode, Mode::Light);
        assert_eq!(load("dark", "").unwrap().mode, Mode::Dark);
        assert_eq!(load("sepia", "").unwrap().background.to_string(), "#f4ecd8");
    }

    #[test]
    fn ids_cannot_escape_the_theme_folders() {
        assert_eq!(load("omarchy:../../etc", ""), None);
        assert_eq!(load("omarchy:", ""), None);
        assert_eq!(load("nonsense", ""), None);
    }

    #[test]
    fn names_read_well() {
        assert_eq!(name_of(None), "Omarchy");
        assert_eq!(name_of(Some("sepia")), "Sepia");
        assert_eq!(name_of(Some("omarchy:tokyo-night")), "Tokyo Night");
    }
}

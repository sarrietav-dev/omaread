//! `~/.config/omaread/config.toml`. Everything is optional; a missing or
//! broken file means defaults.
//!
//! ```toml
//! [library]
//! folders = ["~/Documents/Books"]
//!
//! [keymap.reader]
//! next_page = ["j", "l", "space"]
//! prev_page = ["k", "h"]
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub library: LibraryConfig,
    pub lookup: LookupConfig,
    /// `[keymap.<context>] <action> = ["key", …]` rebinds the defaults.
    pub keymap: Keymap,
}

/// One binding's keys: `["j", "space"]`, or a lone `"j"`.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Keys {
    One(String),
    Many(Vec<String>),
}

impl Keys {
    pub fn list(&self) -> Vec<String> {
        match self {
            Keys::One(k) => vec![k.clone()],
            Keys::Many(ks) => ks.clone(),
        }
    }
}

/// Context → action → keys, as read from `[keymap.<context>]`.
pub type Keymap = BTreeMap<String, BTreeMap<String, Keys>>;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LookupConfig {
    /// Replaces the built-in set when given.
    pub provider: Vec<crate::lookup::ProviderSpec>,
}

impl LookupConfig {
    pub fn providers(&self) -> Vec<crate::lookup::ProviderSpec> {
        if self.provider.is_empty() { crate::lookup::default_providers() } else { self.provider.clone() }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LibraryConfig {
    pub folders: Vec<String>,
}

impl Default for Config {
    fn default() -> Config {
        Config { library: LibraryConfig::default(), lookup: LookupConfig::default(), keymap: Keymap::new() }
    }
}

impl Default for LibraryConfig {
    fn default() -> LibraryConfig {
        LibraryConfig { folders: vec!["~/Documents/Books".into()] }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        let home = home();
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
            .join("omaread/config.toml")
    }

    pub fn load() -> Config {
        let Ok(text) = std::fs::read_to_string(Config::path()) else {
            return Config::default();
        };
        Config::parse(&text).unwrap_or_else(|e| {
            eprintln!("omaread: ignoring {}: {e}", Config::path().display());
            Config::default()
        })
    }

    pub fn parse(text: &str) -> Result<Config, toml::de::Error> {
        toml::from_str(text)
    }

    /// The library folders, with `~` expanded.
    pub fn library_folders(&self) -> Vec<PathBuf> {
        self.library.folders.iter().map(|f| expand(f)).collect()
    }

    /// A cheap stamp of the config file, to notice edits without re-reading
    /// it on every tick. `None` when there is no file.
    pub fn fingerprint() -> Option<u64> {
        let meta = std::fs::metadata(Config::path()).ok()?;
        let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos() as u64)?;
        Some(mtime ^ meta.len().rotate_left(17))
    }
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None if path == "~" => home(),
        None => PathBuf::from(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_documents_books() {
        let c = Config::parse("").unwrap();
        assert!(c.library_folders()[0].ends_with("Documents/Books"));
    }

    #[test]
    fn lookup_providers_default_and_override() {
        assert_eq!(Config::parse("").unwrap().lookup.providers().len(), 3);
        let c = Config::parse(
            "[[lookup.provider]]\nname = \"Local\"\nkind = \"command\"\nkey = \"D\"\ncommand = [\"sdcv\", \"-n\", \"{text}\"]\n",
        )
        .unwrap();
        let p = c.lookup.providers();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].command, vec!["sdcv", "-n", "{text}"]);
    }

    #[test]
    fn reads_folders() {
        let c = Config::parse("[library]\nfolders = [\"/srv/books\", \"~/x\"]").unwrap();
        assert_eq!(c.library_folders()[0], PathBuf::from("/srv/books"));
        assert!(c.library_folders()[1].ends_with("x"));
    }

    #[test]
    fn keymap_takes_a_list_or_a_single_key() {
        let c = Config::parse(
            "[keymap.reader]\nnext_page = [\"j\", \"space\"]\nprev_page = \"k\"\n[keymap.library]\nopen = [\"enter\", \"o\"]\n",
        )
        .unwrap();
        assert_eq!(c.keymap["reader"]["next_page"].list(), vec!["j", "space"]);
        assert_eq!(c.keymap["reader"]["prev_page"].list(), vec!["k"]);
        assert_eq!(c.keymap["library"]["open"].list(), vec!["enter", "o"]);
        assert!(Config::parse("").unwrap().keymap.is_empty());
    }
}

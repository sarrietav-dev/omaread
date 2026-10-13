//! `~/.config/omaread/config.toml`. Everything is optional; a missing or
//! broken file means defaults.
//!
//! ```toml
//! [library]
//! folders = ["~/Documents/Books"]
//! ```

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub library: LibraryConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LibraryConfig {
    pub folders: Vec<String>,
}

impl Default for Config {
    fn default() -> Config {
        Config { library: LibraryConfig::default() }
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
    fn reads_folders() {
        let c = Config::parse("[library]\nfolders = [\"/srv/books\", \"~/x\"]").unwrap();
        assert_eq!(c.library_folders()[0], PathBuf::from("/srv/books"));
        assert!(c.library_folders()[1].ends_with("x"));
    }
}

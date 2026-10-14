//! `~/.config/omaread/config.toml`. Everything is optional; a missing or
//! broken file means defaults. The library's folders are added and removed
//! in the app, which writes them back here, keeping the rest of the file.
//!
//! ```toml
//! [library]
//! folders = ["~/Books", "/mnt/nas/books"]
//!
//! [keymap.reader]
//! next_page = ["j", "l", "space"]
//! prev_page = ["k", "h"]
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
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

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LibraryConfig {
    /// `None` when the file never named any, which an old library reads as
    /// the folder it used to imply (see [`Config::adopt_old_default`]).
    pub folders: Option<Vec<String>>,
}

/// Where books were read from before folders were chosen in the app.
const OLD_DEFAULT: &str = "~/Documents/Books";

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

    /// The library folders, with `~` expanded. None until some are added.
    pub fn library_folders(&self) -> Vec<PathBuf> {
        self.library.folders.iter().flatten().map(|f| expand(f)).collect()
    }

    /// Libraries from before folders were chosen in the app read
    /// `~/Documents/Books` without naming it. Name it, so they keep it.
    pub fn adopt_old_default(&mut self, library_has_books: bool) {
        if self.library.folders.is_some() || !library_has_books || !expand(OLD_DEFAULT).is_dir() {
            return;
        }
        let folders = vec![expand(OLD_DEFAULT)];
        match save_library_folders(&Config::path(), &folders) {
            Ok(()) => self.library.folders = Some(vec![OLD_DEFAULT.into()]),
            Err(e) => eprintln!("omaread: couldn't name {OLD_DEFAULT} in {}: {e}", Config::path().display()),
        }
    }

    /// A cheap stamp of the config file, to notice edits without re-reading
    /// it on every tick. `None` when there is no file.
    pub fn fingerprint() -> Option<u64> {
        let meta = std::fs::metadata(Config::path()).ok()?;
        let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos() as u64)?;
        Some(mtime ^ meta.len().rotate_left(17))
    }
}

/// Write `[library] folders` into the config at `path`, leaving everything
/// else in it (comments too) as it was. A file that doesn't parse is left
/// alone rather than overwritten.
pub fn save_library_folders(path: &Path, folders: &[PathBuf]) -> Result<(), String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.to_string()),
    };
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| format!("fix it first: {e}"))?;
    if !doc.contains_key("library") {
        doc["library"] = toml_edit::table();
    }
    let list: toml_edit::Array = folders.iter().map(|f| contract(f)).collect();
    doc["library"]["folders"] = toml_edit::value(list);

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    // Write beside it and rename, so a crash can't leave half a config.
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, doc.to_string()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// `~/Books` for a folder under home, so the config reads well and moves
/// between machines.
pub fn contract(path: &Path) -> String {
    let home = home();
    match path.strip_prefix(&home) {
        Ok(rest) if !home.as_os_str().is_empty() && rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) if !home.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
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
    fn no_folders_by_default() {
        let c = Config::parse("").unwrap();
        assert!(c.library_folders().is_empty());
        assert!(c.library.folders.is_none());
        assert_eq!(Config::parse("[library]\nfolders = []").unwrap().library.folders, Some(vec![]));
    }

    #[test]
    fn saving_folders_keeps_the_rest_of_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omaread/config.toml");
        save_library_folders(&path, &[PathBuf::from("/srv/books")]).unwrap();
        assert_eq!(Config::parse(&std::fs::read_to_string(&path).unwrap()).unwrap().library_folders(), vec![PathBuf::from("/srv/books")]);

        let text = "# mine\n[keymap.reader]\nnext_page = \"j\" # down\n\n[library]\nfolders = [\"/old\"]\n";
        std::fs::write(&path, text).unwrap();
        save_library_folders(&path, &[PathBuf::from("/a"), PathBuf::from("/b")]).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("# mine") && saved.contains("# down"));
        let c = Config::parse(&saved).unwrap();
        assert_eq!(c.library_folders(), vec![PathBuf::from("/a"), PathBuf::from("/b")]);
        assert_eq!(c.keymap["reader"]["next_page"].list(), vec!["j"]);

        // A broken file is left for its owner to fix.
        std::fs::write(&path, "[library\n").unwrap();
        assert!(save_library_folders(&path, &[]).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[library\n");
    }

    #[test]
    fn contracts_home() {
        let home = home();
        assert_eq!(contract(&home.join("Books/Rome")), "~/Books/Rome");
        assert_eq!(contract(&home), "~");
        assert_eq!(contract(Path::new("/srv/books")), "/srv/books");
        assert_eq!(expand(&contract(&home.join("x"))), home.join("x"));
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

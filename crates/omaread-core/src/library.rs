//! The library: every EPUB under the configured folders.
//!
//! A scan walks the folders, skips files whose size and mtime haven't moved
//! since the last scan, reads the rest in parallel, and writes thumbnails of
//! their covers to the cache. Each folder becomes a shelf.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rayon::prelude::*;

use crate::db::{BookRow, Db, Progress};
use crate::epub::{Epub, Metadata};

/// Thumbnails are drawn at up to this size (2× a grid cell).
const THUMB_W: u32 = 300;
const THUMB_H: u32 = 450;

#[derive(Debug, Clone)]
pub struct Paths {
    pub db: PathBuf,
    pub covers: PathBuf,
}

impl Paths {
    /// `~/.local/share/omaread/omaread.db` and `~/.cache/omaread/covers/`.
    pub fn from_env() -> Paths {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/share"));
        let cache = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".cache"));
        Paths { db: data.join("omaread/omaread.db"), covers: cache.join("omaread/covers") }
    }

    pub fn open_db(&self) -> Result<Db, crate::db::Error> {
        Db::open(&self.db)
    }
}

/// A book as the library shows it: the file, and where you are in it.
#[derive(Debug, Clone, PartialEq)]
pub struct Book {
    pub path: PathBuf,
    pub identity: String,
    pub title: String,
    pub authors: Vec<String>,
    pub language: Option<String>,
    pub series: Option<String>,
    pub series_index: Option<f64>,
    pub shelf: String,
    pub cover: Option<PathBuf>,
    /// Why the book can't be read, if it can't.
    pub error: Option<String>,
    pub added_at: i64,
    pub progress: Option<Progress>,
}

impl Book {
    pub fn fraction(&self) -> f64 {
        self.progress.as_ref().map(|p| p.fraction).unwrap_or(0.0)
    }

    pub fn opened_at(&self) -> i64 {
        self.progress.as_ref().map(|p| p.opened_at).unwrap_or(0)
    }

    /// Started and not finished.
    pub fn in_progress(&self) -> bool {
        self.progress.is_some() && self.fraction() < 0.995
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ScanReport {
    pub total: usize,
    pub read: usize,
    pub removed: usize,
    pub failed: usize,
}

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Every book the store knows, with progress.
pub fn load(db: &Db) -> Result<Vec<Book>, crate::db::Error> {
    let progress: HashMap<String, Progress> = db.all_progress()?.into_iter().collect();
    let mut rows = db.books()?;
    // Copies of one book show once: the first path in sort order.
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    let mut seen = HashSet::new();
    rows.retain(|r| seen.insert(r.identity.clone()));
    let shelves = display_shelves(rows.iter().map(|r| r.shelf.as_str()));
    Ok(rows
        .into_iter()
        .map(|mut r| {
            r.shelf = shelves.get(r.shelf.as_str()).cloned().unwrap_or(r.shelf);
            r
        })
        .map(|r| Book {
            progress: progress.get(&r.identity).cloned(),
            path: PathBuf::from(r.path),
            identity: r.identity,
            title: r.title,
            authors: r.authors,
            language: r.language,
            series: r.series,
            series_index: r.series_index,
            shelf: r.shelf,
            cover: r.cover.map(PathBuf::from),
            error: r.error,
            added_at: r.added_at,
        })
        .collect())
}

/// Bring the store in line with the folders.
pub fn scan(db: &mut Db, roots: &[PathBuf], covers: &Path) -> Result<ScanReport, crate::db::Error> {
    let mut files = Vec::new();
    for root in roots {
        walk(root, root, &mut files);
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));

    let known: HashMap<String, (i64, i64)> = db.stamps()?.into_iter().map(|(p, s, m)| (p, (s, m))).collect();
    let present: HashSet<&str> = files.iter().filter_map(|f| f.path.to_str()).collect();
    let removed: Vec<String> = known.keys().filter(|p| !present.contains(p.as_str())).cloned().collect();

    let changed: Vec<&Found> = files
        .iter()
        .filter(|f| f.path.to_str().is_some_and(|p| known.get(p) != Some(&(f.size, f.mtime))))
        .collect();

    let _ = std::fs::create_dir_all(covers);
    let added_at = now();
    let mut rows: Vec<BookRow> = changed.par_iter().map(|f| read_book(f, covers, added_at)).collect();

    // Two files claiming one identifier are either copies of one book (same
    // size: they share a reading position and show once) or different
    // editions from a careless publisher (the later one falls back to its hash).
    let mut taken: HashMap<String, (String, i64)> = db
        .books()?
        .into_iter()
        .filter(|b| !removed.contains(&b.path) && !rows.iter().any(|r| r.path == b.path))
        .map(|b| (b.identity, (b.path, b.size)))
        .collect();
    for row in &mut rows {
        if taken.get(&row.identity).is_some_and(|(path, size)| path != &row.path && *size != row.size) {
            row.identity = hash_identity(Path::new(&row.path)).unwrap_or_else(|| format!("p:{}", row.path));
        }
        taken.entry(row.identity.clone()).or_insert((row.path.clone(), row.size));
    }

    let report = ScanReport {
        total: files.len(),
        read: rows.len(),
        removed: removed.len(),
        failed: rows.iter().filter(|r| r.error.is_some()).count(),
    };
    db.sync(&rows, &removed)?;
    Ok(report)
}

struct Found {
    path: PathBuf,
    shelf: String,
    size: i64,
    mtime: i64,
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<Found>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else { continue };
        let path = entry.path();
        // Directory symlinks are not followed, so a loop can't trap the scan.
        if kind.is_dir() {
            walk(root, &path, out);
            continue;
        }
        let is_epub = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("epub"));
        if !is_epub {
            continue;
        }
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        if !meta.is_file() {
            continue;
        }
        let mtime = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64);
        out.push(Found {
            shelf: shelf_of(root, &path),
            size: meta.len() as i64,
            mtime: mtime.unwrap_or(0),
            path,
        });
    }
}

/// A nested folder holding a single book is that book's own folder (an
/// unzipped download, say), not a shelf: show the book on the parent's.
fn display_shelves<'a>(folders: impl Iterator<Item = &'a str>) -> HashMap<String, String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for f in folders {
        *counts.entry(f).or_default() += 1;
    }
    counts
        .iter()
        .map(|(&folder, &n)| {
            let shelf = match folder.rsplit_once(" / ") {
                Some((parent, _)) if n == 1 => parent.to_string(),
                _ => folder.to_string(),
            };
            (folder.to_string(), shelf)
        })
        .collect()
}

/// The folder a book sits in, relative to its root: "Go", "Computer Science",
/// "History / Rome". Books at the root have no shelf ("").
pub fn shelf_of(root: &Path, path: &Path) -> String {
    path.parent()
        .and_then(|p| p.strip_prefix(root).ok())
        .map(|rel| rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join(" / "))
        .unwrap_or_default()
}

fn read_book(f: &Found, covers: &Path, added_at: i64) -> BookRow {
    let path_str = f.path.to_string_lossy().into_owned();
    let fallback_title = f.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut row = BookRow {
        path: path_str.clone(),
        identity: String::new(),
        size: f.size,
        mtime: f.mtime,
        title: fallback_title,
        authors: Vec::new(),
        language: None,
        series: None,
        series_index: None,
        shelf: f.shelf.clone(),
        cover: None,
        error: None,
        added_at,
    };

    let read = Epub::open(&f.path).and_then(|mut epub| {
        let meta = epub.metadata()?;
        let cover = epub.cover(&meta);
        Ok((meta, cover))
    });
    match read {
        Ok((meta, cover)) => {
            row.identity = identity_of(&meta).or_else(|| hash_identity(&f.path)).unwrap_or_else(|| format!("p:{path_str}"));
            if !meta.title.is_empty() {
                row.title = meta.title;
            }
            row.authors = meta.authors;
            row.language = meta.language;
            row.series = meta.series;
            row.series_index = meta.series_index;
            row.cover = cover.and_then(|bytes| thumbnail(&bytes, covers, &row.identity));
        }
        Err(e) => {
            row.identity = hash_identity(&f.path).unwrap_or_else(|| format!("p:{path_str}"));
            row.error = Some(e.to_string());
        }
    }
    row
}

/// `id:<identifier>` when the book declares one worth trusting.
pub fn identity_of(meta: &Metadata) -> Option<String> {
    let id = meta.identifier.as_deref()?.trim();
    // Placeholders some tools write into every book.
    let junk = id.len() < 4 || ["unknown", "none", "isbn", "uuid", "0000000000"].iter().any(|j| id.eq_ignore_ascii_case(j));
    (!junk).then(|| format!("id:{id}"))
}

/// `h:<hash>` of the size and first 64 KiB: stable across moves and renames.
pub fn hash_identity(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    let mut head = vec![0; 64 * 1024];
    let n = file.read(&mut head).ok()?;
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ size;
    for b in &head[..n] {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    Some(format!("h:{h:016x}"))
}

/// Identity of a single file outside a scan (opened from the command line).
pub fn identify(path: &Path) -> (String, Option<Metadata>) {
    let meta = Epub::open(path).and_then(|mut e| e.metadata()).ok();
    let identity = meta
        .as_ref()
        .and_then(identity_of)
        .or_else(|| hash_identity(path))
        .unwrap_or_else(|| format!("p:{}", path.display()));
    (identity, meta)
}

fn thumbnail(bytes: &[u8], covers: &Path, identity: &str) -> Option<String> {
    let image = image::load_from_memory(bytes).ok()?;
    let thumb = if image.width() > THUMB_W || image.height() > THUMB_H {
        image.resize(THUMB_W, THUMB_H, image::imageops::FilterType::Triangle)
    } else {
        image
    };
    let mut name: u64 = 0xcbf2_9ce4_8422_2325;
    for b in identity.as_bytes() {
        name ^= *b as u64;
        name = name.wrapping_mul(0x100_0000_01b3);
    }
    let path = covers.join(format!("{name:016x}.jpg"));
    thumb.to_rgb8().save_with_format(&path, image::ImageFormat::Jpeg).ok()?;
    Some(path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shelves_are_folders_relative_to_the_root() {
        let root = Path::new("/books");
        assert_eq!(shelf_of(root, Path::new("/books/Go/x.epub")), "Go");
        assert_eq!(shelf_of(root, Path::new("/books/History/Rome/x.epub")), "History / Rome");
        assert_eq!(shelf_of(root, Path::new("/books/x.epub")), "");
    }

    #[test]
    fn single_book_subfolders_fold_into_their_parent() {
        let shelves = display_shelves(["Finances", "Finances / Some Book EPUB", "Go", "Business / Sales", "Business / Sales"].into_iter());
        assert_eq!(shelves["Finances / Some Book EPUB"], "Finances");
        assert_eq!(shelves["Go"], "Go");
        assert_eq!(shelves["Business / Sales"], "Business / Sales");
    }

    #[test]
    fn placeholder_identifiers_are_not_trusted() {
        let meta = |id: &str| Metadata { identifier: Some(id.into()), ..Default::default() };
        assert_eq!(identity_of(&meta("urn:uuid:abc-123")).as_deref(), Some("id:urn:uuid:abc-123"));
        assert_eq!(identity_of(&meta("unknown")), None);
        assert_eq!(identity_of(&meta("")), None);
    }

    #[test]
    fn scanning_a_folder_finds_books_and_skips_unchanged_ones() {
        let dir = tempfile::tempdir().unwrap();
        let shelf = dir.path().join("Go");
        std::fs::create_dir(&shelf).unwrap();
        std::fs::write(shelf.join("broken.epub"), b"not a zip").unwrap();
        std::fs::write(shelf.join("notes.pdf"), b"%PDF").unwrap();
        let covers = dir.path().join("covers");
        let mut db = Db::in_memory().unwrap();

        let first = scan(&mut db, &[dir.path().to_path_buf()], &covers).unwrap();
        assert_eq!(first, ScanReport { total: 1, read: 1, removed: 0, failed: 1 });
        let books = load(&db).unwrap();
        assert_eq!(books[0].shelf, "Go");
        assert_eq!(books[0].title, "broken");
        assert!(books[0].error.is_some());

        let second = scan(&mut db, &[dir.path().to_path_buf()], &covers).unwrap();
        assert_eq!(second.read, 0);

        std::fs::remove_file(shelf.join("broken.epub")).unwrap();
        let third = scan(&mut db, &[dir.path().to_path_buf()], &covers).unwrap();
        assert_eq!(third.removed, 1);
        assert!(load(&db).unwrap().is_empty());
    }
}

//! Highlights, notes and bookmarks, and getting them in and out: Markdown and
//! JSON export, and a one-time import from Foliate.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::db::{Db, Error, Progress};
use crate::library::Book;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Highlight,
    Bookmark,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Highlight => "highlight",
            Kind::Bookmark => "bookmark",
        }
    }

    pub fn parse(s: &str) -> Kind {
        if s == "bookmark" { Kind::Bookmark } else { Kind::Highlight }
    }
}

/// Highlight styles: the five theme colours, and Foliate's three line styles.
pub const COLORS: [&str; 8] = ["yellow", "red", "green", "blue", "magenta", "underline", "squiggly", "strikethrough"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    pub id: i64,
    pub kind: Kind,
    /// An EPUB CFI: a range for highlights, a point for bookmarks.
    pub cfi: String,
    pub color: String,
    /// The highlighted text, or the start of the bookmarked page.
    pub text: String,
    pub note: String,
    /// The chapter it's in, for grouping.
    pub chapter: String,
    /// Where in the book, 0–1, for ordering.
    pub fraction: f64,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Db {
    pub fn annotations(&self, identity: &str) -> Result<Vec<Annotation>, Error> {
        let mut stmt = self.conn().prepare_cached(
            "SELECT id, kind, cfi, color, text, note, chapter, fraction, created_at, updated_at
             FROM annotations WHERE identity = ?1 ORDER BY fraction, created_at",
        )?;
        stmt.query_map([identity], |r| {
            Ok(Annotation {
                id: r.get(0)?,
                kind: Kind::parse(&r.get::<_, String>(1)?),
                cfi: r.get(2)?,
                color: r.get(3)?,
                text: r.get(4)?,
                note: r.get(5)?,
                chapter: r.get(6)?,
                fraction: r.get(7)?,
                created_at: r.get(8)?,
                updated_at: r.get(9)?,
            })
        })?
        .collect()
    }

    /// Insert, or update the one already at this place. Returns its id.
    pub fn save_annotation(&self, identity: &str, a: &Annotation) -> Result<i64, Error> {
        self.conn()
            .prepare_cached(
                "INSERT INTO annotations (identity, kind, cfi, color, text, note, chapter, fraction, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(identity, kind, cfi) DO UPDATE SET
                    color = excluded.color, note = excluded.note, updated_at = excluded.updated_at,
                    chapter = CASE WHEN excluded.chapter = '' THEN annotations.chapter ELSE excluded.chapter END,
                    fraction = CASE WHEN excluded.chapter = '' THEN annotations.fraction ELSE excluded.fraction END,
                    text = CASE WHEN excluded.text = '' THEN annotations.text ELSE excluded.text END",
            )?
            .execute(rusqlite::params![
                identity,
                a.kind.as_str(),
                a.cfi,
                a.color,
                a.text,
                a.note,
                a.chapter,
                a.fraction,
                a.created_at,
                a.updated_at
            ])?;
        self.conn().query_row(
            "SELECT id FROM annotations WHERE identity = ?1 AND kind = ?2 AND cfi = ?3",
            rusqlite::params![identity, a.kind.as_str(), a.cfi],
            |r| r.get(0),
        )
    }

    pub fn delete_annotation(&self, identity: &str, id: i64) -> Result<(), Error> {
        self.conn().prepare_cached("DELETE FROM annotations WHERE identity = ?1 AND id = ?2")?.execute(rusqlite::params![identity, id])?;
        Ok(())
    }
}

/// The book's highlights and notes as Markdown, by chapter.
pub fn to_markdown(title: &str, authors: &[String], annotations: &[Annotation]) -> String {
    let mut out = format!("# {title}\n");
    if !authors.is_empty() {
        out.push_str(&format!("\n*{}*\n", authors.join(", ")));
    }
    let mut chapter: Option<&str> = None;
    for a in annotations.iter().filter(|a| a.kind == Kind::Highlight) {
        if chapter != Some(a.chapter.as_str()) {
            chapter = Some(&a.chapter);
            let heading = if a.chapter.is_empty() { "Notes" } else { &a.chapter };
            out.push_str(&format!("\n## {heading}\n"));
        }
        let quote = a.text.trim().lines().map(|l| format!("> {l}")).collect::<Vec<_>>().join("\n");
        out.push_str(&format!("\n{quote}\n"));
        if !a.note.trim().is_empty() {
            out.push_str(&format!("\n{}\n", a.note.trim()));
        }
    }
    out
}

#[derive(Serialize)]
struct Export<'a> {
    title: &'a str,
    authors: &'a [String],
    identity: &'a str,
    annotations: &'a [Annotation],
}

pub fn to_json(book: &Book, annotations: &[Annotation]) -> String {
    serde_json::to_string_pretty(&Export { title: &book.title, authors: &book.authors, identity: &book.identity, annotations })
        .unwrap_or_default()
}

/// Where exports go: `~/Documents/Omaread`.
pub fn export_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join("Documents/Omaread")
}

/// A file name from a title, safe on any filesystem.
pub fn file_name(title: &str, ext: &str) -> String {
    let clean: String = title
        .chars()
        .map(|c| if c.is_alphanumeric() || " -_,.'()".contains(c) { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let clean = clean.trim_matches('.');
    let clean: String = clean.chars().take(120).collect();
    format!("{}.{ext}", if clean.is_empty() { "book" } else { &clean })
}

// --- Foliate import ---------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default)]
struct FoliateBook {
    metadata: FoliateMetadata,
    progress: Option<(f64, f64)>,
    #[serde(rename = "lastLocation")]
    last_location: Option<String>,
    annotations: Vec<FoliateAnnotation>,
    bookmarks: Vec<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct FoliateMetadata {
    identifier: Option<String>,
    title: Option<serde_json::Value>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct FoliateAnnotation {
    value: String,
    color: String,
    text: String,
    note: String,
    created: String,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ImportReport {
    pub books: usize,
    pub positions: usize,
    pub annotations: usize,
    pub bookmarks: usize,
    pub unmatched: usize,
}

pub fn foliate_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"))
        .join("com.github.johnfactotum.Foliate")
}

/// Bring Foliate's reading positions, highlights and bookmarks in. Matches
/// books by identifier, then by title. Never overwrites a position Omaread
/// already has; annotations already here are left as they are.
pub fn import_foliate(db: &Db, dir: &Path, books: &[Book], now: i64) -> Result<ImportReport, Error> {
    let mut report = ImportReport::default();
    let Ok(entries) = std::fs::read_dir(dir) else { return Ok(report) };

    let by_identity: HashMap<&str, &Book> = books.iter().map(|b| (b.identity.as_str(), b)).collect();
    let by_title: HashMap<String, &Book> = books.iter().map(|b| (b.title.trim().to_lowercase(), b)).collect();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let Ok(fb) = serde_json::from_str::<FoliateBook>(&text) else { continue };

        let identifier = fb.metadata.identifier.clone().or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()));
        let title = match &fb.metadata.title {
            Some(serde_json::Value::String(s)) => Some(s.trim().to_lowercase()),
            Some(serde_json::Value::Object(m)) => m.values().next().and_then(|v| v.as_str()).map(|s| s.trim().to_lowercase()),
            _ => None,
        };
        let book = identifier
            .as_deref()
            .and_then(|id| by_identity.get(format!("id:{id}").as_str()).copied())
            .or_else(|| title.as_ref().and_then(|t| by_title.get(t).copied()));
        let Some(book) = book else {
            report.unmatched += 1;
            continue;
        };
        report.books += 1;

        if let Some(cfi) = fb.last_location.filter(|c| c.starts_with("epubcfi(")) {
            let existing = db.progress(&book.identity)?;
            if existing.is_none() {
                let fraction = fb.progress.filter(|(_, t)| *t > 0.0).map(|(c, t)| (c / t).clamp(0.0, 1.0)).unwrap_or(0.0);
                db.save_progress(&book.identity, &Progress { cfi: Some(cfi), fraction, opened_at: 0, ..Default::default() })?;
                report.positions += 1;
            }
        }

        let existing: Vec<String> = db.annotations(&book.identity)?.into_iter().map(|a| a.cfi).collect();
        for fa in fb.annotations.into_iter().filter(|a| a.value.starts_with("epubcfi(")) {
            if existing.contains(&fa.value) {
                continue;
            }
            let created = parse_iso_seconds(&fa.created).unwrap_or(now);
            let color = if COLORS.contains(&fa.color.as_str()) { fa.color } else { "yellow".into() };
            db.save_annotation(
                &book.identity,
                &Annotation {
                    id: 0,
                    kind: Kind::Highlight,
                    cfi: fa.value,
                    color,
                    text: fa.text,
                    note: fa.note,
                    chapter: String::new(),
                    fraction: 0.0,
                    created_at: created,
                    updated_at: created,
                },
            )?;
            report.annotations += 1;
        }
        for cfi in fb.bookmarks.into_iter().filter(|c| c.starts_with("epubcfi(")) {
            if existing.contains(&cfi) {
                continue;
            }
            db.save_annotation(
                &book.identity,
                &Annotation {
                    id: 0,
                    kind: Kind::Bookmark,
                    cfi,
                    color: String::new(),
                    text: String::new(),
                    note: String::new(),
                    chapter: String::new(),
                    fraction: 0.0,
                    created_at: now,
                    updated_at: now,
                },
            )?;
            report.bookmarks += 1;
        }
    }
    Ok(report)
}

/// `2026-09-17T02:40:36.502Z` → seconds since the epoch. Enough of ISO 8601
/// for what Foliate writes.
fn parse_iso_seconds(s: &str) -> Option<i64> {
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let time = time.trim_end_matches('Z');
    let mut t = time.split(':');
    let (hh, mm) = (t.next()?.parse::<i64>().ok()?, t.next()?.parse::<i64>().ok()?);
    let ss = t.next().and_then(|s| s.split('.').next()?.parse::<i64>().ok()).unwrap_or(0);
    // Days from civil (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hh * 3600 + mm * 60 + ss)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn highlight(cfi: &str, chapter: &str, text: &str, note: &str) -> Annotation {
        Annotation {
            id: 0,
            kind: Kind::Highlight,
            cfi: cfi.into(),
            color: "yellow".into(),
            text: text.into(),
            note: note.into(),
            chapter: chapter.into(),
            fraction: 0.1,
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn saving_twice_updates_in_place() {
        let db = Db::in_memory().unwrap();
        let a = highlight("epubcfi(/6/2!/4,/1:0,/1:5)", "One", "Hello", "");
        let id = db.save_annotation("b", &a).unwrap();
        let again = db
            .save_annotation("b", &Annotation { note: "mine".into(), color: "blue".into(), text: String::new(), chapter: "Two".into(), ..a })
            .unwrap();
        assert_eq!(id, again);
        let all = db.annotations("b").unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].note, "mine");
        assert_eq!(all[0].color, "blue");
        assert_eq!(all[0].text, "Hello");
        assert_eq!(all[0].chapter, "Two");
        db.delete_annotation("b", id).unwrap();
        assert!(db.annotations("b").unwrap().is_empty());
    }

    #[test]
    fn markdown_groups_by_chapter() {
        let md = to_markdown(
            "Book",
            &["Ann".into()],
            &[highlight("a", "One", "first", "a thought"), highlight("b", "One", "second", ""), highlight("c", "Two", "third", "")],
        );
        assert_eq!(md.matches("## One").count(), 1);
        assert!(md.contains("> first\n\na thought"));
        assert!(md.contains("## Two\n\n> third"));
    }

    #[test]
    fn file_names_are_safe() {
        assert_eq!(file_name("Go: The/Good Parts?", "md"), "Go The Good Parts.md");
        assert_eq!(file_name("///", "md"), "book.md");
    }

    #[test]
    fn parses_foliate_timestamps() {
        assert_eq!(parse_iso_seconds("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(parse_iso_seconds("2026-09-17T02:40:36.502Z"), Some(1_789_612_836));
    }

    #[test]
    fn imports_foliate_books_by_identifier() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("978.json"),
            r#"{"metadata":{"identifier":"978","title":"T"},"progress":[50,100],"lastLocation":"epubcfi(/6/4!/4/2)",
                "annotations":[{"value":"epubcfi(/6/4!/4/2,/1:0,/1:4)","color":"yellow","text":"word","note":"n","created":"2026-09-17T02:40:36.502Z","modified":""}],
                "bookmarks":["epubcfi(/6/8!/4/2)"]}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("other.json"), r#"{"metadata":{"identifier":"nope","title":"Unknown"}}"#).unwrap();
        let book = Book {
            path: "/b.epub".into(),
            identity: "id:978".into(),
            title: "T".into(),
            authors: vec![],
            language: None,
            series: None,
            series_index: None,
            shelf: String::new(),
            cover: None,
            error: None,
            added_at: 0,
            progress: None,
        };
        let db = Db::in_memory().unwrap();
        let report = import_foliate(&db, dir.path(), &[book.clone()], 5).unwrap();
        assert_eq!(report, ImportReport { books: 1, positions: 1, annotations: 1, bookmarks: 1, unmatched: 1 });
        assert_eq!(db.progress("id:978").unwrap().unwrap().fraction, 0.5);
        // Importing again changes nothing.
        let again = import_foliate(&db, dir.path(), &[book], 5).unwrap();
        assert_eq!(again.annotations + again.bookmarks + again.positions, 0);
    }
}

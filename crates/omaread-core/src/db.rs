//! The SQLite store: what's in the library, and where you are in each book.
//!
//! Books are keyed by path (a file is a row); reading state is keyed by the
//! book's identity, so moving or renaming a file keeps your place.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

pub use rusqlite::Error;

const MIGRATIONS: &[&str] = &[
    // 1: library and progress
    r#"
    CREATE TABLE books (
        path          TEXT PRIMARY KEY,
        identity      TEXT NOT NULL,
        size          INTEGER NOT NULL,
        mtime         INTEGER NOT NULL,
        title         TEXT NOT NULL,
        authors       TEXT NOT NULL,
        language      TEXT,
        series        TEXT,
        series_index  REAL,
        shelf         TEXT NOT NULL,
        cover         TEXT,
        error         TEXT,
        added_at      INTEGER NOT NULL
    );
    CREATE INDEX books_identity ON books(identity);
    CREATE TABLE progress (
        identity   TEXT PRIMARY KEY,
        cfi        TEXT,
        fraction   REAL NOT NULL DEFAULT 0,
        opened_at  INTEGER NOT NULL
    );
    "#,
    // 2: small app-wide values (reading speed, …)
    r#"
    CREATE TABLE settings (
        key    TEXT PRIMARY KEY,
        value  TEXT NOT NULL
    );
    "#,
];

pub struct Db {
    conn: Connection,
}

/// A row of `books`, as the scanner writes it.
#[derive(Debug, Clone, PartialEq)]
pub struct BookRow {
    pub path: String,
    pub identity: String,
    pub size: i64,
    pub mtime: i64,
    pub title: String,
    pub authors: Vec<String>,
    pub language: Option<String>,
    pub series: Option<String>,
    pub series_index: Option<f64>,
    pub shelf: String,
    pub cover: Option<String>,
    pub error: Option<String>,
    pub added_at: i64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Progress {
    pub cfi: Option<String>,
    pub fraction: f64,
    pub opened_at: i64,
}

/// Authors are stored as one string; this byte can't appear in a name.
const SEP: char = '\u{1f}';

impl Db {
    pub fn open(path: &Path) -> Result<Db, Error> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        Db::init(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Db, Error> {
        Db::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Db, Error> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version.max(0) as usize) {
            conn.execute_batch(&format!("BEGIN; {sql}; PRAGMA user_version = {}; COMMIT;", i + 1))?;
        }
        Ok(Db { conn })
    }

    /// `(path, size, mtime)` for every known book, to skip unchanged files.
    pub fn stamps(&self) -> Result<Vec<(String, i64, i64)>, Error> {
        let mut stmt = self.conn.prepare_cached("SELECT path, size, mtime FROM books")?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect()
    }

    pub fn books(&self) -> Result<Vec<BookRow>, Error> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT path, identity, size, mtime, title, authors, language, series, series_index,
                    shelf, cover, error, added_at FROM books",
        )?;
        stmt.query_map([], |r| {
            let authors: String = r.get(5)?;
            Ok(BookRow {
                path: r.get(0)?,
                identity: r.get(1)?,
                size: r.get(2)?,
                mtime: r.get(3)?,
                title: r.get(4)?,
                authors: authors.split(SEP).filter(|a| !a.is_empty()).map(String::from).collect(),
                language: r.get(6)?,
                series: r.get(7)?,
                series_index: r.get(8)?,
                shelf: r.get(9)?,
                cover: r.get(10)?,
                error: r.get(11)?,
                added_at: r.get(12)?,
            })
        })?
        .collect()
    }

    /// Insert or refresh books, and forget paths that are gone, in one transaction.
    pub fn sync(&mut self, upserts: &[BookRow], removed: &[String]) -> Result<(), Error> {
        let tx = self.conn.transaction()?;
        {
            let mut up = tx.prepare_cached(
                "INSERT INTO books (path, identity, size, mtime, title, authors, language, series,
                                    series_index, shelf, cover, error, added_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                 ON CONFLICT(path) DO UPDATE SET
                    identity = excluded.identity, size = excluded.size, mtime = excluded.mtime,
                    title = excluded.title, authors = excluded.authors, language = excluded.language,
                    series = excluded.series, series_index = excluded.series_index,
                    shelf = excluded.shelf, cover = excluded.cover, error = excluded.error",
            )?;
            for b in upserts {
                let authors = b.authors.join(&SEP.to_string());
                up.execute(params![
                    b.path, b.identity, b.size, b.mtime, b.title, authors, b.language, b.series,
                    b.series_index, b.shelf, b.cover, b.error, b.added_at
                ])?;
            }
            let mut del = tx.prepare_cached("DELETE FROM books WHERE path = ?1")?;
            for path in removed {
                del.execute([path])?;
            }
        }
        tx.commit()
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>, Error> {
        self.conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), Error> {
        self.conn
            .prepare_cached("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value")?
            .execute([key, value])?;
        Ok(())
    }

    pub fn progress(&self, identity: &str) -> Result<Option<Progress>, Error> {
        self.conn
            .query_row(
                "SELECT cfi, fraction, opened_at FROM progress WHERE identity = ?1",
                [identity],
                |r| Ok(Progress { cfi: r.get(0)?, fraction: r.get(1)?, opened_at: r.get(2)? }),
            )
            .optional()
    }

    pub fn all_progress(&self) -> Result<Vec<(String, Progress)>, Error> {
        let mut stmt = self.conn.prepare_cached("SELECT identity, cfi, fraction, opened_at FROM progress")?;
        stmt.query_map([], |r| Ok((r.get(0)?, Progress { cfi: r.get(1)?, fraction: r.get(2)?, opened_at: r.get(3)? })))?
            .collect()
    }

    pub fn save_progress(&self, identity: &str, p: &Progress) -> Result<(), Error> {
        self.conn
            .prepare_cached(
                "INSERT INTO progress (identity, cfi, fraction, opened_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(identity) DO UPDATE SET
                    cfi = excluded.cfi, fraction = excluded.fraction, opened_at = excluded.opened_at",
            )?
            .execute(params![identity, p.cfi, p.fraction, p.opened_at])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(path: &str) -> BookRow {
        BookRow {
            path: path.into(),
            identity: format!("id:{path}"),
            size: 1,
            mtime: 2,
            title: "T".into(),
            authors: vec!["A".into(), "B".into()],
            language: Some("en".into()),
            series: None,
            series_index: None,
            shelf: "Go".into(),
            cover: None,
            error: None,
            added_at: 3,
        }
    }

    #[test]
    fn sync_inserts_updates_and_removes() {
        let mut db = Db::in_memory().unwrap();
        db.sync(&[row("a"), row("b")], &[]).unwrap();
        let mut b = row("b");
        b.title = "New".into();
        db.sync(&[b], &["a".into()]).unwrap();
        let books = db.books().unwrap();
        assert_eq!(books.len(), 1);
        assert_eq!(books[0].title, "New");
        assert_eq!(books[0].authors, vec!["A", "B"]);
        // An update keeps the original added_at.
        assert_eq!(books[0].added_at, 3);
    }

    #[test]
    fn progress_round_trips() {
        let db = Db::in_memory().unwrap();
        assert_eq!(db.progress("x").unwrap(), None);
        let p = Progress { cfi: Some("epubcfi(/6/4)".into()), fraction: 0.25, opened_at: 10 };
        db.save_progress("x", &p).unwrap();
        assert_eq!(db.progress("x").unwrap(), Some(p));
    }

    #[test]
    fn settings_round_trip() {
        let db = Db::in_memory().unwrap();
        assert_eq!(db.setting("k").unwrap(), None);
        db.set_setting("k", "1").unwrap();
        db.set_setting("k", "2").unwrap();
        assert_eq!(db.setting("k").unwrap().as_deref(), Some("2"));
    }

    #[test]
    fn reopening_runs_no_migration_twice() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("o.db");
        Db::open(&path).unwrap();
        Db::open(&path).unwrap();
    }
}

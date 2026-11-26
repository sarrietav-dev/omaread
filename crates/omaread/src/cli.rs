//! The command line for scripts and agents: read the library, a book's
//! contents and text, and the annotations, without a window.
//!
//! Conventions every command follows, so an agent can rely on them:
//! * `--json` prints one JSON object on stdout; anything else goes to stderr.
//! * Errors exit non-zero: 2 for a bad request (usage, no such book, an
//!   ambiguous name), 1 when something failed. With `--json` the error is a
//!   JSON object on stderr: `{"error": {"code": …, "message": …, "hint": …}}`.
//! * Nothing prompts or pages. Long output is cut with `--limit` (lists) or
//!   `--max-chars` (text), and the JSON says so (`truncated`, `next`).
//! * A book is named by path, by id (`omaread list --json` prints both), or
//!   by part of its title or author.
//! * `omaread commands --json` describes every command and its options.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use omaread_core::annotations::{Annotation, Kind};
use omaread_core::db::Db;
use omaread_core::library::{self, Book, Paths};
use omaread_core::text::BookText;
use serde_json::{Value, json};

/// Exit codes.
const FAILED: u8 = 1;
const BAD_REQUEST: u8 = 2;

const DEFAULT_LIMIT: usize = 50;
const DEFAULT_MAX_CHARS: usize = 20_000;

pub struct Error {
    pub exit: u8,
    code: &'static str,
    message: String,
    hint: Option<String>,
}

impl Error {
    fn bad(code: &'static str, message: impl Into<String>, hint: Option<String>) -> Error {
        Error { exit: BAD_REQUEST, code, message: message.into(), hint }
    }

    fn failed(message: impl Into<String>) -> Error {
        Error { exit: FAILED, code: "failed", message: message.into(), hint: None }
    }

    pub fn report(&self, json: bool) {
        if json {
            eprintln!("{}", json!({ "error": { "code": self.code, "message": self.message, "hint": self.hint } }));
        } else {
            eprintln!("omaread: {}", self.message);
            if let Some(hint) = &self.hint {
                eprintln!("  {hint}");
            }
        }
    }
}

/// Positional arguments and `--flags`, parsed against the flags that take a value.
pub struct Args {
    pub positional: Vec<String>,
    flags: HashMap<String, Option<String>>,
}

impl Args {
    pub fn parse(args: &[String], valued: &[&str]) -> Result<Args, Error> {
        let mut positional = Vec::new();
        let mut flags = HashMap::new();
        let mut it = args.iter();
        while let Some(a) = it.next() {
            if a == "--" {
                positional.extend(it.by_ref().cloned());
                break;
            }
            let Some(name) = a.strip_prefix("--") else {
                positional.push(a.clone());
                continue;
            };
            let (name, inline) = match name.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (name, None),
            };
            if valued.contains(&name) {
                let value = inline.or_else(|| it.next().cloned());
                if value.is_none() {
                    return Err(Error::bad("usage", format!("--{name} needs a value"), None));
                }
                flags.insert(name.to_string(), value);
            } else {
                flags.insert(name.to_string(), None);
            }
        }
        Ok(Args { positional, flags })
    }

    pub fn has(&self, name: &str) -> bool {
        self.flags.contains_key(name)
    }

    pub fn value(&self, name: &str) -> Option<&str> {
        self.flags.get(name).and_then(|v| v.as_deref())
    }

    fn number(&self, name: &str) -> Result<Option<usize>, Error> {
        match self.value(name) {
            None => Ok(None),
            Some(v) => v.parse().map(Some).map_err(|_| Error::bad("usage", format!("--{name} takes a number, not {v}"), None)),
        }
    }

    fn json(&self) -> bool {
        self.has("json")
    }
}

/// A command's description, for `omaread commands`.
struct Spec {
    name: &'static str,
    usage: &'static str,
    about: &'static str,
    options: &'static [(&'static str, &'static str)],
}

const COMMANDS: &[Spec] = &[
    Spec { name: "list", usage: "omaread list [--reading|--finished|--unread] [--query TEXT] [--limit N] [--json]", about: "Books in the library, most recently opened first.", options: &[
        ("--reading", "only books started and not finished"), ("--finished", "only finished books"), ("--unread", "only books never opened"),
        ("--query TEXT", "title, author or series contains TEXT"), ("--limit N", "at most N books (default 50)"), ("--json", "JSON output"),
    ]},
    Spec { name: "show", usage: "omaread show <book> [--json]", about: "One book: metadata, where you are, its chapters and annotation counts.", options: &[("--json", "JSON output")] },
    Spec { name: "toc", usage: "omaread toc <book> [--json]", about: "The table of contents: each top-level entry's position and number (what --chapter takes) and its section.", options: &[("--json", "JSON output")] },
    Spec { name: "text", usage: "omaread text <book> [--chapter N|--section N|--current] [--offset N] [--max-chars N] [--json]", about: "Clean text of a chapter (counted as :ch counts) or a spine section. With no selector, the chapter you are in.", options: &[
        ("--chapter N", "the contents entry numbered N (\"3. …\", \"Chapter 3\"), else the N-th top-level entry; as :ch counts"), ("--section N", "the n-th document in the spine"), ("--current", "the chapter you are reading (the default)"),
        ("--offset N", "skip the first N characters (for paging)"), ("--max-chars N", "at most N characters (default 20000)"), ("--json", "JSON output"),
    ]},
    Spec { name: "search", usage: "omaread search <book> <text…> [--limit N] [--json]", about: "Find text in a book; each hit has its chapter, section and surrounding words.", options: &[
        ("--case", "match case"), ("--limit N", "at most N hits (default 50)"), ("--json", "JSON output"),
    ]},
    Spec { name: "annotations", usage: "omaread annotations [<book>] [--query TEXT] [--kind highlight|bookmark|all] [--limit N] [--json]", about: "Highlights, notes and bookmarks of one book, or of the whole library when no book is named.", options: &[
        ("--query TEXT", "highlight text, note, chapter or book title contains TEXT"), ("--kind K", "highlight (default), bookmark or all"),
        ("--limit N", "at most N (default 50)"), ("--json", "JSON output"),
    ]},
    Spec { name: "open", usage: "omaread open <book> [--at CFI|--fraction F|--chapter N]", about: "Open the book in the app (the running one, if any), optionally at a place.", options: &[
        ("--at CFI", "an EPUB CFI, as annotations print it"), ("--fraction F", "a position from 0 to 1"), ("--chapter N", "the contents entry numbered N, else the N-th"),
    ]},
    Spec { name: "status", usage: "omaread status [--json]", about: "What you are reading: title, chapter, progress, time left.", options: &[("--json", "JSON output")] },
    Spec { name: "stats", usage: "omaread stats [--days N] [--json]", about: "Reading time per day, streaks, the books read most and the books finished.", options: &[("--days N", "days of history (default 14)"), ("--json", "JSON output")] },
    Spec { name: "lookup", usage: "omaread lookup <key> <text…> [--lang xx]", about: "What the lookup card would show for a provider key (d, w, t, …).", options: &[("--lang xx", "the text's language")] },
    Spec { name: "scan", usage: "omaread scan", about: "Refresh the library without opening a window.", options: &[] },
    Spec { name: "check-library", usage: "omaread check-library [--json]", about: "Open every book and report the ones that won't.", options: &[("--json", "JSON output")] },
    Spec { name: "import-foliate", usage: "omaread import-foliate", about: "Bring Foliate's positions and annotations over.", options: &[] },
    Spec { name: "commands", usage: "omaread commands [--json]", about: "This list.", options: &[("--json", "JSON output")] },
];

/// The commands this module answers. `None` for anything else.
pub fn run(command: &str, args: &[String]) -> Option<u8> {
    let valued = ["query", "limit", "chapter", "section", "offset", "max-chars", "kind", "at", "fraction", "days"];
    let result = Args::parse(args, &valued).and_then(|args| {
        let out = match command {
            "commands" => Ok(commands(&args)),
            "list" => list(&args),
            "show" => show(&args),
            "toc" => toc(&args),
            "text" => text(&args),
            "search" => search(&args),
            "annotations" => annotations(&args),
            "stats" => stats(&args),
            _ => return Ok(None),
        };
        out.map(|o| Some((o, args.json())))
    });
    match result {
        Ok(None) => None,
        Ok(Some((Output { json, text }, as_json))) => {
            if as_json {
                println!("{json}");
            } else {
                print!("{text}");
            }
            Some(0)
        }
        Err(e) => {
            e.report(args.iter().any(|a| a == "--json"));
            Some(e.exit)
        }
    }
}

pub struct Output {
    json: Value,
    text: String,
}

fn commands(_: &Args) -> Output {
    let json = json!({
        "commands": COMMANDS.iter().map(|c| json!({
            "name": c.name, "usage": c.usage, "about": c.about,
            "options": c.options.iter().map(|(o, a)| json!({ "option": o, "about": a })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "conventions": {
            "json": "--json prints one object on stdout; logs and errors go to stderr",
            "exit": { "0": "ok", "1": "failed", "2": "bad request: usage, no such book, ambiguous name" },
            "book": "a path, an id from `list --json`, or part of the title or author",
        },
    });
    let mut text = String::new();
    for c in COMMANDS {
        text.push_str(&format!("{}\n    {}\n", c.usage, c.about));
    }
    text.push_str("\nBooks are named by path, id, or part of the title or author. --json for machine-readable output.\n");
    Output { json, text }
}

fn open_db() -> Result<Db, Error> {
    Paths::from_env().open_db().map_err(|e| Error::failed(format!("no library database: {e}")))
}

fn books(db: &Db) -> Result<Vec<Book>, Error> {
    library::load(db).map_err(|e| Error::failed(e.to_string()))
}

fn book_json(b: &Book) -> Value {
    let p = b.progress.clone().unwrap_or_default();
    json!({
        "id": b.identity,
        "title": b.title,
        "authors": b.authors,
        "series": b.series,
        "seriesIndex": b.series_index,
        "language": b.language,
        "path": b.path,
        "shelf": b.shelf,
        "error": b.error,
        "fraction": b.fraction(),
        "chapter": p.chapter,
        "minutesLeft": p.minutes_left,
        "openedAt": b.progress.as_ref().map(|p| p.opened_at),
        "addedAt": b.added_at,
    })
}

/// A book from what the user typed: a path, an id, or part of a title or author.
pub fn resolve(db: &Db, name: &str) -> Result<Book, Error> {
    let all = books(db)?;
    let path = Path::new(name);
    if path.is_file() {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if let Some(b) = all.iter().find(|b| b.path == canonical || b.path == path) {
            return Ok(b.clone());
        }
        return Ok(book_from_file(db, &canonical));
    }
    if let Some(b) = all.iter().find(|b| b.identity == name) {
        return Ok(b.clone());
    }
    let needle = name.to_lowercase();
    let matches: Vec<&Book> = all
        .iter()
        .filter(|b| b.title.to_lowercase().contains(&needle) || b.authors.iter().any(|a| a.to_lowercase().contains(&needle)))
        .collect();
    match matches.as_slice() {
        [] => Err(Error::bad("not-found", format!("no book matches “{name}”"), Some("omaread list --query <text> shows what is in the library".into()))),
        [one] => Ok((*one).clone()),
        many => {
            if let Some(exact) = many.iter().find(|b| b.title.to_lowercase() == needle) {
                return Ok((*exact).clone());
            }
            let names: Vec<String> = many.iter().take(10).map(|b| format!("{} ({})", b.title, b.identity)).collect();
            Err(Error::bad(
                "ambiguous",
                format!("{} books match “{name}”: {}", many.len(), names.join("; ")),
                Some("name the book by its id or path".into()),
            ))
        }
    }
}

fn book_from_file(db: &Db, path: &Path) -> Book {
    let (identity, meta) = library::identify(path);
    let meta = meta.unwrap_or_default();
    Book {
        path: path.to_path_buf(),
        progress: db.progress(&identity).ok().flatten(),
        identity,
        title: if meta.title.is_empty() { path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default() } else { meta.title },
        authors: meta.authors,
        language: meta.language,
        series: meta.series,
        series_index: meta.series_index,
        shelf: String::new(),
        cover: None,
        error: None,
        added_at: 0,
    }
}

fn book_arg<'a>(args: &'a Args, usage: &str) -> Result<&'a str, Error> {
    args.positional.first().map(String::as_str).ok_or_else(|| Error::bad("usage", format!("usage: {usage}"), None))
}

fn open_text(book: &Book) -> Result<BookText, Error> {
    if let Some(e) = &book.error {
        return Err(Error::failed(format!("“{}” can't be read: {e}", book.title)));
    }
    BookText::open(&book.path).map_err(|e| Error::failed(format!("can't read “{}”: {e}", book.title)))
}

fn list(args: &Args) -> Result<Output, Error> {
    let db = open_db()?;
    let mut all = books(&db)?;
    if args.has("reading") {
        all.retain(Book::in_progress);
    }
    if args.has("finished") {
        all.retain(|b| b.fraction() >= 0.995);
    }
    if args.has("unread") {
        all.retain(|b| b.progress.is_none());
    }
    if let Some(q) = args.value("query").map(str::to_lowercase) {
        all.retain(|b| {
            b.title.to_lowercase().contains(&q)
                || b.authors.iter().any(|a| a.to_lowercase().contains(&q))
                || b.series.as_deref().is_some_and(|s| s.to_lowercase().contains(&q))
        });
    }
    all.sort_by(|a, b| b.opened_at().cmp(&a.opened_at()).then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase())));
    let total = all.len();
    let limit = args.number("limit")?.unwrap_or(DEFAULT_LIMIT);
    all.truncate(limit);
    let mut text = String::new();
    for b in &all {
        let by = if b.authors.is_empty() { String::new() } else { format!(" — {}", b.authors.join(", ")) };
        text.push_str(&format!("{:>4.0}%  {}{by}\n", b.fraction() * 100.0, b.title));
    }
    if total > all.len() {
        text.push_str(&format!("… {} more (--limit)\n", total - all.len()));
    }
    Ok(Output { json: json!({ "books": all.iter().map(book_json).collect::<Vec<_>>(), "total": total, "truncated": total > all.len() }), text })
}

fn chapters_json(book: &BookText) -> Vec<Value> {
    book.chapters()
        .iter()
        .enumerate()
        .map(|(i, c)| json!({ "position": i + 1, "number": omaread_core::text::label_number(&c.label), "label": c.label, "section": c.section }))
        .collect()
}

/// The `--chapter` value for the chapter a progress label is in: its
/// number if its label has one, else its position.
fn current_chapter(text: &BookText, label: Option<&str>) -> Option<usize> {
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    let label = norm(label?);
    let entry = text.toc.iter().find(|e| norm(&e.label) == label)?;
    let section = entry.section?;
    let chapters = text.chapters();
    let at = chapters.iter().rposition(|c| c.section.is_some_and(|s| s <= section))?;
    let n = omaread_core::text::label_number(&chapters[at].label).unwrap_or(at + 1);
    // A number that points elsewhere (two entries numbered alike) falls back to the position.
    Some(if text.chapter_position(n) == Some(at) { n } else { at + 1 })
}

fn show(args: &Args) -> Result<Output, Error> {
    let db = open_db()?;
    let book = resolve(&db, book_arg(args, "omaread show <book>")?)?;
    let notes = db.annotations(&book.identity).unwrap_or_default();
    let highlights = notes.iter().filter(|a| a.kind == Kind::Highlight).count();
    let bookmarks = notes.len() - highlights;
    let text = open_text(&book).ok();
    let chapters = text.as_ref().map(chapters_json).unwrap_or_default();
    let current = text.as_ref().and_then(|t| current_chapter(t, book.progress.as_ref().and_then(|p| p.chapter.as_deref())));
    let mut json = book_json(&book);
    json["sections"] = json!(text.as_ref().map(|t| t.sections.len()));
    json["chapters"] = json!(chapters);
    json["currentChapter"] = json!(current);
    json["highlights"] = json!(highlights);
    json["bookmarks"] = json!(bookmarks);
    let mut out = format!("{}\n", book.title);
    if !book.authors.is_empty() {
        out.push_str(&format!("by {}\n", book.authors.join(", ")));
    }
    if let Some(series) = &book.series {
        out.push_str(&format!("series: {series}{}\n", book.series_index.map(|i| format!(" #{i}")).unwrap_or_default()));
    }
    out.push_str(&format!("{:.0}% read", book.fraction() * 100.0));
    if let Some(ch) = book.progress.as_ref().and_then(|p| p.chapter.as_deref()) {
        out.push_str(&format!(", in “{ch}”"));
    }
    out.push_str(&format!("\n{highlights} highlights, {bookmarks} bookmarks\nid: {}\npath: {}\n", book.identity, book.path.display()));
    if let Some(t) = &text {
        out.push_str(&format!("\n{} chapters, {} sections\n", t.chapters().len(), t.sections.len()));
    }
    Ok(Output { json, text: out })
}

fn toc(args: &Args) -> Result<Output, Error> {
    let db = open_db()?;
    let book = resolve(&db, book_arg(args, "omaread toc <book>")?)?;
    let text = open_text(&book)?;
    let top = text.toc.iter().map(|e| e.depth).min().unwrap_or(0);
    let mut chapter = 0;
    let mut entries = Vec::new();
    let mut out = String::new();
    for e in &text.toc {
        if e.depth == top && e.section.is_some() {
            chapter += 1;
        }
        let n = (e.depth == top && e.section.is_some()).then_some(chapter);
        let number = n.map(|_| omaread_core::text::label_number(&e.label));
        entries.push(json!({ "label": e.label, "depth": e.depth - top, "position": n, "number": number.flatten(), "section": e.section, "href": e.href }));
        let number = n.map(|n| format!("{n:>3}. ")).unwrap_or_else(|| "     ".into());
        out.push_str(&format!("{number}{}{}\n", "  ".repeat(e.depth - top), e.label));
    }
    if entries.is_empty() {
        out.push_str("This book has no table of contents; use --section with omaread text.\n");
    }
    Ok(Output { json: json!({ "book": book_json(&book), "toc": entries, "sections": text.sections.len() }), text: out })
}

fn text(args: &Args) -> Result<Output, Error> {
    let db = open_db()?;
    let book = resolve(&db, book_arg(args, "omaread text <book> [--chapter N|--section N]")?)?;
    let mut text = open_text(&book)?;
    let (range, label, chapter) = if let Some(n) = args.number("section")? {
        if n == 0 || n > text.sections.len() {
            return Err(Error::bad("not-found", format!("there is no section {n}; the book has {}", text.sections.len()), None));
        }
        (n..n + 1, text.label_for(n).map(String::from), None)
    } else {
        let n = match args.number("chapter")? {
            Some(n) => n,
            None => current_chapter(&text, book.progress.as_ref().and_then(|p| p.chapter.as_deref())).ok_or_else(|| {
                Error::bad("usage", "which chapter? You haven't opened this book, or its place isn't in the contents", Some("pass --chapter N or --section N; omaread toc lists them".into()))
            })?,
        };
        let range = text.chapter_sections(n).ok_or_else(|| {
            Error::bad("not-found", format!("there is no chapter {n}; the book has {}", text.chapters().len()), Some("omaread toc lists the chapters".into()))
        })?;
        let label = text.chapter_position(n).and_then(|i| text.chapters().get(i).map(|c| c.label.clone()));
        (range, label, Some(n))
    };
    let mut body = String::new();
    for i in range.clone() {
        let part = text.section_text(i).map_err(|e| Error::failed(e.to_string()))?;
        if !part.is_empty() {
            if !body.is_empty() {
                body.push_str("\n\n");
            }
            body.push_str(&part);
        }
    }
    let total = body.chars().count();
    let offset = args.number("offset")?.unwrap_or(0).min(total);
    let max = args.number("max-chars")?.unwrap_or(DEFAULT_MAX_CHARS);
    let shown: String = body.chars().skip(offset).take(max).collect();
    let end = offset + shown.chars().count();
    let truncated = end < total;
    let json = json!({
        "book": { "id": book.identity, "title": book.title },
        "chapter": chapter,
        "label": label,
        "sections": [range.start, range.end - 1],
        "offset": offset,
        "chars": total,
        "truncated": truncated,
        "next": truncated.then_some(end),
        "text": shown,
    });
    let mut out = shown;
    out.push('\n');
    if truncated {
        out.push_str(&format!("\n[… {} more characters; --offset {end}]\n", total - end));
    }
    Ok(Output { json, text: out })
}

fn search(args: &Args) -> Result<Output, Error> {
    let db = open_db()?;
    let usage = "omaread search <book> <text…>";
    let book = resolve(&db, book_arg(args, usage)?)?;
    let query = args.positional[1..].join(" ");
    if query.trim().is_empty() {
        return Err(Error::bad("usage", format!("usage: {usage}"), None));
    }
    let case = args.has("case");
    let needle = if case { query.clone() } else { query.to_lowercase() };
    let limit = args.number("limit")?.unwrap_or(DEFAULT_LIMIT);
    let mut text = open_text(&book)?;
    let mut hits = Vec::new();
    let mut total = 0usize;
    let sections: Vec<usize> = text.sections.iter().map(|s| s.index).collect();
    for i in sections {
        let Ok(body) = text.section_text(i) else { continue };
        let hay = if case { body.clone() } else { body.to_lowercase() };
        // Lowercasing can change byte lengths; map back through char indices.
        let hay_chars: Vec<char> = hay.chars().collect();
        let body_chars: Vec<char> = body.chars().collect();
        let needle_chars: Vec<char> = needle.chars().collect();
        if hay_chars.len() != body_chars.len() {
            continue;
        }
        let mut at = 0;
        while at + needle_chars.len() <= hay_chars.len() {
            if hay_chars[at..at + needle_chars.len()] == needle_chars[..] {
                total += 1;
                if hits.len() < limit {
                    let start = at.saturating_sub(80);
                    let end = (at + needle_chars.len() + 80).min(body_chars.len());
                    let excerpt: String = body_chars[start..end].iter().collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
                    hits.push(json!({ "section": i, "label": text.label_for(i), "excerpt": excerpt }));
                }
                at += needle_chars.len().max(1);
            } else {
                at += 1;
            }
        }
    }
    let mut out = String::new();
    for h in &hits {
        out.push_str(&format!("§{} {}\n    {}\n", h["section"], h["label"].as_str().unwrap_or(""), h["excerpt"].as_str().unwrap_or("")));
    }
    out.push_str(&format!("{total} matches\n"));
    Ok(Output { json: json!({ "book": { "id": book.identity, "title": book.title }, "query": query, "total": total, "truncated": total > hits.len(), "hits": hits }), text: out })
}

fn annotation_json(a: &Annotation, book: Option<&Book>) -> Value {
    json!({
        "id": a.id,
        "kind": a.kind,
        "cfi": a.cfi,
        "color": a.color,
        "text": a.text,
        "note": a.note,
        "chapter": a.chapter,
        "fraction": a.fraction,
        "createdAt": a.created_at,
        "updatedAt": a.updated_at,
        "book": book.map(|b| json!({ "id": b.identity, "title": b.title, "path": b.path })),
    })
}

fn annotations(args: &Args) -> Result<Output, Error> {
    let db = open_db()?;
    let kind = match args.value("kind").unwrap_or("highlight") {
        "highlight" | "highlights" => Some(Kind::Highlight),
        "bookmark" | "bookmarks" => Some(Kind::Bookmark),
        "all" => None,
        other => return Err(Error::bad("usage", format!("--kind is highlight, bookmark or all, not {other}"), None)),
    };
    let all_books = books(&db)?;
    let by_id: HashMap<&str, &Book> = all_books.iter().map(|b| (b.identity.as_str(), b)).collect();
    let rows: Vec<(String, Annotation)> = match args.positional.first() {
        Some(name) => {
            let book = resolve(&db, name)?;
            db.annotations(&book.identity).map_err(|e| Error::failed(e.to_string()))?.into_iter().map(|a| (book.identity.clone(), a)).collect()
        }
        None => db.all_annotations().map_err(|e| Error::failed(e.to_string()))?,
    };
    let query = args.value("query").map(str::to_lowercase);
    let matched: Vec<(String, Annotation)> = rows
        .into_iter()
        .filter(|(_, a)| kind.is_none_or(|k| a.kind == k))
        .filter(|(id, a)| {
            let Some(q) = &query else { return true };
            let title = by_id.get(id.as_str()).map(|b| b.title.to_lowercase()).unwrap_or_default();
            a.text.to_lowercase().contains(q) || a.note.to_lowercase().contains(q) || a.chapter.to_lowercase().contains(q) || title.contains(q)
        })
        .collect();
    let total = matched.len();
    let limit = args.number("limit")?.unwrap_or(DEFAULT_LIMIT);
    let shown = &matched[..total.min(limit)];
    let mut out = String::new();
    for (id, a) in shown {
        let title = by_id.get(id.as_str()).map(|b| b.title.as_str()).unwrap_or("?");
        let quote = if a.text.is_empty() { format!("bookmark at {:.0}%", a.fraction * 100.0) } else { format!("“{}”", a.text.trim()) };
        let chapter = a.chapter.split_whitespace().collect::<Vec<_>>().join(" ");
        out.push_str(&format!("{title} · {chapter}\n    {quote}\n"));
        if !a.note.trim().is_empty() {
            out.push_str(&format!("    note: {}\n", a.note.trim()));
        }
    }
    if total > shown.len() {
        out.push_str(&format!("… {} more (--limit)\n", total - shown.len()));
    }
    let list: Vec<Value> = shown.iter().map(|(id, a)| annotation_json(a, by_id.get(id.as_str()).copied())).collect();
    Ok(Output { json: json!({ "annotations": list, "total": total, "truncated": total > shown.len() }), text: out })
}

fn stats(args: &Args) -> Result<Output, Error> {
    let db = open_db()?;
    let days = args.number("days")?.unwrap_or(14).clamp(1, 3660);
    let all = books(&db)?;
    let today = gtk::glib::DateTime::now_local().ok().and_then(|d| d.format("%F").ok()).map(|s| s.to_string()).unwrap_or_default();
    let report = omaread_core::stats::report(&db, &today, days, &all).map_err(|e| Error::failed(e.to_string()))?;
    let text = report.to_text();
    Ok(Output { json: serde_json::to_value(&report).unwrap_or_default(), text })
}

/// Where `omaread open` should land, as the hint passed to the running app.
pub fn open_hint(args: &Args) -> Result<String, Error> {
    if let Some(cfi) = args.value("at") {
        return Ok(format!("at:{cfi}"));
    }
    if let Some(f) = args.value("fraction") {
        let f: f64 = f.parse().ok().filter(|f| (0.0..=1.0).contains(f)).ok_or_else(|| Error::bad("usage", "--fraction goes from 0 to 1", None))?;
        return Ok(format!("fraction:{f}"));
    }
    if let Some(n) = args.number("chapter")? {
        return Ok(format!("chapter:{n}"));
    }
    Ok(String::new())
}

/// The file `omaread open` names, checked before the app starts.
pub fn open_target(args: &[String]) -> Result<(PathBuf, String), Error> {
    let args = Args::parse(args, &["at", "fraction", "chapter"])?;
    let name = book_arg(&args, "omaread open <book> [--at CFI|--fraction F|--chapter N]")?;
    let db = open_db()?;
    let book = resolve(&db, name)?;
    Ok((book.path, open_hint(&args)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn flags_take_values_inline_or_next() {
        let args = Args::parse(&strings(&["dune", "--limit", "5", "--query=frank", "--json"]), &["limit", "query"]).ok().unwrap();
        assert_eq!(args.positional, ["dune"]);
        assert_eq!(args.value("limit"), Some("5"));
        assert_eq!(args.value("query"), Some("frank"));
        assert!(args.json());
        assert!(Args::parse(&strings(&["--limit"]), &["limit"]).is_err());
    }

    #[test]
    fn open_hints_name_the_place() {
        let hint = |a: &[&str]| open_hint(&Args::parse(&strings(a), &["at", "fraction", "chapter"]).ok().unwrap()).ok();
        assert_eq!(hint(&["b", "--at", "epubcfi(/6/4)"]).as_deref(), Some("at:epubcfi(/6/4)"));
        assert_eq!(hint(&["b", "--fraction", "0.5"]).as_deref(), Some("fraction:0.5"));
        assert_eq!(hint(&["b", "--chapter", "3"]).as_deref(), Some("chapter:3"));
        assert_eq!(hint(&["b"]).as_deref(), Some(""));
        assert!(hint(&["b", "--fraction", "2"]).is_none());
    }

    #[test]
    fn every_command_is_described() {
        for name in ["list", "show", "toc", "text", "search", "annotations", "open", "stats", "commands"] {
            assert!(COMMANDS.iter().any(|c| c.name == name), "{name}");
        }
    }
}

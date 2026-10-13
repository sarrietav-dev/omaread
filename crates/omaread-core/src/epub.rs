//! What the library needs from an EPUB without opening it in the reader:
//! metadata from the OPF package document, and the cover image.
//!
//! Only the zip's central directory, `container.xml`, the OPF and (when asked)
//! the cover are read, so a scan of hundreds of books stays fast.

use std::borrow::Cow;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use zip::ZipArchive;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Metadata {
    pub title: String,
    pub authors: Vec<String>,
    pub language: Option<String>,
    /// The package's unique identifier (ISBN, UUID, …), when it has one.
    pub identifier: Option<String>,
    pub series: Option<String>,
    pub series_index: Option<f64>,
    /// Zip path of the cover image, if the book names one.
    pub cover_href: Option<String>,
}

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Zip(zip::result::ZipError),
    /// Not an EPUB we can read: no container, no package document, or DRM.
    Invalid(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Zip(e) => write!(f, "{e}"),
            Error::Invalid(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::Io(e)
    }
}

impl From<zip::result::ZipError> for Error {
    fn from(e: zip::result::ZipError) -> Error {
        Error::Zip(e)
    }
}

/// The OPF and container are small; anything this big is not one.
const MAX_XML: u64 = 8 * 1024 * 1024;
/// Covers bigger than this are skipped rather than decoded.
const MAX_COVER: u64 = 32 * 1024 * 1024;

pub struct Epub {
    zip: ZipArchive<BufReader<File>>,
    opf_path: String,
}

impl Epub {
    pub fn open(path: &Path) -> Result<Epub, Error> {
        let mut zip = ZipArchive::new(BufReader::new(File::open(path)?))?;
        if zip.by_name("META-INF/encryption.xml").is_ok_and(|f| encrypted_content(f)) {
            return Err(Error::Invalid("the book is DRM-protected"));
        }
        let container = read_text(&mut zip, "META-INF/container.xml")?;
        let opf_path = rootfile(&container).ok_or(Error::Invalid("no package document"))?;
        Ok(Epub { zip, opf_path })
    }

    pub fn metadata(&mut self) -> Result<Metadata, Error> {
        let opf = read_text(&mut self.zip, &self.opf_path)?;
        let mut meta = parse_opf(&opf);
        meta.cover_href = meta.cover_href.map(|href| resolve(&self.opf_path, &href));
        Ok(meta)
    }

    /// The cover image's bytes, if the book has one we can find.
    pub fn cover(&mut self, meta: &Metadata) -> Option<Vec<u8>> {
        let href = meta.cover_href.as_deref()?;
        let file = self.zip.by_name(href).ok()?;
        if file.size() > MAX_COVER {
            return None;
        }
        let mut bytes = Vec::with_capacity(file.size() as usize);
        file.take(MAX_COVER).read_to_end(&mut bytes).ok()?;
        Some(bytes)
    }

    /// A deeper look than `metadata`: does the spine resolve to files that
    /// are actually in the zip? What a smoke test asks of every book.
    pub fn check(&mut self) -> Result<Check, Error> {
        let opf = read_text(&mut self.zip, &self.opf_path)?;
        let meta = parse_opf(&opf);
        let package = parse_package(&opf);
        let mut problems: Vec<String> = Vec::new();
        if package.spine.is_empty() {
            problems.push("no spine".into());
        }
        for idref in &package.spine {
            let Some(href) = package.items.get(idref) else {
                problems.push(format!("spine points at missing item {idref}"));
                continue;
            };
            if is_remote(href) {
                continue;
            }
            let path = resolve(&self.opf_path, href);
            if self.zip.by_name(&path).is_err() {
                problems.push(format!("missing {path}"));
            }
        }
        problems.dedup();
        Ok(Check { title: meta.title, chapters: package.spine.len(), problems })
    }
}

/// What `Epub::check` found: how many chapters, and what's wrong.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Check {
    pub title: String,
    pub chapters: usize,
    pub problems: Vec<String>,
}

/// The manifest (`id` → `href`) and the spine's `idref`s, in reading order.
#[derive(Debug, Default)]
pub struct Package {
    pub items: std::collections::HashMap<String, String>,
    pub spine: Vec<String>,
}

pub fn parse_package(opf: &str) -> Package {
    let mut reader = Reader::from_str(opf);
    let mut package = Package::default();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) => match e.local_name().as_ref() {
                "item" => {
                    let id = attr(&e, "id").unwrap_or_default();
                    let href = attr(&e, "href").unwrap_or_default();
                    if !id.is_empty() && !href.is_empty() {
                        package.items.insert(id, href);
                    }
                }
                "itemref" => {
                    let idref = attr(&e, "idref").unwrap_or_default();
                    if !idref.is_empty() {
                        package.spine.push(idref);
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    package
}

/// A resource that isn't in the zip: the check leaves it alone.
fn is_remote(href: &str) -> bool {
    let lower = href.trim_start().to_ascii_lowercase();
    lower.starts_with("http:") || lower.starts_with("https:") || lower.starts_with("data:")
}

fn read_text(zip: &mut ZipArchive<BufReader<File>>, name: &str) -> Result<String, Error> {
    let file = zip.by_name(name).map_err(|_| Error::Invalid("missing package file"))?;
    if file.size() > MAX_XML {
        return Err(Error::Invalid("package file too large"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_XML).read_to_end(&mut bytes)?;
    // Strip a UTF-8 BOM; quick-xml would see it as text before the root.
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

/// Font obfuscation (IDPF's and Adobe's) only hides embedded fonts, and the
/// reader undoes it; any other encryption method is DRM.
const OBFUSCATION: [&str; 2] = ["http://www.idpf.org/2008/embedding", "http://ns.adobe.com/pdf/enc#RC"];

fn encrypted_content(mut file: impl Read) -> bool {
    let mut text = String::new();
    if file.read_to_string(&mut text).is_err() {
        return false;
    }
    is_drm(&text)
}

fn is_drm(encryption_xml: &str) -> bool {
    let mut reader = Reader::from_str(encryption_xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) if e.local_name().as_ref() == "EncryptionMethod" => {
                let algorithm = attr(&e, "Algorithm").unwrap_or_default();
                if !OBFUSCATION.contains(&algorithm.as_str()) {
                    return true;
                }
            }
            Ok(Event::Eof) | Err(_) => return false,
            _ => {}
        }
    }
}

fn attr(e: &BytesStart, local: &str) -> Option<String> {
    e.attributes().flatten().find(|a| a.key.local_name().as_ref() == local).map(|a| {
        #[allow(deprecated)]
        let value = a.unescape_value().unwrap_or_else(|_| Cow::Owned(a.value.to_string()));
        value.trim().to_string()
    })
}

fn rootfile(container: &str) -> Option<String> {
    let mut reader = Reader::from_str(container);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) if e.local_name().as_ref() == "rootfile" => {
                let media = attr(&e, "media-type").unwrap_or_default();
                if media.is_empty() || media == "application/oebps-package+xml" {
                    return attr(&e, "full-path").filter(|p| !p.is_empty());
                }
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

/// A zip path relative to the OPF, normalised (`../`, `./`, `%20`).
fn resolve(opf_path: &str, href: &str) -> String {
    let href = href.split(['#', '?']).next().unwrap_or(href);
    let href = percent_decode(href);
    let mut parts: Vec<&str> = match opf_path.rsplit_once('/') {
        Some((dir, _)) => dir.split('/').collect(),
        None => Vec::new(),
    };
    for seg in href.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(b) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[derive(Default)]
struct Item {
    id: String,
    href: String,
    media_type: String,
    properties: String,
}

#[derive(Default)]
struct Creator {
    id: Option<String>,
    name: String,
    role: Option<String>,
}

/// Parse the package document. Lenient: a malformed OPF yields whatever was
/// read before the error.
pub fn parse_opf(opf: &str) -> Metadata {
    let mut reader = Reader::from_str(opf);

    let mut unique_id_ref = None;
    let mut identifiers: Vec<(Option<String>, String)> = Vec::new();
    let mut titles: Vec<String> = Vec::new();
    let mut creators: Vec<Creator> = Vec::new();
    // EPUB 3 `<meta refines="#id" property="role">aut</meta>`.
    let mut refines: Vec<(String, String, String)> = Vec::new();
    let mut language = None;
    let mut cover_id = None;
    let mut series = None;
    let mut series_index = None;
    let mut items: Vec<Item> = Vec::new();

    // The element whose text we're collecting, and its attributes.
    let mut open: Option<(String, BytesStart<'static>)> = None;
    let mut text = String::new();

    loop {
        let event = match reader.read_event() {
            Ok(e) => e,
            Err(_) => break,
        };
        match event {
            Event::Start(e) => {
                let local = e.local_name().as_ref().to_string();
                if local == "package" {
                    unique_id_ref = attr(&e, "unique-identifier");
                }
                if matches!(local.as_str(), "title" | "creator" | "identifier" | "language" | "meta") {
                    open = Some((local, e.into_owned()));
                    text.clear();
                }
            }
            Event::Empty(e) => match e.local_name().as_ref() {
                "item" => items.push(Item {
                    id: attr(&e, "id").unwrap_or_default(),
                    href: attr(&e, "href").unwrap_or_default(),
                    media_type: attr(&e, "media-type").unwrap_or_default(),
                    properties: attr(&e, "properties").unwrap_or_default(),
                }),
                "meta" => {
                    let name = attr(&e, "name").unwrap_or_default();
                    let content = attr(&e, "content").unwrap_or_default();
                    match name.as_str() {
                        "cover" => cover_id = Some(content),
                        "calibre:series" if !content.is_empty() => series = Some(content),
                        "calibre:series_index" => series_index = content.parse().ok(),
                        _ => {}
                    }
                }
                _ => {}
            },
            Event::Text(t) => {
                if open.is_some() {
                    text.push_str(t.as_ref());
                }
            }
            Event::CData(t) => {
                if open.is_some() {
                    text.push_str(t.as_ref());
                }
            }
            Event::GeneralRef(r) => {
                if open.is_some() {
                    let resolved = r.resolve_char_ref().ok().flatten().map(String::from).or_else(|| {
                        match r.as_ref() {
                            "amp" => Some("&"),
                            "lt" => Some("<"),
                            "gt" => Some(">"),
                            "quot" => Some("\""),
                            "apos" => Some("'"),
                            _ => None,
                        }
                        .map(String::from)
                    });
                    text.push_str(&resolved.unwrap_or_default());
                }
            }
            Event::End(e) => {
                let Some((local, start)) = open.take_if(|(l, _)| l.as_str() == e.local_name().as_ref()) else {
                    continue;
                };
                let value = collapse(&text);
                let id = attr(&start, "id");
                match local.as_str() {
                    "title" if !value.is_empty() => titles.push(value),
                    "creator" if !value.is_empty() => creators.push(Creator { id, name: value, role: attr(&start, "role") }),
                    "identifier" if !value.is_empty() => identifiers.push((id, value)),
                    "language" if language.is_none() && !value.is_empty() => language = Some(value),
                    "meta" => {
                        let property = attr(&start, "property").unwrap_or_default();
                        match (attr(&start, "refines"), property.as_str()) {
                            (Some(target), p) => refines.push((target.trim_start_matches('#').to_string(), p.to_string(), value)),
                            (None, "belongs-to-collection") if series.is_none() => series = Some(value),
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    for (target, property, value) in &refines {
        if property == "role"
            && let Some(c) = creators.iter_mut().find(|c| c.id.as_deref() == Some(target))
        {
            c.role = Some(value.clone());
        }
        if property == "group-position" && series_index.is_none() {
            series_index = value.parse().ok();
        }
    }

    // Authors only; editors, illustrators and translators are left out
    // unless nobody is marked as an author.
    let is_author = |c: &Creator| c.role.as_deref().is_none_or(|r| r == "aut");
    let mut authors: Vec<String> = creators.iter().filter(|c| is_author(c)).map(|c| c.name.clone()).collect();
    if authors.is_empty() {
        authors = creators.iter().map(|c| c.name.clone()).collect();
    }
    authors.dedup();

    let identifier = identifiers
        .iter()
        .find(|(id, _)| id.is_some() && *id == unique_id_ref)
        .or_else(|| identifiers.first())
        .map(|(_, v)| v.clone());

    let image = |i: &&Item| i.media_type.starts_with("image/");
    let cover = items
        .iter()
        .find(|i| i.properties.split_whitespace().any(|p| p == "cover-image"))
        .or_else(|| cover_id.as_ref().and_then(|id| items.iter().find(|i| &i.id == id)))
        .or_else(|| items.iter().filter(image).find(|i| i.id.to_lowercase().contains("cover")))
        .or_else(|| items.iter().filter(image).find(|i| i.href.to_lowercase().contains("cover")))
        .filter(|i| i.media_type.is_empty() || i.media_type.starts_with("image/"))
        .map(|i| i.href.clone());

    Metadata {
        title: titles.into_iter().next().unwrap_or_default(),
        authors,
        language,
        identifier,
        series,
        series_index,
        cover_href: cover,
    }
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPUB3: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="isbn">9780000000000</dc:identifier>
    <dc:identifier id="uid">urn:uuid:1234</dc:identifier>
    <dc:title>Concurrency  in
      Go</dc:title>
    <dc:creator id="c1">Katherine Cox-Buday</dc:creator>
    <meta refines="#c1" property="role" scheme="marc:relators">aut</meta>
    <dc:creator id="c2">Some Editor</dc:creator>
    <meta refines="#c2" property="role">edt</meta>
    <dc:language>en</dc:language>
    <meta property="belongs-to-collection" id="s">Go Series</meta>
    <meta refines="#s" property="group-position">2</meta>
  </metadata>
  <manifest>
    <item id="img" href="images/cover%20art.jpg" media-type="image/jpeg" properties="cover-image"/>
    <item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
</package>"##;

    const EPUB2: &str = r##"<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="BookId" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:title>Outliers &amp; Others</dc:title>
    <dc:creator opf:role="aut">Malcolm Gladwell</dc:creator>
    <dc:identifier id="BookId">isbn-123</dc:identifier>
    <meta name="cover" content="cover-id"/>
    <meta name="calibre:series" content="Gladwell"/>
    <meta name="calibre:series_index" content="3.0"/>
  </metadata>
  <manifest>
    <item id="cover-id" href="../Images/cover.jpeg" media-type="image/jpeg"/>
  </manifest>
</package>"##;

    #[test]
    fn reads_epub3_metadata() {
        let m = parse_opf(EPUB3);
        assert_eq!(m.title, "Concurrency in Go");
        assert_eq!(m.authors, vec!["Katherine Cox-Buday"]);
        assert_eq!(m.identifier.as_deref(), Some("urn:uuid:1234"));
        assert_eq!(m.language.as_deref(), Some("en"));
        assert_eq!(m.series.as_deref(), Some("Go Series"));
        assert_eq!(m.series_index, Some(2.0));
        assert_eq!(m.cover_href.as_deref(), Some("images/cover%20art.jpg"));
    }

    #[test]
    fn reads_epub2_metadata() {
        let m = parse_opf(EPUB2);
        assert_eq!(m.title, "Outliers & Others");
        assert_eq!(m.authors, vec!["Malcolm Gladwell"]);
        assert_eq!(m.identifier.as_deref(), Some("isbn-123"));
        assert_eq!(m.series_index, Some(3.0));
        assert_eq!(m.cover_href.as_deref(), Some("../Images/cover.jpeg"));
    }

    #[test]
    fn resolves_hrefs_against_the_opf() {
        assert_eq!(resolve("OEBPS/Text/content.opf", "../Images/cover.jpeg"), "OEBPS/Images/cover.jpeg");
        assert_eq!(resolve("content.opf", "images/cover%20art.jpg"), "images/cover art.jpg");
        assert_eq!(resolve("OPS/package.opf", "./x.png#frag"), "OPS/x.png");
    }

    #[test]
    fn font_obfuscation_is_not_drm() {
        let adobe = r#"<encryption><EncryptedData><EncryptionMethod Algorithm="http://ns.adobe.com/pdf/enc#RC"/></EncryptedData></encryption>"#;
        let idpf = r#"<encryption><EncryptedData><EncryptionMethod Algorithm="http://www.idpf.org/2008/embedding"/></EncryptedData></encryption>"#;
        let aes = r#"<encryption><EncryptedData><EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#aes128-cbc"/></EncryptedData></encryption>"#;
        assert!(!is_drm(adobe));
        assert!(!is_drm(idpf));
        assert!(is_drm(aes));
    }

    #[test]
    fn finds_the_rootfile() {
        let c = r#"<container><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
        assert_eq!(rootfile(c).as_deref(), Some("OEBPS/content.opf"));
    }

    const PACKAGE: &str = r##"<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>A Book</dc:title></metadata>
  <manifest>
    <item id="c1" href="Text/ch1.xhtml" media-type="application/xhtml+xml"/>
    <item id="c2" href="Text/ch2.xhtml" media-type="application/xhtml+xml"/>
    <item id="web" href="https://example.com/x.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="c1"/><itemref idref="c2"/><itemref idref="web"/></spine>
</package>"##;

    #[test]
    fn reads_the_package_spine_and_manifest() {
        let p = parse_package(PACKAGE);
        assert_eq!(p.spine, vec!["c1", "c2", "web"]);
        assert_eq!(p.items["c1"], "Text/ch1.xhtml");
    }

    /// Write a minimal EPUB: container, OPF, and the given chapter files.
    fn write_epub(dir: &Path, opf: &str, files: &[(&str, &[u8])]) -> std::path::PathBuf {
        use std::io::Write as _;
        let path = dir.join("book.epub");
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("META-INF/container.xml", options).unwrap();
        zip.write_all(br#"<container><rootfiles><rootfile full-path="content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        zip.start_file("content.opf", options).unwrap();
        zip.write_all(opf.as_bytes()).unwrap();
        for (name, bytes) in files {
            zip.start_file(*name, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    #[test]
    fn check_finds_a_complete_book_and_a_broken_one() {
        let dir = tempfile::tempdir().unwrap();
        let good = write_epub(dir.path(), PACKAGE, &[("Text/ch1.xhtml", b"<p/>"), ("Text/ch2.xhtml", b"<p/>")]);
        let check = Epub::open(&good).unwrap().check().unwrap();
        assert_eq!(check.title, "A Book");
        assert_eq!(check.chapters, 3);
        assert!(check.problems.is_empty(), "{:?}", check.problems);

        let dir2 = tempfile::tempdir().unwrap();
        let broken = write_epub(dir2.path(), PACKAGE, &[("Text/ch1.xhtml", b"<p/>")]);
        let check = Epub::open(&broken).unwrap().check().unwrap();
        assert!(check.problems.iter().any(|p| p == "missing Text/ch2.xhtml"), "{:?}", check.problems);
    }
}

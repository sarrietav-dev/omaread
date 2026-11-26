//! A book's words without the reader: its sections in spine order, its table
//! of contents, and each section as plain text. What `omaread text` and
//! `omaread toc` print, and what an agent reads to summarize or quote.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use serde::Serialize;
use zip::ZipArchive;

use crate::epub::Error;

/// Sections bigger than this are cut rather than read whole.
const MAX_SECTION: u64 = 16 * 1024 * 1024;

/// One document in the spine.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Section {
    /// 1-based position in the spine.
    pub index: usize,
    /// Path inside the zip.
    pub href: String,
    /// `linear="no"`: notes, covers and the like, outside the reading order.
    pub linear: bool,
}

/// An entry of the table of contents.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TocEntry {
    pub label: String,
    /// Path inside the zip, with its `#fragment` if it has one.
    pub href: String,
    /// The section it points into (1-based), if it resolves to one.
    pub section: Option<usize>,
    pub depth: usize,
}

pub struct BookText {
    zip: ZipArchive<BufReader<File>>,
    pub sections: Vec<Section>,
    pub toc: Vec<TocEntry>,
}

#[derive(Default)]
struct Manifest {
    items: Vec<(String, String, String, String)>, // id, href, media-type, properties
    spine: Vec<(String, bool)>,
    /// The spine's `toc` attribute: the NCX's id, for EPUB 2.
    ncx: Option<String>,
}

impl BookText {
    pub fn open(path: &Path) -> Result<BookText, Error> {
        let mut zip = ZipArchive::new(BufReader::new(File::open(path)?))?;
        if zip.by_name("META-INF/encryption.xml").is_ok_and(crate::epub::encrypted_content) {
            return Err(Error::Invalid("the book is DRM-protected"));
        }
        let container = crate::epub::read_text(&mut zip, "META-INF/container.xml")?;
        let opf_path = crate::epub::rootfile(&container).ok_or(Error::Invalid("no package document"))?;
        let opf = crate::epub::read_text(&mut zip, &opf_path)?;
        let manifest = parse_manifest(&opf);
        let href_of = |id: &str| manifest.items.iter().find(|i| i.0 == id).map(|i| crate::epub::resolve(&opf_path, &i.1));
        let sections: Vec<Section> = manifest
            .spine
            .iter()
            .filter_map(|(idref, linear)| Some((href_of(idref)?, *linear)))
            .enumerate()
            .map(|(i, (href, linear))| Section { index: i + 1, href, linear })
            .collect();

        // EPUB 3's nav document, else EPUB 2's NCX.
        let nav = manifest.items.iter().find(|i| i.3.split_whitespace().any(|p| p == "nav")).map(|i| crate::epub::resolve(&opf_path, &i.1));
        let ncx = manifest
            .ncx
            .as_deref()
            .and_then(href_of)
            .or_else(|| manifest.items.iter().find(|i| i.2 == "application/x-dtbncx+xml").map(|i| crate::epub::resolve(&opf_path, &i.1)));
        let mut toc = Vec::new();
        if let Some(nav) = nav
            && let Ok(xml) = crate::epub::read_text(&mut zip, &nav)
        {
            toc = parse_nav(&xml, &nav);
        }
        if toc.is_empty()
            && let Some(ncx) = ncx
            && let Ok(xml) = crate::epub::read_text(&mut zip, &ncx)
        {
            toc = parse_ncx(&xml, &ncx);
        }
        for entry in &mut toc {
            let file = entry.href.split('#').next().unwrap_or(&entry.href);
            entry.section = sections.iter().find(|s| s.href == file).map(|s| s.index);
        }
        Ok(BookText { zip, sections, toc })
    }

    /// The section's text: paragraphs separated by blank lines.
    pub fn section_text(&mut self, index: usize) -> Result<String, Error> {
        let section = self.sections.iter().find(|s| s.index == index).ok_or(Error::Invalid("no such section"))?;
        let file = self.zip.by_name(&section.href).map_err(|_| Error::Invalid("the section's file is missing"))?;
        let mut bytes = Vec::new();
        file.take(MAX_SECTION).read_to_end(&mut bytes)?;
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
        Ok(html_to_text(&String::from_utf8_lossy(bytes)))
    }

    /// The top-level TOC entries, the way `:ch N` counts chapters.
    pub fn chapters(&self) -> Vec<&TocEntry> {
        let top = self.toc.iter().map(|e| e.depth).min().unwrap_or(0);
        self.toc.iter().filter(|e| e.depth == top && e.section.is_some()).collect()
    }

    /// Which top-level entry chapter `n` is (0-based), the way `:ch n`
    /// finds it: the one numbered `n`, else the n-th.
    pub fn chapter_position(&self, n: usize) -> Option<usize> {
        let chapters = self.chapters();
        chapters.iter().position(|c| label_number(&c.label) == Some(n)).or_else(|| n.checked_sub(1).filter(|i| *i < chapters.len()))
    }

    /// The sections chapter `n` spans: from its own to the next chapter's,
    /// or to the end of the book.
    pub fn chapter_sections(&self, n: usize) -> Option<std::ops::Range<usize>> {
        let chapters = self.chapters();
        let at = self.chapter_position(n)?;
        let start = chapters[at].section?;
        let end = chapters.iter().skip(at + 1).filter_map(|c| c.section).find(|s| *s > start).unwrap_or(self.sections.len() + 1);
        Some(start..end)
    }

    /// The TOC label that covers a section: the last entry at or before it.
    pub fn label_for(&self, index: usize) -> Option<&str> {
        self.toc.iter().filter(|e| e.section.is_some_and(|s| s <= index)).max_by_key(|e| e.section).map(|e| e.label.as_str())
    }
}

/// The number a contents label gives its chapter: "3. Title", "3 Title",
/// "Chapter 3", "Capítulo 3" — but not "1984" or "3D Negotiation".
pub fn label_number(label: &str) -> Option<usize> {
    let lower = label.trim().to_lowercase();
    let rest = ["chapter ", "capítulo ", "capitulo ", "part ", "parte "].iter().find_map(|p| lower.strip_prefix(p)).unwrap_or(&lower);
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let after = rest[digits.len()..].chars().next();
    let ends_cleanly = matches!(after, None | Some('.' | ':' | ' ' | ')' | '-' | '—'));
    if digits.is_empty() || !ends_cleanly { None } else { digits.parse().ok() }
}

fn parse_manifest(opf: &str) -> Manifest {
    let mut reader = Reader::from_str(opf);
    let mut m = Manifest::default();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) => match e.local_name().as_ref() {
                "item" => m.items.push((
                    crate::epub::attr(&e, "id").unwrap_or_default(),
                    crate::epub::attr(&e, "href").unwrap_or_default(),
                    crate::epub::attr(&e, "media-type").unwrap_or_default(),
                    crate::epub::attr(&e, "properties").unwrap_or_default(),
                )),
                "itemref" => {
                    let idref = crate::epub::attr(&e, "idref").unwrap_or_default();
                    let linear = crate::epub::attr(&e, "linear").is_none_or(|l| l != "no");
                    if !idref.is_empty() {
                        m.spine.push((idref, linear));
                    }
                }
                "spine" => m.ncx = crate::epub::attr(&e, "toc"),
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    m
}

/// `<nav epub:type="toc">`'s nested lists.
fn parse_nav(xml: &str, nav_path: &str) -> Vec<TocEntry> {
    let mut reader = Reader::from_str(xml);
    let mut out = Vec::new();
    let mut in_toc = false;
    let mut nav_depth = 0usize;
    let mut list_depth = 0usize;
    // The open link: href, label so far, the element that opened it, and how
    // deep the same element nests inside it (`<a><span>10</span> Title</a>`).
    let mut link: Option<(String, String, String, usize)> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                "nav" => {
                    if in_toc {
                        nav_depth += 1;
                    } else if is_toc_nav(&e) {
                        in_toc = true;
                        nav_depth = 1;
                    }
                }
                "ol" | "ul" if in_toc => list_depth += 1,
                name @ ("a" | "span") if in_toc => match link.as_mut() {
                    Some((_, _, tag, nest)) if tag == name => *nest += 1,
                    Some(_) => {}
                    None => {
                        let href = crate::epub::attr(&e, "href").map(|h| resolve_keep_fragment(nav_path, &h)).unwrap_or_default();
                        link = Some((href, String::new(), name.to_string(), 0));
                    }
                },
                _ => {}
            },
            Ok(Event::End(e)) => match e.local_name().as_ref() {
                "nav" if in_toc => {
                    nav_depth -= 1;
                    if nav_depth == 0 {
                        break;
                    }
                }
                "ol" | "ul" if in_toc => list_depth = list_depth.saturating_sub(1),
                name @ ("a" | "span") if in_toc => {
                    let closes = match link.as_mut() {
                        Some((_, _, tag, nest)) if tag == name && *nest > 0 => {
                            *nest -= 1;
                            false
                        }
                        Some((_, _, tag, _)) => tag == name,
                        None => false,
                    };
                    if closes && let Some((href, label, _, _)) = link.take() {
                        let label = collapse(&label);
                        if !label.is_empty() && !href.is_empty() {
                            out.push(TocEntry { label, href, section: None, depth: list_depth.saturating_sub(1) });
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::Text(t)) => {
                if let Some((_, label, _, _)) = link.as_mut() {
                    label.push_str(&t.html_content());
                }
            }
            Ok(Event::GeneralRef(r)) => {
                if let Some((_, label, _, _)) = link.as_mut() {
                    push_entity(label, &r);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

fn is_toc_nav(e: &BytesStart) -> bool {
    e.attributes().flatten().any(|a| a.key.local_name().as_ref() == "type" && a.value.as_ref().split_whitespace().any(|t| t == "toc"))
}

/// EPUB 2's `navMap` of `navPoint`s.
fn parse_ncx(xml: &str, ncx_path: &str) -> Vec<TocEntry> {
    let mut reader = Reader::from_str(xml);
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut label: Option<String> = None;
    let mut in_text = false;
    // navPoints whose label we have and whose content we wait for.
    let mut pending: Vec<(usize, String)> = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                "navPoint" => depth += 1,
                "navLabel" => label = Some(String::new()),
                "text" if label.is_some() => in_text = true,
                _ => {}
            },
            Ok(Event::Empty(e)) if e.local_name().as_ref() == "content" => {
                let src = crate::epub::attr(&e, "src").unwrap_or_default();
                if let Some((d, l)) = pending.pop() {
                    let label = collapse(&l);
                    if !label.is_empty() && !src.is_empty() {
                        out.push(TocEntry { label, href: resolve_keep_fragment(ncx_path, &src), section: None, depth: d.saturating_sub(1) });
                    }
                }
            }
            Ok(Event::End(e)) => match e.local_name().as_ref() {
                "navPoint" => depth = depth.saturating_sub(1),
                "text" => in_text = false,
                "navLabel" => {
                    if let Some(l) = label.take() {
                        pending.push((depth, l));
                    }
                }
                _ => {}
            },
            Ok(Event::Text(t)) if in_text => {
                if let Some(l) = label.as_mut() {
                    l.push_str(&t.html_content());
                }
            }
            Ok(Event::GeneralRef(r)) if in_text => {
                if let Some(l) = label.as_mut() {
                    push_entity(l, &r);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

fn resolve_keep_fragment(base: &str, href: &str) -> String {
    let fragment = href.split_once('#').map(|(_, f)| f);
    let path = crate::epub::resolve(base, href);
    match fragment {
        Some(f) if !f.is_empty() => format!("{path}#{f}"),
        _ => path,
    }
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

const BLOCKS: &[&str] = &[
    "p", "div", "section", "article", "aside", "blockquote", "li", "dt", "dd", "h1", "h2", "h3", "h4",
    "h5", "h6", "pre", "tr", "table", "figure", "figcaption", "header", "footer", "hr", "ul", "ol",
];
const SKIP: &[&str] = &["head", "script", "style", "svg", "math"];

/// XHTML to readable text: one paragraph per block, `<br>` as a line break,
/// scripts, styles and the head left out.
pub fn html_to_text(xhtml: &str) -> String {
    let mut reader = Reader::from_str(xhtml);
    let mut out = String::new();
    let mut line = String::new();
    let mut skip = 0usize;
    let mut pre = 0usize;
    let flush = |line: &mut String, out: &mut String, pre: bool| {
        let text = if pre {
            line.trim_end().to_string()
        } else {
            line.split('\n').map(collapse).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
        };
        if !text.is_empty() {
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(&text);
        }
        line.clear();
    };
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.local_name();
                let name = name.as_ref();
                if SKIP.contains(&name) {
                    skip += 1;
                } else if skip == 0 && BLOCKS.contains(&name) {
                    flush(&mut line, &mut out, pre > 0);
                    if name == "pre" {
                        pre += 1;
                    }
                }
            }
            Ok(Event::Empty(e)) if skip == 0 => match e.local_name().as_ref() {
                "br" => line.push('\n'),
                "hr" => flush(&mut line, &mut out, pre > 0),
                "img" => {
                    if let Some(alt) = crate::epub::attr(&e, "alt").filter(|a| !a.trim().is_empty()) {
                        line.push_str(&format!(" [{}] ", alt.trim()));
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) => {
                let name = e.local_name();
                let name = name.as_ref();
                if SKIP.contains(&name) {
                    skip = skip.saturating_sub(1);
                } else if skip == 0 && BLOCKS.contains(&name) {
                    flush(&mut line, &mut out, pre > 0);
                    if name == "pre" {
                        pre = pre.saturating_sub(1);
                    }
                }
            }
            Ok(Event::Text(t)) if skip == 0 => {
                // `<br>` breaks survive collapsing as their own lines.
                let text = t.html_content();
                if pre > 0 {
                    line.push_str(&text);
                } else {
                    line.push_str(&text.replace('\n', " "));
                }
            }
            Ok(Event::CData(t)) if skip == 0 => line.push_str(&t),
            Ok(Event::GeneralRef(r)) if skip == 0 => push_entity(&mut line, &r),
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    flush(&mut line, &mut out, false);
    // Lines broken by <br> keep their breaks; spaces around them go.
    out.lines().map(str::trim).collect::<Vec<_>>().join("\n")
}

/// `&amp;`, `&#8217;`, and the HTML names books use without declaring.
fn push_entity(out: &mut String, r: &quick_xml::events::BytesRef) {
    if let Ok(Some(c)) = r.resolve_char_ref() {
        out.push(c);
        return;
    }
    let s = match &**r {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        "nbsp" => " ",
        "shy" => "",
        "mdash" => "—",
        "ndash" => "–",
        "hellip" => "…",
        "lsquo" => "‘",
        "rsquo" => "’",
        "ldquo" => "“",
        "rdquo" => "”",
        "laquo" => "«",
        "raquo" => "»",
        "copy" => "©",
        "reg" => "®",
        "trade" => "™",
        "deg" => "°",
        "middot" => "·",
        "bull" => "•",
        "times" => "×",
        "eacute" => "é",
        "aacute" => "á",
        "iacute" => "í",
        "oacute" => "ó",
        "uacute" => "ú",
        "ntilde" => "ñ",
        "uuml" => "ü",
        "thinsp" | "ensp" | "emsp" => " ",
        "zwnj" | "zwj" => "",
        _ => return,
    };
    out.push_str(s);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_reads_as_paragraphs() {
        let xhtml = r#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>T</title><style>p{}</style></head>
            <body><h1>Chapter&#160;One</h1><p>It was a  dark
            and <em>stormy</em> night&mdash;truly.</p><p>Line one<br/>line two</p><script>x()</script><img alt="A map" src="m.png"/></body></html>"#;
        assert_eq!(html_to_text(xhtml), "Chapter One\n\nIt was a dark and stormy night—truly.\n\nLine one\nline two\n\n[A map]");
    }

    #[test]
    fn nav_lists_become_entries_with_depth() {
        let nav = r#"<html xmlns:epub="http://www.idpf.org/2007/ops"><body>
            <nav epub:type="landmarks"><ol><li><a href="cover.xhtml">Cover</a></li></ol></nav>
            <nav epub:type="toc"><ol>
              <li><a href="ch1.xhtml">One</a><ol><li><a href="ch1.xhtml#s2">One &amp; a half</a></li></ol></li>
              <li><a href="../Text/ch2.xhtml"><span>2</span> Two</a></li>
            </ol></nav></body></html>"#;
        let toc = parse_nav(nav, "OEBPS/nav.xhtml");
        let got: Vec<(&str, &str, usize)> = toc.iter().map(|e| (e.label.as_str(), e.href.as_str(), e.depth)).collect();
        assert_eq!(got, [("One", "OEBPS/ch1.xhtml", 0), ("One & a half", "OEBPS/ch1.xhtml#s2", 1), ("2 Two", "Text/ch2.xhtml", 0)]);
    }

    #[test]
    fn ncx_nav_points_become_entries() {
        let ncx = r#"<ncx><navMap>
            <navPoint id="a"><navLabel><text>First</text></navLabel><content src="a.html"/>
              <navPoint id="b"><navLabel><text>Inner</text></navLabel><content src="a.html#x"/></navPoint>
            </navPoint>
            <navPoint id="c"><navLabel><text>Second</text></navLabel><content src="c.html"/></navPoint>
            </navMap></ncx>"#;
        let toc = parse_ncx(ncx, "toc.ncx");
        let got: Vec<(&str, usize)> = toc.iter().map(|e| (e.label.as_str(), e.depth)).collect();
        assert_eq!(got, [("First", 0), ("Inner", 1), ("Second", 0)]);
    }

    #[test]
    fn a_book_reads_by_chapter() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("b.epub");
        let file = File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default();
        let mut add = |name: &str, body: &str| {
            use std::io::Write;
            zip.start_file(name, opts).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        };
        add("META-INF/container.xml", r#"<container><rootfiles><rootfile full-path="OPS/p.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#);
        add("OPS/p.opf", r#"<package><manifest>
            <item id="nav" href="nav.xhtml" properties="nav" media-type="application/xhtml+xml"/>
            <item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>
            <item id="c1b" href="c1b.xhtml" media-type="application/xhtml+xml"/>
            <item id="c2" href="c2.xhtml" media-type="application/xhtml+xml"/>
            </manifest><spine><itemref idref="c1"/><itemref idref="c1b"/><itemref idref="c2" linear="no"/></spine></package>"#);
        add("OPS/nav.xhtml", r#"<html><body><nav type="toc"><ol><li><a href="c1.xhtml">One</a></li><li><a href="c2.xhtml">Two</a></li></ol></nav></body></html>"#);
        add("OPS/c1.xhtml", "<html><body><p>Hello.</p></body></html>");
        add("OPS/c1b.xhtml", "<html><body><p>Still one.</p></body></html>");
        add("OPS/c2.xhtml", "<html><body><p>Two.</p></body></html>");
        zip.finish().unwrap();

        let mut book = BookText::open(&path).unwrap();
        assert_eq!(book.sections.len(), 3);
        assert!(!book.sections[2].linear);
        assert_eq!(book.chapters().len(), 2);
        assert_eq!(book.chapter_sections(1), Some(1..3));
        assert_eq!(book.chapter_sections(2), Some(3..4));
        assert_eq!(book.chapter_sections(3), None);
        assert_eq!(book.section_text(2).unwrap(), "Still one.");
        assert_eq!(book.label_for(2), Some("One"));
    }
}

//! How books are set: type, layout and theme. One set for all books, and an
//! optional set per book that replaces it.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Flow {
    Paginated,
    Scrolled,
}

/// How much of the publisher's styling gives way to ours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Publisher {
    /// The book's own fonts and spacing, ours only where it says nothing.
    Keep,
    /// Our font family and size win; the rest is the book's.
    Fonts,
    /// Our font, size, line height and alignment win everywhere.
    All,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReaderPrefs {
    /// `None` is the publisher's font (with our serif as the fallback).
    pub font_family: Option<String>,
    pub font_size: u32,
    pub line_height: f64,
    pub justify: bool,
    pub hyphenate: bool,
    /// Space around the text, in px.
    pub margin: u32,
    /// The widest a column of text gets, in px.
    pub max_width: u32,
    /// 0 = as many as fit (up to two), else exactly this many.
    pub columns: u32,
    pub flow: Flow,
    pub publisher: Publisher,
    /// `None` follows Omarchy; otherwise a theme id (see [`crate::themes`]).
    pub theme: Option<String>,
}

impl Default for ReaderPrefs {
    fn default() -> ReaderPrefs {
        ReaderPrefs {
            font_family: None,
            font_size: 18,
            line_height: 1.5,
            justify: true,
            hyphenate: true,
            margin: 48,
            max_width: 720,
            columns: 0,
            flow: Flow::Paginated,
            publisher: Publisher::Keep,
            theme: None,
        }
    }
}

pub const FONT_SIZE: (u32, u32) = (10, 48);
pub const LINE_HEIGHT: (f64, f64) = (1.0, 2.4);
pub const MARGIN: (u32, u32) = (0, 200);
pub const MAX_WIDTH: (u32, u32) = (360, 2000);

impl ReaderPrefs {
    /// Values from a hand-edited or older store, pulled back into range.
    pub fn sanitized(mut self) -> ReaderPrefs {
        self.font_size = self.font_size.clamp(FONT_SIZE.0, FONT_SIZE.1);
        self.line_height = if self.line_height.is_finite() { self.line_height.clamp(LINE_HEIGHT.0, LINE_HEIGHT.1) } else { 1.5 };
        self.margin = self.margin.clamp(MARGIN.0, MARGIN.1);
        self.max_width = self.max_width.clamp(MAX_WIDTH.0, MAX_WIDTH.1);
        self.columns = self.columns.min(2);
        self.font_family = self.font_family.filter(|f| !f.trim().is_empty() && f.len() < 128);
        self
    }

    pub fn from_json(s: &str) -> Option<ReaderPrefs> {
        serde_json::from_str::<ReaderPrefs>(s).ok().map(ReaderPrefs::sanitized)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_fills_missing_fields() {
        let p = ReaderPrefs { font_size: 22, theme: Some("light".into()), ..Default::default() };
        assert_eq!(ReaderPrefs::from_json(&p.to_json()), Some(p));
        let partial = ReaderPrefs::from_json(r#"{"font_size": 20}"#).unwrap();
        assert_eq!(partial.font_size, 20);
        assert_eq!(partial.line_height, 1.5);
    }

    #[test]
    fn out_of_range_values_are_pulled_back() {
        let p = ReaderPrefs::from_json(r#"{"font_size": 400, "line_height": 0.1, "columns": 9, "font_family": " "}"#).unwrap();
        assert_eq!(p.font_size, 48);
        assert_eq!(p.line_height, 1.0);
        assert_eq!(p.columns, 2);
        assert_eq!(p.font_family, None);
    }
}

//! The `:` command line. Small for now; the palette (M6) grows it.
//!
//! ```text
//! :42%            :goto 42%
//! :p 120          :goto page 120
//! :ch 7           :goto chapter 7
//! ```

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Somewhere in the book, 0.0–1.0.
    GotoFraction(f64),
    /// A page label from the book's page list ("120", "xii").
    GotoPage(String),
    /// The n-th top-level entry of the table of contents, 1-based.
    GotoChapter(usize),
    /// Write the book's highlights and notes out.
    Export(ExportFormat),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Markdown,
    Json,
}

pub fn parse(input: &str) -> Result<Command, String> {
    let input = input.trim().trim_start_matches(':').trim();
    if let Some(rest) = input.strip_prefix("export").filter(|r| r.is_empty() || r.starts_with(' ')) {
        return match rest.trim() {
            "" | "md" | "markdown" => Ok(Command::Export(ExportFormat::Markdown)),
            "json" => Ok(Command::Export(ExportFormat::Json)),
            other => Err(format!("export as md or json, not {other}")),
        };
    }
    let rest = input
        .strip_prefix("goto ")
        .or_else(|| input.strip_prefix("go "))
        .or_else(|| input.strip_prefix("g "))
        .unwrap_or(input)
        .trim();

    if let Some(percent) = rest.strip_suffix('%') {
        let n: f64 = percent.trim().parse().map_err(|_| format!("not a percentage: {rest}"))?;
        if !(0.0..=100.0).contains(&n) {
            return Err("percentages go from 0 to 100".into());
        }
        return Ok(Command::GotoFraction(n / 100.0));
    }

    let mut words = rest.split_whitespace();
    let (Some(head), arg) = (words.next(), words.next()) else {
        return Err("try :42%, :p 120 or :ch 7".into());
    };
    match (head, arg) {
        ("p" | "page", Some(label)) => Ok(Command::GotoPage(label.to_string())),
        ("ch" | "chapter", Some(n)) => match n.parse::<usize>() {
            Ok(n) if n >= 1 => Ok(Command::GotoChapter(n)),
            _ => Err(format!("not a chapter number: {n}")),
        },
        (word, None) if word.parse::<usize>().is_ok() => Ok(Command::GotoPage(word.to_string())),
        _ => Err(format!("unknown command: {input}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gotos() {
        assert_eq!(parse(":42%"), Ok(Command::GotoFraction(0.42)));
        assert_eq!(parse("goto 100%"), Ok(Command::GotoFraction(1.0)));
        assert_eq!(parse(":p 120"), Ok(Command::GotoPage("120".into())));
        assert_eq!(parse("goto page xii"), Ok(Command::GotoPage("xii".into())));
        assert_eq!(parse(":ch 7"), Ok(Command::GotoChapter(7)));
        assert_eq!(parse(":120"), Ok(Command::GotoPage("120".into())));
        assert_eq!(parse(":export"), Ok(Command::Export(ExportFormat::Markdown)));
        assert_eq!(parse(":export json"), Ok(Command::Export(ExportFormat::Json)));
        assert!(parse(":export pdf").is_err());
    }

    #[test]
    fn rejects_nonsense() {
        assert!(parse(":150%").is_err());
        assert!(parse(":ch 0").is_err());
        assert!(parse(":frobnicate").is_err());
        assert!(parse(":").is_err());
    }
}

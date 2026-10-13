//! Fuzzy matching for the palette: every query character in order, scored to
//! favour matches at word starts and in runs.

/// `None` if `query` isn't a subsequence of `text`; higher is a better match.
pub fn score(query: &str, text: &str) -> Option<i32> {
    let query: Vec<char> = query.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
    if query.is_empty() {
        return Some(0);
    }
    let chars: Vec<char> = text.to_lowercase().chars().collect();
    let mut score = 0;
    let mut qi = 0;
    let mut previous: Option<usize> = None;
    for (i, c) in chars.iter().enumerate() {
        if qi == query.len() {
            break;
        }
        if *c != query[qi] {
            continue;
        }
        let word_start = i == 0 || !chars[i - 1].is_alphanumeric();
        score += 1;
        if word_start {
            score += 8;
        }
        if previous.is_some_and(|p| p + 1 == i) {
            score += 5;
        }
        if i == 0 {
            score += 4;
        }
        previous = Some(i);
        qi += 1;
    }
    // Shorter names win ties: "Search" over "Search history".
    (qi == query.len()).then_some(score * 100 - chars.len() as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_subsequences_only() {
        assert!(score("tc", "Table of contents").is_some());
        assert!(score("xyz", "Table of contents").is_none());
        assert_eq!(score("", "anything"), Some(0));
    }

    #[test]
    fn prefers_word_starts_and_runs() {
        let toc = score("toc", "Table of contents").unwrap();
        let other = score("toc", "Photocopy").unwrap();
        assert!(toc > other);
        assert!(score("book", "Bookmarks").unwrap() > score("book", "Back to library: books").unwrap());
    }
}

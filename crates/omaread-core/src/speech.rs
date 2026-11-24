//! Text as a speech engine should hear it.
//!
//! Engines read a bare Roman numeral as letters, so "Part I" comes out as
//! "part eye" and "Part II" as "part eye eye". A numeral that follows a label
//! like "Part" or "Chapter" is a number, and is replaced with its word.

/// Words after which a Roman numeral is a number, not letters.
const LABELS: &[&str] = &[
    "part",
    "parts",
    "chapter",
    "chapters",
    "section",
    "sections",
    "book",
    "books",
    "volume",
    "volumes",
    "title",
    "titles",
    "appendix",
    "appendices",
    "annex",
    "annexes",
    "act",
    "acts",
    "scene",
    "scenes",
    "article",
    "articles",
    "clause",
    "clauses",
    "exhibit",
    "exhibits",
    "figure",
    "figures",
    "table",
    "tables",
    "step",
    "steps",
    "phase",
    "phases",
    "level",
    "levels",
    "tier",
    "tiers",
    "stage",
    "stages",
    "unit",
    "units",
    "module",
    "modules",
    "rule",
    "rules",
    "regulation",
    "regulations",
    "division",
    "divisions",
    "grade",
    "grades",
    "class",
    "classes",
    "item",
    "items",
    "question",
    "questions",
    "example",
    "examples",
    "exercise",
    "exercises",
    "war",
    "wars",
];

/// Words that keep a list of numerals going: "Parts I and II".
const JOINERS: &[&str] = &["and", "or", "through", "to", "versus", "vs"];

const WORDS: [&str; 39] = [
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
    "twenty",
    "twenty-one",
    "twenty-two",
    "twenty-three",
    "twenty-four",
    "twenty-five",
    "twenty-six",
    "twenty-seven",
    "twenty-eight",
    "twenty-nine",
    "thirty",
    "thirty-one",
    "thirty-two",
    "thirty-three",
    "thirty-four",
    "thirty-five",
    "thirty-six",
    "thirty-seven",
    "thirty-eight",
    "thirty-nine",
];

/// The same text with Roman numerals after a label spelled out. Text without
/// such numerals comes back unchanged.
pub fn for_speech(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    // After a label, a numeral is a number; a joiner keeps a list going.
    let mut after_label = false;
    let mut in_list = false;
    for chunk in text.split_inclusive(char::is_whitespace) {
        let body = chunk.trim_end_matches(char::is_whitespace);
        let space = &chunk[body.len()..];
        let start = body.len()
            - body
                .trim_start_matches(|c: char| !c.is_alphanumeric())
                .len();
        let end = body.trim_end_matches(|c: char| !c.is_alphanumeric()).len();
        let core = body.get(start..end).unwrap_or_default();
        if core.is_empty() {
            out.push_str(chunk);
            continue;
        }
        let word = core.to_lowercase();
        if LABELS.contains(&word.as_str()) {
            after_label = true;
            in_list = false;
        } else if (after_label || in_list) && JOINERS.contains(&word.as_str()) {
            after_label = false;
            in_list = true;
        } else if (after_label || in_list)
            && let Some(said) = roman_word(core)
        {
            out.push_str(&body[..start]);
            out.push_str(said);
            out.push_str(&body[end..]);
            out.push_str(space);
            after_label = false;
            // A comma or an "and" may come before the next numeral.
            in_list = true;
            continue;
        } else {
            after_label = false;
            in_list = false;
        }
        out.push_str(chunk);
    }
    out
}

/// "III" → "three". Only canonical numerals we have a word for.
fn roman_word(token: &str) -> Option<&'static str> {
    let value = roman_value(token)?;
    WORDS.get(value.checked_sub(1)? as usize).copied()
}

fn roman_value(token: &str) -> Option<u32> {
    let mut total = 0u32;
    let mut previous = 0u32;
    for ch in token.chars().rev() {
        let value = match ch.to_ascii_uppercase() {
            'I' => 1,
            'V' => 5,
            'X' => 10,
            'L' => 50,
            'C' => 100,
            'D' => 500,
            'M' => 1000,
            _ => return None,
        };
        if value < previous {
            total = total.checked_sub(value)?;
        } else {
            total = total.checked_add(value)?;
            previous = value;
        }
    }
    (total > 0 && to_roman(total).eq_ignore_ascii_case(token)).then_some(total)
}

fn to_roman(mut value: u32) -> String {
    const STEPS: [(u32, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut roman = String::new();
    for (step, digits) in STEPS {
        while value >= step {
            roman.push_str(digits);
            value -= step;
        }
    }
    roman
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numerals_after_a_label_become_words() {
        assert_eq!(
            for_speech("Part I of this book is dedicated"),
            "Part one of this book is dedicated"
        );
        assert_eq!(
            for_speech("Part II, “Auditing Techniques,” then"),
            "Part two, “Auditing Techniques,” then"
        );
        assert_eq!(
            for_speech("Part III focuses on standards"),
            "Part three focuses on standards"
        );
        assert_eq!(
            for_speech("Chapter IV: the next step"),
            "Chapter four: the next step"
        );
        assert_eq!(for_speech("Title V of the Act"), "Title five of the Act");
    }

    #[test]
    fn the_label_may_be_followed_by_punctuation() {
        assert_eq!(for_speech("(Part I)"), "(Part one)");
        assert_eq!(for_speech("see Part II."), "see Part two.");
        assert_eq!(for_speech("PART I"), "PART one");
    }

    #[test]
    fn a_list_of_numerals_is_read_through() {
        assert_eq!(for_speech("Parts I and II"), "Parts one and two");
        assert_eq!(
            for_speech("Parts I, II, and III"),
            "Parts one, two, and three"
        );
        assert_eq!(for_speech("Parts I through III"), "Parts one through three");
    }

    #[test]
    fn numbers_outside_a_label_are_left_alone() {
        assert_eq!(
            for_speech("I think this is II far"),
            "I think this is II far"
        );
        assert_eq!(
            for_speech("COBIT, ITIL, ISO 27001, and IT"),
            "COBIT, ITIL, ISO 27001, and IT"
        );
        assert_eq!(
            for_speech("the CD and the XML file"),
            "the CD and the XML file"
        );
        assert_eq!(for_speech("Parts of II"), "Parts of II");
    }

    #[test]
    fn text_without_numerals_is_unchanged() {
        let text = "When we began writing this book, we had a fundamental tenet:";
        assert_eq!(for_speech(text), text);
        assert_eq!(for_speech(""), "");
    }

    #[test]
    fn numerals_are_canonical() {
        assert_eq!(roman_word("IIII"), None);
        assert_eq!(roman_word("IC"), None);
        assert_eq!(roman_word("ii"), Some("two"));
        assert_eq!(roman_word("XL"), None);
        assert_eq!(roman_word("X"), Some("ten"));
        assert_eq!(roman_word("XXXIX"), Some("thirty-nine"));
    }
}

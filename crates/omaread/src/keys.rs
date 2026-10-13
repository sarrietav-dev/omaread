//! Vim-style keys. One table per view; `g` waits for a second key.
//!
//! Defaults live here. `[keymap.<context>]` in `config.toml` rebinds an
//! action by name (see [`Keymap::new`]); a rebound action's default keys are
//! dropped, so the config is the whole story for that action.

use std::collections::HashMap;

use gtk::gdk::{Key, ModifierType};
use omaread_core::config::Keymap as ConfigKeymap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    // Reader
    NextPage,
    PrevPage,
    NextSection,
    PrevSection,
    First,
    Last,
    Back,
    Forward,
    ToggleStatus,
    Toc,
    CommandLine,
    Search,
    NextHit,
    PrevHit,
    /// Esc while reading: clear the search if there is one, else close.
    ClearOrClose,
    Settings,
    Bigger,
    Smaller,
    CycleTheme,
    ToggleBookmark,
    Highlights,
    Bookmarks,
    // Selection and annotations
    Highlight,
    Color(usize),
    Note,
    Copy,
    Delete,
    /// Read aloud, or pause.
    Speak,
    /// Read aloud from the selection.
    SpeakFrom,
    SentenceNext,
    SentencePrev,
    Faster,
    Slower,
    NextVoice,
    /// A lookup provider's key, from a selection or the lookup card.
    Lookup(char),
    OpenLink,
    // Library and panels
    Move(i32, i32),
    Open,
    Filter,
    Sort,
    NextShelf,
    PrevShelf,
    Rescan,
    // Both
    Help,
    Close,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Context {
    Library,
    Reader,
    /// The table of contents is open over the book.
    Toc,
    /// The reading settings are open over the book.
    Settings,
    /// Text is selected, or a highlight was clicked.
    Selection,
    /// The highlights or bookmarks panel is open.
    Annotations,
    /// The search results have the keys.
    Search,
    /// The lookup card is open.
    Lookup,
    /// The `?` help is open.
    Help,
}

/// The actions a config can name, for `[keymap.<context>]`. Parameterised
/// actions (colours, lookups) aren't here: they take an argument.
pub const NAMED: &[(&str, Action)] = &[
    ("next_page", Action::NextPage),
    ("prev_page", Action::PrevPage),
    ("next_chapter", Action::NextSection),
    ("prev_chapter", Action::PrevSection),
    ("first", Action::First),
    ("last", Action::Last),
    ("back", Action::Back),
    ("forward", Action::Forward),
    ("status_line", Action::ToggleStatus),
    ("contents", Action::Toc),
    ("command_line", Action::CommandLine),
    ("search", Action::Search),
    ("next_hit", Action::NextHit),
    ("prev_hit", Action::PrevHit),
    ("clear_or_close", Action::ClearOrClose),
    ("settings", Action::Settings),
    ("bigger_text", Action::Bigger),
    ("smaller_text", Action::Smaller),
    ("next_theme", Action::CycleTheme),
    ("bookmark", Action::ToggleBookmark),
    ("highlights", Action::Highlights),
    ("bookmarks", Action::Bookmarks),
    ("highlight", Action::Highlight),
    ("note", Action::Note),
    ("copy", Action::Copy),
    ("delete", Action::Delete),
    ("speak", Action::Speak),
    ("speak_from_selection", Action::SpeakFrom),
    ("next_sentence", Action::SentenceNext),
    ("prev_sentence", Action::SentencePrev),
    ("faster", Action::Faster),
    ("slower", Action::Slower),
    ("next_voice", Action::NextVoice),
    ("open_link", Action::OpenLink),
    ("open", Action::Open),
    ("filter", Action::Filter),
    ("sort", Action::Sort),
    ("next_shelf", Action::NextShelf),
    ("prev_shelf", Action::PrevShelf),
    ("rescan", Action::Rescan),
    ("help", Action::Help),
    ("close", Action::Close),
    ("quit", Action::Quit),
];

pub fn context_by_name(name: &str) -> Option<Context> {
    Some(match name {
        "library" => Context::Library,
        "reader" => Context::Reader,
        "toc" => Context::Toc,
        "settings" => Context::Settings,
        "selection" => Context::Selection,
        "annotations" => Context::Annotations,
        "search" => Context::Search,
        "lookup" => Context::Lookup,
        "help" => Context::Help,
        _ => return None,
    })
}

fn action_by_name(name: &str) -> Option<Action> {
    NAMED.iter().find(|(n, _)| *n == name).map(|(_, a)| *a)
}

pub struct Keymap {
    pending_g: bool,
    pending_d: bool,
    /// A config binding, by (context, canonical key).
    overrides: HashMap<(Context, String), Action>,
    /// Actions whose defaults the config replaced; their default keys do nothing.
    rebound: Vec<(Context, Action)>,
}

impl Default for Keymap {
    fn default() -> Keymap {
        Keymap { pending_g: false, pending_d: false, overrides: HashMap::new(), rebound: Vec::new() }
    }
}

impl Keymap {
    /// Build the defaults, then apply `[keymap.<context>]` overrides.
    pub fn new(config: &ConfigKeymap) -> Keymap {
        let mut map = Keymap::default();
        for (context_name, actions) in config {
            let Some(context) = context_by_name(context_name) else {
                eprintln!("omaread: keymap: no context {context_name:?}");
                continue;
            };
            for (action_name, keys) in actions {
                let Some(action) = action_by_name(action_name) else {
                    eprintln!("omaread: keymap: no action {action_name:?}");
                    continue;
                };
                map.rebound.push((context, action));
                for key in keys.list() {
                    match canonical(&key) {
                        Some(spec) => {
                            map.overrides.insert((context, spec), action);
                        }
                        None => eprintln!("omaread: keymap: can't read key {key:?}"),
                    }
                }
            }
        }
        map
    }

    /// The action for a key press, if any. `None` lets the key through.
    pub fn press(&mut self, context: Context, key: Key, mods: ModifierType) -> Option<Action> {
        if !self.overrides.is_empty()
            && let Some(action) = self.overrides.get(&(context, key_spec(key, mods)))
        {
            return Some(*action);
        }

        let ctrl = mods.contains(ModifierType::CONTROL_MASK);
        let shift = mods.contains(ModifierType::SHIFT_MASK);
        // Shift+t is T, however the keyboard layer reports it.
        let ch = key.to_unicode().map(|c| if shift && c.is_ascii_lowercase() { c.to_ascii_uppercase() } else { c });

        let action = if std::mem::take(&mut self.pending_d) {
            (ch == Some('d') && context == Context::Annotations).then_some(Action::Delete)
        } else if std::mem::take(&mut self.pending_g) {
            match (ch, context) {
                (Some('g'), _) => Some(Action::First),
                (Some('r'), Context::Library) => Some(Action::Rescan),
                _ => None,
            }
        } else if ctrl {
            match (ch, context) {
                (Some('q'), _) => Some(Action::Quit),
                (Some('o'), Context::Reader) => Some(Action::Back),
                (Some('i'), Context::Reader) => Some(Action::Forward),
                _ => None,
            }
        } else if ch == Some('g') && context != Context::Selection {
            self.pending_g = true;
            return None;
        } else if ch == Some('d') && context == Context::Annotations {
            self.pending_d = true;
            return None;
        } else {
            match context {
                Context::Reader => reader(key, ch, shift),
                Context::Library => library(key, ch, shift),
                Context::Toc => toc(key, ch),
                Context::Settings => settings(key, ch),
                Context::Selection => selection(key, ch),
                Context::Annotations => annotations(key, ch),
                Context::Search => search(key, ch),
                Context::Lookup => lookup(key, ch),
                Context::Help => help(key, ch),
            }
        };

        action.filter(|a| !self.rebound.iter().any(|(c, x)| *c == context && x == a))
    }
}

/// The canonical name of a key press, as written in the config. Letters keep
/// their case (Shift is part of it); named keys are `space`, `enter`, …;
/// modifiers are `ctrl+…` and `shift+…`.
pub fn key_spec(key: Key, mods: ModifierType) -> String {
    let ctrl = mods.contains(ModifierType::CONTROL_MASK);
    let mut shift = mods.contains(ModifierType::SHIFT_MASK);
    let mut base = match key {
        Key::space => "space".to_string(),
        Key::Return | Key::KP_Enter => "enter".to_string(),
        Key::Escape => "escape".to_string(),
        Key::Tab | Key::ISO_Left_Tab => "tab".to_string(),
        Key::Up => "up".to_string(),
        Key::Down => "down".to_string(),
        Key::Left => "left".to_string(),
        Key::Right => "right".to_string(),
        Key::Home => "home".to_string(),
        Key::End => "end".to_string(),
        Key::Page_Up => "page_up".to_string(),
        Key::Page_Down => "page_down".to_string(),
        Key::Delete => "delete".to_string(),
        Key::BackSpace => "backspace".to_string(),
        Key::F1 => "f1".to_string(),
        Key::F2 => "f2".to_string(),
        Key::F3 => "f3".to_string(),
        Key::F4 => "f4".to_string(),
        Key::F5 => "f5".to_string(),
        Key::F6 => "f6".to_string(),
        Key::F7 => "f7".to_string(),
        Key::F8 => "f8".to_string(),
        Key::F9 => "f9".to_string(),
        Key::F10 => "f10".to_string(),
        Key::F11 => "f11".to_string(),
        Key::F12 => "f12".to_string(),
        _ => match key.to_unicode() {
            Some(c) => c.to_string(),
            None => return String::new(),
        },
    };
    if key == Key::ISO_Left_Tab {
        shift = true;
    }
    if base.chars().count() == 1 {
        let c = base.chars().next().unwrap();
        base = if shift && c.is_ascii_lowercase() { c.to_ascii_uppercase().to_string() } else { c.to_string() };
        return if ctrl { format!("ctrl+{base}") } else { base };
    }
    if ctrl {
        format!("ctrl+{base}")
    } else if shift {
        format!("shift+{base}")
    } else {
        base
    }
}

/// Turn a config key into the canonical form `key_spec` produces.
fn canonical(spec: &str) -> Option<String> {
    let parts: Vec<&str> = spec.split('+').map(str::trim).filter(|p| !p.is_empty()).collect();
    let (base, mods) = parts.split_last()?;
    let mut ctrl = false;
    let mut shift = false;
    for m in mods {
        match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => ctrl = true,
            "shift" => shift = true,
            _ => return None,
        }
    }
    let mut base = named_or_char(base)?;
    if base.chars().count() == 1 {
        let c = base.chars().next().unwrap();
        // Shift is the uppercase letter; a bare `G` means Shift+G. Without
        // shift, a `Ctrl+O` chord is the lowercase key.
        base = if shift && c.is_ascii_lowercase() {
            c.to_ascii_uppercase().to_string()
        } else if !shift && ctrl && c.is_ascii_uppercase() {
            c.to_ascii_lowercase().to_string()
        } else {
            c.to_string()
        };
        return Some(if ctrl { format!("ctrl+{base}") } else { base });
    }
    Some(if ctrl {
        format!("ctrl+{base}")
    } else if shift {
        format!("shift+{base}")
    } else {
        base
    })
}

/// A one-character key as itself; longer names normalised to `key_spec`'s.
fn named_or_char(s: &str) -> Option<String> {
    if s.chars().count() == 1 {
        return Some(s.to_string());
    }
    let name = match s.to_ascii_lowercase().as_str() {
        "space" | "spacebar" => "space",
        "enter" | "return" => "enter",
        "escape" | "esc" => "escape",
        "tab" => "tab",
        "up" => "up",
        "down" => "down",
        "left" => "left",
        "right" => "right",
        "home" => "home",
        "end" => "end",
        "page_up" | "pageup" | "pgup" => "page_up",
        "page_down" | "pagedown" | "pgdn" => "page_down",
        "delete" | "del" => "delete",
        "backspace" => "backspace",
        "f1" => "f1",
        "f2" => "f2",
        "f3" => "f3",
        "f4" => "f4",
        "f5" => "f5",
        "f6" => "f6",
        "f7" => "f7",
        "f8" => "f8",
        "f9" => "f9",
        "f10" => "f10",
        "f11" => "f11",
        "f12" => "f12",
        _ => return None,
    };
    Some(name.to_string())
}

fn reader(key: Key, ch: Option<char>, shift: bool) -> Option<Action> {
    match key {
        Key::space if shift => return Some(Action::PrevPage),
        Key::space | Key::Right | Key::Down | Key::Page_Down => return Some(Action::NextPage),
        Key::Left | Key::Up | Key::Page_Up => return Some(Action::PrevPage),
        Key::Home => return Some(Action::First),
        Key::End => return Some(Action::Last),
        Key::Escape => return Some(Action::ClearOrClose),
        _ => {}
    }
    match ch? {
        'j' | 'l' => Some(Action::NextPage),
        'k' | 'h' => Some(Action::PrevPage),
        'J' | 'L' => Some(Action::NextSection),
        'K' | 'H' => Some(Action::PrevSection),
        'G' => Some(Action::Last),
        'i' => Some(Action::ToggleStatus),
        't' => Some(Action::Toc),
        ':' => Some(Action::CommandLine),
        '/' => Some(Action::Search),
        '?' => Some(Action::Help),
        'n' => Some(Action::NextHit),
        'N' => Some(Action::PrevHit),
        ',' => Some(Action::Settings),
        '+' | '=' => Some(Action::Bigger),
        '-' => Some(Action::Smaller),
        'T' => Some(Action::CycleTheme),
        'm' => Some(Action::ToggleBookmark),
        's' => Some(Action::Speak),
        ']' => Some(Action::SentenceNext),
        '[' => Some(Action::SentencePrev),
        '>' => Some(Action::Faster),
        '<' => Some(Action::Slower),
        'V' => Some(Action::NextVoice),
        'A' => Some(Action::Highlights),
        '\'' => Some(Action::Bookmarks),
        'q' => Some(Action::Close),
        _ => None,
    }
}

fn search(key: Key, ch: Option<char>) -> Option<Action> {
    match key {
        Key::Up => return Some(Action::Move(0, -1)),
        Key::Down => return Some(Action::Move(0, 1)),
        Key::Return | Key::KP_Enter => return Some(Action::Open),
        Key::Escape => return Some(Action::Close),
        _ => {}
    }
    match ch? {
        'j' => Some(Action::Move(0, 1)),
        'k' => Some(Action::Move(0, -1)),
        'n' => Some(Action::NextHit),
        'N' => Some(Action::PrevHit),
        '/' => Some(Action::Search),
        'q' => Some(Action::Close),
        _ => None,
    }
}

fn selection(key: Key, ch: Option<char>) -> Option<Action> {
    if key == Key::Escape {
        return Some(Action::Close);
    }
    match ch? {
        'h' => Some(Action::Highlight),
        c @ '1'..='5' => Some(Action::Color(c as usize - '1' as usize)),
        'a' => Some(Action::Note),
        'y' => Some(Action::Copy),
        'x' => Some(Action::Delete),
        'q' => Some(Action::Close),
        's' => Some(Action::SpeakFrom),
        // Anything else may be a lookup provider's key (d, w, t, …).
        c if c.is_alphanumeric() => Some(Action::Lookup(c)),
        _ => None,
    }
}

fn lookup(key: Key, ch: Option<char>) -> Option<Action> {
    match key {
        Key::Escape => return Some(Action::Close),
        Key::Up => return Some(Action::Move(0, -1)),
        Key::Down => return Some(Action::Move(0, 1)),
        _ => {}
    }
    match ch? {
        'j' => Some(Action::Move(0, 1)),
        'k' => Some(Action::Move(0, -1)),
        'o' => Some(Action::OpenLink),
        'y' => Some(Action::Copy),
        'q' => Some(Action::Close),
        c if c.is_alphanumeric() => Some(Action::Lookup(c)),
        _ => None,
    }
}

fn annotations(key: Key, ch: Option<char>) -> Option<Action> {
    match key {
        Key::Up => return Some(Action::Move(0, -1)),
        Key::Down => return Some(Action::Move(0, 1)),
        Key::Return | Key::KP_Enter => return Some(Action::Open),
        Key::Escape => return Some(Action::Close),
        Key::Delete => return Some(Action::Delete),
        _ => {}
    }
    match ch? {
        'j' => Some(Action::Move(0, 1)),
        'k' => Some(Action::Move(0, -1)),
        'G' => Some(Action::Last),
        'a' => Some(Action::Note),
        'x' => Some(Action::Delete),
        'q' | 'A' | '\'' => Some(Action::Close),
        _ => None,
    }
}

fn settings(key: Key, ch: Option<char>) -> Option<Action> {
    match key {
        Key::Up => return Some(Action::Move(0, -1)),
        Key::Down => return Some(Action::Move(0, 1)),
        Key::Left => return Some(Action::Move(-1, 0)),
        Key::Right | Key::Return | Key::KP_Enter => return Some(Action::Move(1, 0)),
        Key::Escape => return Some(Action::Close),
        _ => {}
    }
    match ch? {
        'j' => Some(Action::Move(0, 1)),
        'k' => Some(Action::Move(0, -1)),
        'h' => Some(Action::Move(-1, 0)),
        'l' => Some(Action::Move(1, 0)),
        ',' | 'q' => Some(Action::Close),
        _ => None,
    }
}

fn toc(key: Key, ch: Option<char>) -> Option<Action> {
    match key {
        Key::Up => return Some(Action::Move(0, -1)),
        Key::Down => return Some(Action::Move(0, 1)),
        Key::Left => return Some(Action::Move(-1, 0)),
        Key::Right => return Some(Action::Move(1, 0)),
        Key::Return | Key::KP_Enter => return Some(Action::Open),
        Key::Escape => return Some(Action::Close),
        Key::Home => return Some(Action::First),
        Key::End => return Some(Action::Last),
        _ => {}
    }
    match ch? {
        'j' => Some(Action::Move(0, 1)),
        'k' => Some(Action::Move(0, -1)),
        'h' => Some(Action::Move(-1, 0)),
        'l' => Some(Action::Move(1, 0)),
        'G' => Some(Action::Last),
        't' | 'q' => Some(Action::Close),
        _ => None,
    }
}

fn library(key: Key, ch: Option<char>, shift: bool) -> Option<Action> {
    match key {
        Key::Tab if shift => return Some(Action::PrevShelf),
        Key::Left => return Some(Action::Move(-1, 0)),
        Key::Right => return Some(Action::Move(1, 0)),
        Key::Up => return Some(Action::Move(0, -1)),
        Key::Down => return Some(Action::Move(0, 1)),
        Key::Return | Key::KP_Enter => return Some(Action::Open),
        Key::Tab => return Some(Action::NextShelf),
        Key::ISO_Left_Tab => return Some(Action::PrevShelf),
        Key::Home => return Some(Action::First),
        Key::End => return Some(Action::Last),
        _ => {}
    }
    match ch? {
        'h' => Some(Action::Move(-1, 0)),
        'l' => Some(Action::Move(1, 0)),
        'k' => Some(Action::Move(0, -1)),
        'j' => Some(Action::Move(0, 1)),
        'G' => Some(Action::Last),
        '/' => Some(Action::Filter),
        ':' => Some(Action::CommandLine),
        '?' => Some(Action::Help),
        'o' => Some(Action::Sort),
        'q' => Some(Action::Close),
        _ => None,
    }
}

fn help(key: Key, ch: Option<char>) -> Option<Action> {
    match key {
        Key::Up => return Some(Action::Move(0, -1)),
        Key::Down => return Some(Action::Move(0, 1)),
        Key::Page_Up => return Some(Action::Move(0, -8)),
        Key::Page_Down => return Some(Action::Move(0, 8)),
        Key::Home => return Some(Action::First),
        Key::End => return Some(Action::Last),
        Key::Escape => return Some(Action::Close),
        _ => {}
    }
    match ch? {
        'j' => Some(Action::Move(0, 1)),
        'k' => Some(Action::Move(0, -1)),
        'G' => Some(Action::Last),
        '?' | 'q' => Some(Action::Close),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(map: &mut Keymap, context: Context, c: char) -> Option<Action> {
        let key = Key::from_name(c.to_string()).expect("a key for every letter");
        let mods = if c.is_uppercase() { ModifierType::SHIFT_MASK } else { ModifierType::empty() };
        map.press(context, key, mods)
    }

    #[test]
    fn vim_paging() {
        let mut map = Keymap::default();
        assert_eq!(press(&mut map, Context::Reader, 'j'), Some(Action::NextPage));
        assert_eq!(press(&mut map, Context::Reader, 'k'), Some(Action::PrevPage));
        assert_eq!(press(&mut map, Context::Reader, 'G'), Some(Action::Last));
    }

    #[test]
    fn shift_h_l_move_by_section_like_j_k() {
        let mut map = Keymap::default();
        assert_eq!(press(&mut map, Context::Reader, 'L'), Some(Action::NextSection));
        assert_eq!(press(&mut map, Context::Reader, 'H'), Some(Action::PrevSection));
        assert_eq!(press(&mut map, Context::Reader, 'J'), Some(Action::NextSection));
        assert_eq!(press(&mut map, Context::Reader, 'K'), Some(Action::PrevSection));
    }

    #[test]
    fn gg_goes_to_the_start_and_g_alone_waits() {
        let mut map = Keymap::default();
        assert_eq!(press(&mut map, Context::Reader, 'g'), None);
        assert_eq!(press(&mut map, Context::Reader, 'g'), Some(Action::First));
        assert_eq!(press(&mut map, Context::Reader, 'g'), None);
        assert_eq!(press(&mut map, Context::Reader, 'x'), None);
        assert_eq!(press(&mut map, Context::Reader, 'j'), Some(Action::NextPage));
    }

    #[test]
    fn dd_deletes_in_the_annotations_panel_only() {
        let mut map = Keymap::default();
        assert_eq!(press(&mut map, Context::Annotations, 'd'), None);
        assert_eq!(press(&mut map, Context::Annotations, 'd'), Some(Action::Delete));
        assert_eq!(press(&mut map, Context::Annotations, 'j'), Some(Action::Move(0, 1)));
        assert_eq!(press(&mut map, Context::Selection, '3'), Some(Action::Color(2)));
    }

    #[test]
    fn library_moves_like_vim() {
        let mut map = Keymap::default();
        assert_eq!(press(&mut map, Context::Library, 'j'), Some(Action::Move(0, 1)));
        assert_eq!(press(&mut map, Context::Library, 'h'), Some(Action::Move(-1, 0)));
        assert_eq!(press(&mut map, Context::Library, 'g'), None);
        assert_eq!(press(&mut map, Context::Library, 'r'), Some(Action::Rescan));
    }

    #[test]
    fn question_mark_opens_help_and_help_closes_on_it() {
        let mut map = Keymap::default();
        let question = Key::from_name("question").unwrap();
        assert_eq!(map.press(Context::Reader, question, ModifierType::empty()), Some(Action::Help));
        assert_eq!(map.press(Context::Help, question, ModifierType::empty()), Some(Action::Close));
        assert_eq!(press(&mut map, Context::Help, 'q'), Some(Action::Close));
    }

    #[test]
    fn key_specs_read_like_the_config() {
        let spec = |name: &str, mods| key_spec(Key::from_name(name).unwrap(), mods);
        assert_eq!(spec("j", ModifierType::empty()), "j");
        assert_eq!(spec("j", ModifierType::SHIFT_MASK), "J");
        assert_eq!(spec("space", ModifierType::SHIFT_MASK), "shift+space");
        assert_eq!(spec("o", ModifierType::CONTROL_MASK), "ctrl+o");
        assert_eq!(spec("Return", ModifierType::empty()), "enter");
        assert_eq!(spec("Page_Down", ModifierType::empty()), "page_down");
        assert_eq!(spec("F1", ModifierType::empty()), "f1");
    }

    #[test]
    fn canonical_matches_what_the_keyboard_sends() {
        assert_eq!(canonical("Space").as_deref(), Some("space"));
        assert_eq!(canonical("shift+j").as_deref(), Some("J"));
        assert_eq!(canonical("Ctrl+O").as_deref(), Some("ctrl+o"));
        assert_eq!(canonical("Return").as_deref(), Some("enter"));
        assert_eq!(canonical("esc").as_deref(), Some("escape"));
        assert_eq!(canonical("F1").as_deref(), Some("f1"));
        assert_eq!(canonical("nonsense").as_deref(), None);
    }

    #[test]
    fn config_rebinds_an_action_and_drops_its_defaults() {
        let config = omaread_core::config::Config::parse(
            "[keymap.reader]\nnext_page = [\"space\", \"n\"]\nprev_page = \"p\"\n",
        )
        .unwrap()
        .keymap;
        let mut map = Keymap::new(&config);
        // The configured keys work…
        let space = Key::from_name("space").unwrap();
        assert_eq!(map.press(Context::Reader, space, ModifierType::empty()), Some(Action::NextPage));
        assert_eq!(press(&mut map, Context::Reader, 'n'), Some(Action::NextPage));
        assert_eq!(press(&mut map, Context::Reader, 'p'), Some(Action::PrevPage));
        // …and the old ones are gone.
        assert_eq!(press(&mut map, Context::Reader, 'j'), None);
        assert_eq!(press(&mut map, Context::Reader, 'k'), None);
        // Other actions keep their defaults.
        assert_eq!(press(&mut map, Context::Reader, 't'), Some(Action::Toc));
        // A different context is untouched.
        assert_eq!(press(&mut map, Context::Library, 'j'), Some(Action::Move(0, 1)));
    }
}

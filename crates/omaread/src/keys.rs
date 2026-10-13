//! Vim-style keys. One table per view; `g` waits for a second key.

use gtk::gdk::{Key, ModifierType};

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
    // Library and panels
    Move(i32, i32),
    Open,
    Filter,
    Sort,
    NextShelf,
    PrevShelf,
    Rescan,
    // Both
    Close,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
    Library,
    Reader,
    /// The table of contents is open over the book.
    Toc,
}

#[derive(Default)]
pub struct Keymap {
    pending_g: bool,
}

impl Keymap {
    /// The action for a key press, if any. `None` lets the key through.
    pub fn press(&mut self, context: Context, key: Key, mods: ModifierType) -> Option<Action> {
        let ctrl = mods.contains(ModifierType::CONTROL_MASK);
        let shift = mods.contains(ModifierType::SHIFT_MASK);
        let ch = key.to_unicode();

        if std::mem::take(&mut self.pending_g) {
            return match (ch, context) {
                (Some('g'), _) => Some(Action::First),
                (Some('r'), Context::Library) => Some(Action::Rescan),
                _ => None,
            };
        }

        if ctrl {
            return match (ch, context) {
                (Some('q'), _) => Some(Action::Quit),
                (Some('o'), Context::Reader) => Some(Action::Back),
                (Some('i'), Context::Reader) => Some(Action::Forward),
                _ => None,
            };
        }

        if ch == Some('g') {
            self.pending_g = true;
            return None;
        }

        match context {
            Context::Reader => reader(key, ch, shift),
            Context::Library => library(key, ch, shift),
            Context::Toc => toc(key, ch),
        }
    }
}

fn reader(key: Key, ch: Option<char>, shift: bool) -> Option<Action> {
    match key {
        Key::space if shift => return Some(Action::PrevPage),
        Key::space | Key::Right | Key::Down | Key::Page_Down => return Some(Action::NextPage),
        Key::Left | Key::Up | Key::Page_Up => return Some(Action::PrevPage),
        Key::Home => return Some(Action::First),
        Key::End => return Some(Action::Last),
        Key::Escape => return Some(Action::Close),
        _ => {}
    }
    match ch? {
        'j' | 'l' => Some(Action::NextPage),
        'k' | 'h' => Some(Action::PrevPage),
        'J' => Some(Action::NextSection),
        'K' => Some(Action::PrevSection),
        'G' => Some(Action::Last),
        'i' => Some(Action::ToggleStatus),
        't' => Some(Action::Toc),
        ':' => Some(Action::CommandLine),
        'q' => Some(Action::Close),
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
        'o' => Some(Action::Sort),
        'q' => Some(Action::Close),
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
    fn gg_goes_to_the_start_and_g_alone_waits() {
        let mut map = Keymap::default();
        assert_eq!(press(&mut map, Context::Reader, 'g'), None);
        assert_eq!(press(&mut map, Context::Reader, 'g'), Some(Action::First));
        assert_eq!(press(&mut map, Context::Reader, 'g'), None);
        assert_eq!(press(&mut map, Context::Reader, 'x'), None);
        assert_eq!(press(&mut map, Context::Reader, 'j'), Some(Action::NextPage));
    }

    #[test]
    fn library_moves_like_vim() {
        let mut map = Keymap::default();
        assert_eq!(press(&mut map, Context::Library, 'j'), Some(Action::Move(0, 1)));
        assert_eq!(press(&mut map, Context::Library, 'h'), Some(Action::Move(-1, 0)));
        assert_eq!(press(&mut map, Context::Library, 'g'), None);
        assert_eq!(press(&mut map, Context::Library, 'r'), Some(Action::Rescan));
    }
}

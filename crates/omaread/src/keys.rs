//! Vim-style keys. One table maps keys to actions; `g` waits for a second key.

use gtk::gdk::{Key, ModifierType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    NextPage,
    PrevPage,
    NextSection,
    PrevSection,
    First,
    Last,
    Back,
    Forward,
    ToggleStatus,
    Close,
    Quit,
}

#[derive(Default)]
pub struct Keymap {
    pending_g: bool,
}

impl Keymap {
    /// The action for a key press, if any. `None` lets the key through.
    pub fn press(&mut self, key: Key, mods: ModifierType) -> Option<Action> {
        let ctrl = mods.contains(ModifierType::CONTROL_MASK);
        let shift = mods.contains(ModifierType::SHIFT_MASK);
        let name = key.to_unicode();

        if std::mem::take(&mut self.pending_g) {
            return match name {
                Some('g') => Some(Action::First),
                _ => None,
            };
        }

        if ctrl {
            return match name {
                Some('o') => Some(Action::Back),
                Some('i') => Some(Action::Forward),
                Some('q') => Some(Action::Quit),
                _ => None,
            };
        }

        match key {
            Key::space if shift => return Some(Action::PrevPage),
            Key::space | Key::Right | Key::Down | Key::Page_Down => return Some(Action::NextPage),
            Key::Left | Key::Up | Key::Page_Up => return Some(Action::PrevPage),
            Key::Home => return Some(Action::First),
            Key::End => return Some(Action::Last),
            Key::Escape => return Some(Action::Close),
            _ => {}
        }

        match name? {
            'j' | 'l' => Some(Action::NextPage),
            'k' | 'h' => Some(Action::PrevPage),
            'J' => Some(Action::NextSection),
            'K' => Some(Action::PrevSection),
            'G' => Some(Action::Last),
            'g' => {
                self.pending_g = true;
                None
            }
            'i' => Some(Action::ToggleStatus),
            'q' => Some(Action::Close),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(map: &mut Keymap, c: char) -> Option<Action> {
        let key = Key::from_name(c.to_string()).expect("a key for every letter");
        let mods = if c.is_uppercase() { ModifierType::SHIFT_MASK } else { ModifierType::empty() };
        map.press(key, mods)
    }

    #[test]
    fn vim_paging() {
        let mut map = Keymap::default();
        assert_eq!(press(&mut map, 'j'), Some(Action::NextPage));
        assert_eq!(press(&mut map, 'k'), Some(Action::PrevPage));
        assert_eq!(press(&mut map, 'G'), Some(Action::Last));
    }

    #[test]
    fn gg_goes_to_the_start_and_g_alone_waits() {
        let mut map = Keymap::default();
        assert_eq!(press(&mut map, 'g'), None);
        assert_eq!(press(&mut map, 'g'), Some(Action::First));
        assert_eq!(press(&mut map, 'g'), None);
        assert_eq!(press(&mut map, 'x'), None);
        assert_eq!(press(&mut map, 'j'), Some(Action::NextPage));
    }
}

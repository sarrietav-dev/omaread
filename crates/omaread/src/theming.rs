//! Wearing the Omarchy theme, and following it live.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use omaread_core::style::chrome_css;
use omaread_core::theme::{Mode, Sources, Theme};
use omaread_core::themes;

pub struct Theming {
    sources: Sources,
    fingerprint: Option<u64>,
    /// The desktop's theme.
    desktop: Theme,
    /// A book's own theme, worn while it's open.
    book: Option<(String, Theme)>,
    provider: gtk::CssProvider,
}

impl Theming {
    pub fn load() -> Theming {
        let sources = Sources::from_env();
        let theme = sources.load().unwrap_or_else(Theme::fallback);
        let provider = gtk::CssProvider::new();
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                // Above libadwaita's own sheet, so our named colours win.
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
            );
        }
        let theming = Theming { fingerprint: sources.fingerprint(), sources, desktop: theme, book: None, provider };
        theming.apply();
        theming
    }

    /// What the window wears now: the book's theme, or the desktop's.
    pub fn theme(&self) -> &Theme {
        self.book.as_ref().map(|(_, t)| t).unwrap_or(&self.desktop)
    }

    /// Wear a book's theme (`None`: the desktop's). `true` if anything changed.
    pub fn set_book_theme(&mut self, id: Option<&str>) -> bool {
        if self.book.as_ref().map(|(i, _)| i.as_str()) == id {
            return false;
        }
        self.book = id.and_then(|id| themes::load(id, &self.sources.shell_text()).map(|t| (id.to_string(), t)));
        self.apply();
        true
    }

    fn apply(&self) {
        let theme = self.theme();
        adw::StyleManager::default().set_color_scheme(match theme.mode {
            Mode::Dark => adw::ColorScheme::ForceDark,
            Mode::Light => adw::ColorScheme::ForceLight,
        });
        self.provider.load_from_string(&chrome_css(theme));
    }

    /// Re-read the desktop theme if it moved. `true` when it did.
    fn poll(&mut self) -> bool {
        let fingerprint = self.sources.fingerprint();
        if fingerprint == self.fingerprint {
            return false;
        }
        self.fingerprint = fingerprint;
        // A theme switch rewrites several files; a half-written palette parses
        // to defaults, and the next tick picks up the finished one.
        if let Some(theme) = self.sources.load() {
            self.desktop = theme;
            // A book's theme takes the desktop's new sizes too.
            if let Some((id, _)) = self.book.take() {
                self.book = themes::load(&id, &self.sources.shell_text()).map(|t| (id, t));
            }
            self.apply();
        }
        true
    }

    /// Poll on a timer and call `changed` after each restyle.
    pub fn follow(theming: &Rc<RefCell<Theming>>, changed: impl Fn(&Theme) + 'static) {
        let theming = Rc::downgrade(theming);
        glib::timeout_add_local(Duration::from_millis(500), move || {
            let Some(theming) = theming.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let moved = theming.borrow_mut().poll();
            if moved {
                changed(theming.borrow().theme());
            }
            glib::ControlFlow::Continue
        });
    }
}

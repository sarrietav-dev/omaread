//! The `:` palette: every command, fuzzy-filtered as you type, and the
//! commands that take an argument (`:42%`, `:p 120`, `:export`).

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use omaread_core::fuzzy;

use crate::keys::{Action, Context};

/// A command: what it's called, its keys, and what it does.
#[derive(Debug, Clone)]
pub struct Entry {
    pub name: &'static str,
    pub keys: &'static str,
    pub run: Run,
}

#[derive(Debug, Clone)]
pub enum Run {
    Action(Action),
    /// An ex command line, as typed after `:`.
    Line(&'static str),
}

/// The commands on offer while reading.
pub fn reader_commands() -> Vec<Entry> {
    use Action::*;
    let a = |name, keys, action| Entry { name, keys, run: Run::Action(action) };
    let l = |name, keys, line| Entry { name, keys, run: Run::Line(line) };
    vec![
        a("Contents", "t", Toc),
        a("Search in book", "/", Search),
        a("Highlights and notes", "A", Highlights),
        a("Bookmarks", "'", Bookmarks),
        a("Bookmark this page", "m", ToggleBookmark),
        a("Reading settings", ",", Settings),
        a("Bigger text", "+", Bigger),
        a("Smaller text", "-", Smaller),
        a("Next theme", "T", CycleTheme),
        a("Status line", "i", ToggleStatus),
        a("Next chapter", "J", NextSection),
        a("Previous chapter", "K", PrevSection),
        a("Start of the book", "gg", First),
        a("End of the book", "G", Last),
        a("Back", "ctrl+o", Back),
        a("Forward", "ctrl+i", Forward),
        a("Read aloud / pause", "s", Speak),
        a("Next sentence (reading aloud)", "]", SentenceNext),
        a("Previous sentence (reading aloud)", "[", SentencePrev),
        a("Read faster", ">", Faster),
        a("Read slower", "<", Slower),
        a("Next voice", "V", NextVoice),
        a("Next search result", "n", NextHit),
        a("Previous search result", "N", PrevHit),
        l("Export highlights as Markdown", ":export", "export"),
        l("Export highlights as JSON", ":export json", "export json"),
        l("Go to… (42%, p 120, ch 7)", ":", ""),
        a("Back to the library", "q", Close),
        a("Quit", "ctrl+q", Quit),
    ]
}

pub fn library_commands() -> Vec<Entry> {
    use Action::*;
    let a = |name, keys, action| Entry { name, keys, run: Run::Action(action) };
    vec![
        a("Open", "enter", Open),
        a("Filter books", "/", Filter),
        a("Sort by…", "o", Sort),
        a("Next shelf", "tab", NextShelf),
        a("Previous shelf", "shift+tab", PrevShelf),
        a("First book", "gg", First),
        a("Last book", "G", Last),
        a("Rescan the library", "gr", Rescan),
        a("Quit", "q", Quit),
    ]
}

pub struct Palette {
    pub root: gtk::Box,
    pub entry: gtk::Entry,
    list: gtk::ListBox,
    commands: RefCell<Vec<Entry>>,
    /// The commands showing, best match first.
    shown: RefCell<Vec<Entry>>,
    context: RefCell<Option<Context>>,
}

impl Palette {
    pub fn new() -> Rc<Palette> {
        let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::Single).css_classes(["palette-list"]).build();
        let scroller = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(360)
            .build();
        let entry = gtk::Entry::builder().css_classes(["panel-entry"]).build();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::End)
            .width_request(560)
            .visible(false)
            .css_classes(["card", "palette"])
            .build();
        root.append(&scroller);
        root.append(&entry);
        let palette = Rc::new(Palette {
            root,
            entry,
            list,
            commands: RefCell::default(),
            shown: RefCell::default(),
            context: RefCell::default(),
        });
        palette.entry.connect_changed({
            let palette = Rc::downgrade(&palette);
            move |_| {
                if let Some(p) = palette.upgrade() {
                    p.refilter();
                }
            }
        });
        palette
    }

    pub fn is_open(&self) -> bool {
        self.root.is_visible()
    }

    pub fn open(&self, context: Context) {
        let commands = match context {
            Context::Library => library_commands(),
            _ => reader_commands(),
        };
        self.commands.replace(commands);
        self.context.replace(Some(context));
        self.entry.set_text("");
        self.refilter();
        self.root.set_visible(true);
        self.entry.grab_focus();
    }

    /// The context the palette was opened from.
    pub fn context(&self) -> Option<Context> {
        *self.context.borrow()
    }

    pub fn close(&self) {
        self.root.set_visible(false);
    }

    pub fn has_focus(&self) -> bool {
        self.is_open()
            && self
                .entry
                .root()
                .and_then(|r| gtk::prelude::RootExt::focus(&r))
                .is_some_and(|f| f == self.entry.clone().upcast::<gtk::Widget>() || f.is_ancestor(&self.entry))
    }

    pub fn text(&self) -> String {
        self.entry.text().to_string()
    }

    fn refilter(&self) {
        let query = self.entry.text().to_string();
        let mut scored: Vec<(i32, Entry)> = self
            .commands
            .borrow()
            .iter()
            .filter_map(|e| fuzzy::score(&query, e.name).or_else(|| fuzzy::score(&query, e.keys).map(|s| s - 1000)).map(|s| (s, e.clone())))
            .collect();
        if !query.trim().is_empty() {
            scored.sort_by(|a, b| b.0.cmp(&a.0));
        }
        let shown: Vec<Entry> = scored.into_iter().map(|(_, e)| e).collect();
        while let Some(row) = self.list.row_at_index(0) {
            self.list.remove(&row);
        }
        for e in &shown {
            let name = gtk::Label::builder().label(e.name).xalign(0.0).hexpand(true).build();
            let keys = gtk::Label::builder().label(e.keys).css_classes(["key"]).build();
            let row = gtk::Box::builder().spacing(12).build();
            row.append(&name);
            row.append(&keys);
            self.list.append(&gtk::ListBoxRow::builder().child(&row).build());
        }
        self.shown.replace(shown);
        if let Some(row) = self.list.row_at_index(0) {
            self.list.select_row(Some(&row));
        }
    }

    pub fn move_cursor(&self, delta: i32) {
        let n = self.shown.borrow().len() as i32;
        if n == 0 {
            return;
        }
        let i = self.list.selected_row().map(|r| r.index()).unwrap_or(0);
        if let Some(row) = self.list.row_at_index((i + delta).clamp(0, n - 1)) {
            self.list.select_row(Some(&row));
        }
    }

    pub fn selected(&self) -> Option<Entry> {
        let i = self.list.selected_row()?.index() as usize;
        self.shown.borrow().get(i).cloned()
    }
}

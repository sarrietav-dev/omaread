//! Searching the book: a panel with the query, live progress, and the hits
//! grouped by chapter.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Hit {
    pub cfi: String,
    #[serde(default)]
    pub pre: String,
    #[serde(default)]
    pub hit: String,
    #[serde(default)]
    pub post: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Options {
    pub match_case: bool,
    pub whole_words: bool,
    pub diacritics: bool,
}

pub struct SearchView {
    pub root: gtk::Box,
    pub entry: gtk::Entry,
    status: gtk::Label,
    list: gtk::ListBox,
    /// Hits in book order; list rows map onto them (headers map to None).
    hits: RefCell<Vec<Hit>>,
    rows: RefCell<Vec<Option<usize>>>,
    /// The hit n/N last went to.
    current: Cell<Option<usize>>,
    options: Cell<Options>,
    searching: Cell<bool>,
    last_chapter: RefCell<String>,
    /// Move the keys to the results when the first ones arrive.
    focus_on_hits: Cell<bool>,
}

impl SearchView {
    pub fn new() -> Rc<SearchView> {
        let title = gtk::Label::builder().label("Search").xalign(0.0).css_classes(["title"]).build();
        let entry = gtk::Entry::builder().placeholder_text("Find in this book").css_classes(["panel-entry"]).build();
        let status = gtk::Label::builder().xalign(0.0).css_classes(["caption"]).build();
        let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::Single).css_classes(["search-list"]).build();
        let scroller = gtk::ScrolledWindow::builder().child(&list).vexpand(true).hscrollbar_policy(gtk::PolicyType::Never).build();
        let hint = gtk::Label::builder()
            .label("enter search   j/k move   enter go   n/N next   alt+c case   alt+w words   alt+d accents")
            .xalign(0.0)
            .wrap(true)
            .css_classes(["caption"])
            .build();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Fill)
            .visible(false)
            .css_classes(["card", "side-panel", "search-panel"])
            .build();
        root.append(&title);
        root.append(&entry);
        root.append(&status);
        root.append(&scroller);
        root.append(&hint);
        let view = Rc::new(SearchView {
            root,
            entry,
            status,
            list,
            hits: RefCell::default(),
            rows: RefCell::default(),
            current: Cell::new(None),
            options: Cell::new(Options::default()),
            searching: Cell::new(false),
            last_chapter: RefCell::default(),
            focus_on_hits: Cell::new(false),
        });
        view.update_status();
        view
    }

    pub fn is_open(&self) -> bool {
        self.root.is_visible()
    }

    pub fn open(&self) {
        self.root.set_visible(true);
        self.entry.grab_focus();
        self.entry.select_region(0, -1);
    }

    pub fn close(&self) {
        self.root.set_visible(false);
    }

    pub fn entry_has_focus(&self) -> bool {
        self.entry
            .root()
            .and_then(|r| gtk::prelude::RootExt::focus(&r))
            .is_some_and(|f| f == self.entry.clone().upcast::<gtk::Widget>() || f.is_ancestor(&self.entry))
    }

    pub fn query(&self) -> String {
        self.entry.text().trim().to_string()
    }

    pub fn options(&self) -> Options {
        self.options.get()
    }

    /// Alt+c / Alt+w / Alt+d.
    pub fn toggle(&self, which: char) {
        let mut o = self.options.get();
        match which {
            'c' => o.match_case = !o.match_case,
            'w' => o.whole_words = !o.whole_words,
            'd' => o.diacritics = !o.diacritics,
            _ => return,
        }
        self.options.set(o);
        self.update_status();
    }

    /// A new search is starting: forget the old hits.
    pub fn begin(&self) {
        while let Some(row) = self.list.row_at_index(0) {
            self.list.remove(&row);
        }
        self.hits.borrow_mut().clear();
        self.rows.borrow_mut().clear();
        self.current.set(None);
        self.last_chapter.borrow_mut().clear();
        self.searching.set(true);
        self.focus_on_hits.set(true);
        self.status.set_label("Searching…");
    }

    pub fn clear(&self) {
        self.begin();
        self.searching.set(false);
        self.focus_on_hits.set(false);
        self.update_status();
    }

    pub fn has_hits(&self) -> bool {
        !self.hits.borrow().is_empty()
    }

    pub fn add(&self, chapter: &str, hits: Vec<Hit>) {
        if *self.last_chapter.borrow() != chapter {
            self.last_chapter.replace(chapter.to_string());
            let header = gtk::Label::builder()
                .label(if chapter.is_empty() { "—" } else { chapter })
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .css_classes(["caption"])
                .build();
            let row = gtk::ListBoxRow::builder().child(&header).selectable(false).activatable(false).css_classes(["header"]).build();
            self.list.append(&row);
            self.rows.borrow_mut().push(None);
        }
        for hit in hits {
            let markup = format!(
                "{}<b>{}</b>{}",
                glib::markup_escape_text(hit.pre.trim_start()),
                glib::markup_escape_text(&hit.hit),
                glib::markup_escape_text(hit.post.trim_end())
            );
            let label = gtk::Label::builder()
                .use_markup(true)
                .label(markup)
                .xalign(0.0)
                .wrap(true)
                .lines(2)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .build();
            self.list.append(&gtk::ListBoxRow::builder().child(&label).build());
            let index = self.hits.borrow().len();
            self.rows.borrow_mut().push(Some(index));
            self.hits.borrow_mut().push(hit);
        }
        self.update_status();
        if self.focus_on_hits.replace(false) && self.is_open() {
            self.focus_results();
        }
    }

    pub fn progress(&self, fraction: f64) {
        if self.searching.get() {
            let n = self.hits.borrow().len();
            self.status.set_label(&format!("Searching… {}%  ·  {n} found", (fraction * 100.0).round()));
        }
    }

    pub fn done(&self, error: Option<&str>) {
        self.searching.set(false);
        match error {
            Some(e) => self.status.set_label(&format!("Search failed: {e}")),
            None => self.update_status(),
        }
    }

    fn update_status(&self) {
        if self.searching.get() {
            return;
        }
        let n = self.hits.borrow().len();
        let o = self.options.get();
        let flags: Vec<&str> = [(o.match_case, "case"), (o.whole_words, "whole words"), (o.diacritics, "accents")]
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, name)| *name)
            .collect();
        let found = match n {
            0 if self.query().is_empty() => String::new(),
            0 => "Nothing found".into(),
            1 => "1 match".into(),
            n => format!("{n} matches"),
        };
        let flags = if flags.is_empty() { String::new() } else { format!("matching {}", flags.join(", ")) };
        self.status.set_label(&[found, flags].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("  ·  "));
    }

    /// Give the keys to the list, on the first hit.
    pub fn focus_results(&self) {
        let first = self.rows.borrow().iter().position(Option::is_some);
        if let Some(i) = first {
            self.select_row(i as i32);
        }
    }

    pub fn move_cursor(&self, delta: i32) {
        let rows = self.rows.borrow().clone();
        let current = self.list.selected_row().map(|r| r.index()).unwrap_or(-1);
        let mut i = current;
        loop {
            i += delta.signum();
            if i < 0 || i as usize >= rows.len() {
                return;
            }
            if rows[i as usize].is_some() {
                break;
            }
        }
        self.select_row(i);
    }

    fn select_row(&self, i: i32) {
        if let Some(row) = self.list.row_at_index(i) {
            self.list.select_row(Some(&row));
            row.grab_focus();
        }
    }

    /// The hit under the cursor, which becomes the current one for n/N.
    pub fn selected(&self) -> Option<Hit> {
        let i = self.list.selected_row()?.index() as usize;
        let hit = (*self.rows.borrow().get(i)?)?;
        self.current.set(Some(hit));
        self.hits.borrow().get(hit).cloned()
    }

    /// n / N: the next or previous hit, wrapping.
    pub fn step(&self, delta: i32) -> Option<Hit> {
        let hits = self.hits.borrow();
        if hits.is_empty() {
            return None;
        }
        let n = hits.len() as i32;
        let next = match self.current.get() {
            Some(i) => (i as i32 + delta).rem_euclid(n),
            None if delta >= 0 => 0,
            None => n - 1,
        } as usize;
        self.current.set(Some(next));
        let hit = hits[next].clone();
        drop(hits);
        self.status.set_label(&format!("{} of {}", next + 1, n));
        Some(hit)
    }
}

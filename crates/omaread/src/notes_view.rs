//! Highlights and notes from every book, searched as you type; Enter opens
//! the book at the one chosen.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use omaread_core::annotations::Annotation;
use omaread_core::library::Book;

/// More than this and the list is cut; typing narrows it.
const MAX_SHOWN: usize = 200;

pub struct NotesView {
    pub root: gtk::Box,
    pub entry: gtk::Entry,
    list: gtk::ListBox,
    count: gtk::Label,
    all: RefCell<Vec<(Book, Annotation)>>,
    shown: RefCell<Vec<(Book, Annotation)>>,
}

/// True when every word of the query is in the highlight, its note, its
/// chapter, or its book's title or authors.
pub fn matches(book: &Book, a: &Annotation, query: &str) -> bool {
    let hay = format!("{} {} {} {} {}", a.text, a.note, a.chapter, book.title, book.authors.join(" ")).to_lowercase();
    query.to_lowercase().split_whitespace().all(|w| hay.contains(w))
}

impl NotesView {
    pub fn new() -> Rc<NotesView> {
        let entry = gtk::Entry::builder().placeholder_text("Search highlights and notes in every book").css_classes(["panel-entry"]).build();
        let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::Single).css_classes(["palette-list"]).build();
        let scroller = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(460)
            .build();
        let count = gtk::Label::builder().xalign(0.0).css_classes(["caption"]).build();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Start)
            .margin_top(40)
            .width_request(640)
            .visible(false)
            .css_classes(["card", "palette", "notes-search"])
            .build();
        root.append(&entry);
        root.append(&scroller);
        root.append(&count);
        let view = Rc::new(NotesView { root, entry, list, count, all: RefCell::default(), shown: RefCell::default() });
        view.entry.connect_changed({
            let view = Rc::downgrade(&view);
            move |_| {
                if let Some(v) = view.upgrade() {
                    v.refilter();
                }
            }
        });
        view
    }

    pub fn is_open(&self) -> bool {
        self.root.is_visible()
    }

    pub fn has_focus(&self) -> bool {
        self.is_open()
            && self.root.root().and_then(|r| gtk::prelude::RootExt::focus(&r)).is_some_and(|f| f.is_ancestor(&self.root))
    }

    pub fn open(&self, notes: Vec<(Book, Annotation)>) {
        self.all.replace(notes);
        self.entry.set_text("");
        self.refilter();
        self.root.set_visible(true);
        self.entry.grab_focus();
    }

    pub fn close(&self) {
        self.root.set_visible(false);
        self.all.take();
        self.shown.take();
    }

    fn refilter(&self) {
        let query = self.entry.text().to_string();
        let all = self.all.borrow();
        let matched: Vec<&(Book, Annotation)> = all.iter().filter(|(b, a)| matches(b, a, &query)).collect();
        let total = matched.len();
        let shown: Vec<(Book, Annotation)> = matched.into_iter().take(MAX_SHOWN).cloned().collect();
        while let Some(row) = self.list.row_at_index(0) {
            self.list.remove(&row);
        }
        for (book, a) in &shown {
            let chapter = a.chapter.split_whitespace().collect::<Vec<_>>().join(" ");
            let place = if chapter.is_empty() { book.title.clone() } else { format!("{}  ·  {chapter}", book.title) };
            let place = gtk::Label::builder().label(&place).xalign(0.0).ellipsize(gtk::pango::EllipsizeMode::End).css_classes(["caption"]).build();
            let quote: String = a.text.split_whitespace().collect::<Vec<_>>().join(" ");
            let quote = gtk::Label::builder().label(format!("“{quote}”")).xalign(0.0).wrap(true).lines(3).ellipsize(gtk::pango::EllipsizeMode::End).build();
            let row = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(2).build();
            row.append(&place);
            row.append(&quote);
            if !a.note.trim().is_empty() {
                let note = gtk::Label::builder().label(format!("note: {}", a.note.trim())).xalign(0.0).wrap(true).lines(2).ellipsize(gtk::pango::EllipsizeMode::End).css_classes(["key"]).build();
                row.append(&note);
            }
            self.list.append(&gtk::ListBoxRow::builder().child(&row).build());
        }
        if let Some(row) = self.list.row_at_index(0) {
            self.list.select_row(Some(&row));
        }
        let what = if all.is_empty() {
            "No highlights yet: select text in a book and press h".to_string()
        } else if total > shown.len() {
            format!("{} of {total} highlights; type to narrow   ↑/↓ choose   enter open   esc close", shown.len())
        } else {
            format!("{total} of {} highlights   ↑/↓ choose   enter open   esc close", all.len())
        };
        self.count.set_label(&what);
        self.shown.replace(shown);
    }

    pub fn move_cursor(&self, delta: i32) {
        let n = self.shown.borrow().len() as i32;
        if n == 0 {
            return;
        }
        let i = self.list.selected_row().map(|r| r.index()).unwrap_or(0);
        if let Some(row) = self.list.row_at_index((i + delta).clamp(0, n - 1)) {
            self.list.select_row(Some(&row));
            row.grab_focus();
            self.entry.grab_focus_without_selecting();
        }
    }

    pub fn selected(&self) -> Option<(Book, Annotation)> {
        let i = self.list.selected_row()?.index() as usize;
        self.shown.borrow().get(i).cloned()
    }

    pub fn connect_row_activated(&self, f: impl Fn() + 'static) {
        self.list.connect_row_activated(move |_, _| f());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omaread_core::annotations::Kind;

    #[test]
    fn every_word_must_appear_somewhere() {
        let book = Book {
            path: "/b.epub".into(),
            identity: "b".into(),
            title: "Make It Stick".into(),
            authors: vec!["Peter C. Brown".into()],
            language: None,
            series: None,
            series_index: None,
            shelf: String::new(),
            cover: None,
            error: None,
            added_at: 0,
            progress: None,
        };
        let a = Annotation {
            id: 1,
            kind: Kind::Highlight,
            cfi: String::new(),
            color: "yellow".into(),
            text: "Retrieval practice strengthens memory".into(),
            note: "use flashcards".into(),
            chapter: "Preface".into(),
            fraction: 0.1,
            created_at: 0,
            updated_at: 0,
        };
        assert!(matches(&book, &a, ""));
        assert!(matches(&book, &a, "retrieval memory"));
        assert!(matches(&book, &a, "flashcards stick"));
        assert!(matches(&book, &a, "brown"));
        assert!(!matches(&book, &a, "retrieval spaced"));
    }
}

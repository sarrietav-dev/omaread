//! Highlights and bookmarks on screen: the bar over a selection, the note
//! editor, and the panel listing them.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use omaread_core::annotations::{Annotation, Kind};

use crate::reader::Rect;

/// The card that follows a selection (or a clicked highlight) with its keys.
pub struct SelectionBar {
    pub root: gtk::Box,
    excerpt: gtk::Label,
    keys: gtk::Label,
}

const BAR_WIDTH: i32 = 440;

impl SelectionBar {
    pub fn new() -> SelectionBar {
        let excerpt = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .lines(2)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(48)
            .css_classes(["excerpt"])
            .build();
        let keys = gtk::Label::builder().xalign(0.0).wrap(true).css_classes(["caption"]).build();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Start)
            .width_request(BAR_WIDTH)
            .visible(false)
            .css_classes(["card", "selection-bar"])
            .build();
        root.append(&excerpt);
        root.append(&keys);
        SelectionBar { root, excerpt, keys }
    }

    /// Show below the text (above it near the bottom), inside the page.
    pub fn show(&self, text: &str, keys: &str, rect: Option<Rect>, page: (i32, i32)) {
        self.excerpt.set_label(text.trim());
        self.excerpt.set_visible(!text.trim().is_empty());
        self.keys.set_label(keys);
        let (w, h) = page;
        let r = rect.unwrap_or(Rect { x: (w - BAR_WIDTH) / 2, y: h / 2, width: 0, height: 0 });
        let x = r.x.clamp(8, (w - BAR_WIDTH - 8).max(8));
        let below = r.y + r.height + 10;
        let y = if below + 120 > h { (r.y - 110).max(8) } else { below };
        self.root.set_margin_start(x);
        self.root.set_margin_top(y);
        self.root.set_visible(true);
    }

    pub fn hide(&self) {
        self.root.set_visible(false);
    }

    pub fn is_open(&self) -> bool {
        self.root.is_visible()
    }
}

/// A note, written over the book.
pub struct NoteEditor {
    pub root: gtk::Box,
    quote: gtk::Label,
    text: gtk::TextView,
}

impl NoteEditor {
    pub fn new() -> NoteEditor {
        let title = gtk::Label::builder().label("Note").xalign(0.0).css_classes(["title"]).build();
        let quote = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .lines(3)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["excerpt"])
            .build();
        let text = gtk::TextView::builder()
            .wrap_mode(gtk::WrapMode::WordChar)
            .accepts_tab(false)
            .height_request(140)
            .css_classes(["note-text"])
            .build();
        let hint = gtk::Label::builder().label("ctrl+enter save   esc cancel").xalign(0.0).css_classes(["caption"]).build();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .width_request(520)
            .visible(false)
            .css_classes(["card"])
            .build();
        root.append(&title);
        root.append(&quote);
        root.append(&text);
        root.append(&hint);
        NoteEditor { root, quote, text }
    }

    pub fn open(&self, quote: &str, note: &str) {
        self.quote.set_label(quote.trim());
        self.text.buffer().set_text(note);
        self.root.set_visible(true);
        self.text.grab_focus();
    }

    pub fn text(&self) -> String {
        let buffer = self.text.buffer();
        buffer.text(&buffer.start_iter(), &buffer.end_iter(), false).to_string()
    }

    pub fn close(&self) {
        self.root.set_visible(false);
    }

    pub fn is_open(&self) -> bool {
        self.root.is_visible()
    }
}

/// The list of highlights, or of bookmarks.
pub struct AnnotationsPanel {
    pub root: gtk::Box,
    title: gtk::Label,
    list: gtk::ListBox,
    empty: gtk::Label,
    items: RefCell<Vec<Annotation>>,
    kind: Cell<Kind>,
    cursor: Cell<i32>,
}

impl AnnotationsPanel {
    pub fn new() -> Rc<AnnotationsPanel> {
        let title = gtk::Label::builder().xalign(0.0).css_classes(["title"]).build();
        let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::Single).css_classes(["annotation-list"]).build();
        let scroller = gtk::ScrolledWindow::builder().child(&list).vexpand(true).hscrollbar_policy(gtk::PolicyType::Never).build();
        let empty = gtk::Label::builder().xalign(0.0).wrap(true).css_classes(["caption"]).build();
        let hint = gtk::Label::builder()
            .label("j/k move   enter go   a note   dd delete   q close")
            .xalign(0.0)
            .css_classes(["caption"])
            .build();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .halign(gtk::Align::End)
            .valign(gtk::Align::Fill)
            .visible(false)
            .css_classes(["card", "side-panel", "annotations-panel"])
            .build();
        root.append(&title);
        root.append(&empty);
        root.append(&scroller);
        root.append(&hint);
        Rc::new(AnnotationsPanel {
            root,
            title,
            list,
            empty,
            items: RefCell::default(),
            kind: Cell::new(Kind::Highlight),
            cursor: Cell::new(0),
        })
    }

    pub fn is_open(&self) -> bool {
        self.root.is_visible()
    }

    pub fn open(&self, kind: Kind, all: &[Annotation]) {
        self.kind.set(kind);
        self.title.set_label(match kind {
            Kind::Highlight => "Highlights",
            Kind::Bookmark => "Bookmarks",
        });
        self.set(all);
        self.root.set_visible(true);
        self.list.grab_focus();
        self.select(self.cursor.get().min(self.items.borrow().len() as i32 - 1).max(0));
    }

    /// Refresh the rows (after a change), keeping the cursor.
    pub fn set(&self, all: &[Annotation]) {
        let kind = self.kind.get();
        let items: Vec<Annotation> = all.iter().filter(|a| a.kind == kind).cloned().collect();
        while let Some(row) = self.list.row_at_index(0) {
            self.list.remove(&row);
        }
        for a in &items {
            self.list.append(&row_for(a));
        }
        self.empty.set_visible(items.is_empty());
        self.empty.set_label(match kind {
            Kind::Highlight => "No highlights yet. Select text and press h.",
            Kind::Bookmark => "No bookmarks yet. Press m on a page.",
        });
        let n = items.len() as i32;
        self.items.replace(items);
        if self.is_open() && n > 0 {
            self.select(self.cursor.get().clamp(0, n - 1));
        }
    }

    pub fn close(&self) {
        self.root.set_visible(false);
    }

    pub fn move_cursor(&self, delta: i32) {
        let n = self.items.borrow().len() as i32;
        if n > 0 {
            self.select((self.cursor.get() + delta).clamp(0, n - 1));
        }
    }

    fn select(&self, i: i32) {
        self.cursor.set(i);
        if let Some(row) = self.list.row_at_index(i) {
            self.list.select_row(Some(&row));
            row.grab_focus();
        }
    }

    pub fn selected(&self) -> Option<Annotation> {
        self.items.borrow().get(self.cursor.get() as usize).cloned()
    }
}

fn row_for(a: &Annotation) -> gtk::ListBoxRow {
    let row = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(2).build();
    let chapter = if a.chapter.is_empty() { format!("{}%", (a.fraction * 100.0).round()) } else { a.chapter.clone() };
    let head = gtk::Label::builder()
        .label(chapter)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["caption"])
        .build();
    row.append(&head);
    if !a.text.trim().is_empty() {
        let swatch = format!("swatch-{}", a.color);
        let text = gtk::Label::builder()
            .label(a.text.trim())
            .xalign(0.0)
            .wrap(true)
            .lines(3)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["excerpt", swatch.as_str()])
            .build();
        row.append(&text);
    }
    if !a.note.trim().is_empty() {
        let note = gtk::Label::builder()
            .label(a.note.trim())
            .xalign(0.0)
            .wrap(true)
            .lines(2)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["note"])
            .build();
        row.append(&note);
    }
    gtk::ListBoxRow::builder().child(&row).build()
}

//! Highlights and bookmarks on screen: the bar over a selection, the note
//! editor, and the panel listing them.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use omaread_core::annotations::{Annotation, Kind};

use crate::reader::Rect;

/// A click in the selection card, for the window to act on.
#[derive(Debug, Clone)]
pub enum SelectionAction {
    /// One of the five highlight colours, by number (0–4).
    Color(usize),
    /// `h`: highlight the selection, or remove the highlight it's about.
    Toggle,
    /// The note field was saved (Enter).
    Note(String),
    Copy,
    Delete,
}

/// The five highlight colours, in the order the number keys pick them.
const SWATCHES: [(&str, &str); 5] =
    [("yellow", "yellow"), ("red", "red"), ("green", "green"), ("blue", "blue"), ("magenta", "magenta")];

/// What the card is showing, so one card serves a fresh selection and a
/// highlight that was clicked.
pub struct SelectionInfo<'a> {
    /// The highlight's note, if it has one.
    pub note: &'a str,
    /// The highlight's colour, if it is one.
    pub color: Option<&'a str>,
    /// True when the card is about a highlight that already exists.
    pub existing: bool,
    /// The keys that work right now.
    pub hint: &'a str,
    pub rect: Option<Rect>,
}

/// The card that follows a selection (or a clicked highlight): its colours, a
/// note field, and the actions, all clickable, with the keys beside them.
pub struct SelectionBar {
    pub root: gtk::Box,
    swatches: Vec<gtk::Button>,
    primary: gtk::Button,
    note: gtk::Entry,
    delete: gtk::Button,
    hint: gtk::Label,
}

const BAR_WIDTH: i32 = 460;

impl SelectionBar {
    pub fn new(on_action: impl Fn(SelectionAction) + 'static) -> SelectionBar {
        let on_action = Rc::new(on_action);

        let colors = gtk::Box::builder().spacing(6).build();
        let swatches: Vec<gtk::Button> = SWATCHES
            .iter()
            .enumerate()
            .map(|(i, (name, label))| {
                let button = gtk::Button::builder()
                    .tooltip_text(format!("{label} ({})", i + 1))
                    .hexpand(false)
                    .vexpand(false)
                    .valign(gtk::Align::Center)
                    .css_classes(["swatch", *name])
                    .build();
                button.connect_clicked({
                    let on_action = on_action.clone();
                    move |_| on_action(SelectionAction::Color(i))
                });
                colors.append(&button);
                button
            })
            .collect();
        let primary = gtk::Button::builder().label("Highlight").valign(gtk::Align::Center).css_classes(["primary"]).build();
        primary.connect_clicked({
            let on_action = on_action.clone();
            move |_| on_action(SelectionAction::Toggle)
        });
        colors.append(&gtk::Box::builder().hexpand(true).build());
        colors.append(&primary);

        let note = gtk::Entry::builder()
            .placeholder_text("Add a note…")
            .hexpand(true)
            .css_classes(["selection-note"])
            .build();
        note.connect_activate({
            let on_action = on_action.clone();
            move |entry| on_action(SelectionAction::Note(entry.text().trim().to_string()))
        });

        let actions = gtk::Box::builder().spacing(6).build();
        actions.append(&action_button("Copy", "y copy", &on_action, SelectionAction::Copy));
        let delete = action_button("Delete", "x delete", &on_action, SelectionAction::Delete);
        delete.set_visible(false);
        actions.append(&delete);

        let hint = gtk::Label::builder().xalign(0.0).wrap(true).max_width_chars(54).css_classes(["caption"]).build();

        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Start)
            .width_request(BAR_WIDTH)
            .visible(false)
            .css_classes(["card", "selection-bar"])
            .build();
        root.append(&colors);
        root.append(&note);
        root.append(&actions);
        root.append(&hint);
        SelectionBar { root, swatches, primary, note, delete, hint }
    }

    /// Show below the text (above it near the bottom), inside the page.
    pub fn show(&self, info: SelectionInfo, page: (i32, i32)) {
        self.primary.set_label(if info.existing { "Remove" } else { "Highlight" });
        self.delete.set_visible(info.existing);
        for (button, (name, _)) in self.swatches.iter().zip(SWATCHES) {
            if info.color == Some(name) {
                button.add_css_class("active");
            } else {
                button.remove_css_class("active");
            }
        }
        self.note.set_text(info.note);
        self.hint.set_label(info.hint);
        let (w, h) = page;
        let r = info.rect.unwrap_or(Rect { x: (w - BAR_WIDTH) / 2, y: h / 2, width: 0, height: 0 });
        let x = r.x.clamp(8, (w - BAR_WIDTH - 8).max(8));
        let below = r.y + r.height + 10;
        let y = if below + 210 > h { (r.y - 200).max(8) } else { below };
        self.root.set_margin_start(x);
        self.root.set_margin_top(y);
        self.root.set_visible(true);
    }

    /// True while the note field has the keyboard, so keys become text.
    pub fn note_has_focus(&self) -> bool {
        self.note.has_focus()
    }

    /// What's in the note field.
    pub fn note(&self) -> String {
        self.note.text().to_string()
    }

    pub fn hide(&self) {
        self.root.set_visible(false);
    }

    pub fn is_open(&self) -> bool {
        self.root.is_visible()
    }
}

fn action_button<F: Fn(SelectionAction) + 'static>(
    label: &str,
    tooltip: &str,
    on_action: &Rc<F>,
    action: SelectionAction,
) -> gtk::Button {
    let button = gtk::Button::builder().label(label).tooltip_text(tooltip).css_classes(["action"]).build();
    button.connect_clicked({
        let on_action = on_action.clone();
        move |_| on_action(action.clone())
    });
    button
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

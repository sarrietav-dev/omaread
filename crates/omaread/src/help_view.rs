//! Context-aware shortcut help, using the active keymap.

use std::rc::Rc;
use gtk::prelude::*;
use crate::keys::{Action, Context, Keymap};

type Row = (&'static str, Action, &'static [&'static str]);

fn sections(context: Context) -> Vec<(&'static str, Context, Vec<Row>)> {
    use Action::*;
    if context == Context::Library {
        vec![("Library", Context::Library, vec![
            ("Move left", Move(-1, 0), &["h", "left"]), ("Move right", Move(1, 0), &["l", "right"]),
            ("Move down", Move(0, 1), &["j", "down"]), ("Move up", Move(0, -1), &["k", "up"]),
            ("Open book", Open, &["enter"]), ("Filter books", Filter, &["/"]),
            ("Cycle sort", Sort, &["o"]), ("Next folder", NextShelf, &["tab"]),
            ("Previous folder", PrevShelf, &["shift+tab"]), ("Open / close folder", ToggleFolder, &["space"]),
            ("Add folder", AddFolder, &["a"]), ("Remove folder (files stay)", RemoveFolder, &["D"]),
            ("First book", First, &["gg", "home"]), ("Last book", Last, &["G", "end"]),
            ("Rescan", Rescan, &["gr"]), ("Command palette", CommandLine, &[":"]),
            ("Help", Help, &["?"]), ("Close library", Close, &["q"]), ("Quit", Quit, &["ctrl+q"]),
        ])]
    } else {
        vec![
            ("Reading", Context::Reader, vec![
                ("Next page", NextPage, &["j", "l", "space", "right", "down", "page_down"]),
                ("Previous page", PrevPage, &["k", "h", "shift+space", "left", "up", "page_up"]),
                ("Next chapter", NextSection, &["J", "L"]), ("Previous chapter", PrevSection, &["K", "H"]),
                ("Start of book", First, &["gg", "home"]), ("End of book", Last, &["G", "end"]),
                ("Back", Back, &["ctrl+o"]), ("Forward", Forward, &["ctrl+i"]),
                ("Contents", Toc, &["t"]), ("Search", Search, &["/"]),
                ("Next search result", NextHit, &["n"]), ("Previous search result", PrevHit, &["N"]),
                ("Bookmark page", ToggleBookmark, &["m"]), ("Bookmarks", Bookmarks, &["'"]),
                ("Highlights and notes", Highlights, &["A"]), ("Reading settings", Settings, &[","]),
                ("Bigger text", Bigger, &["+", "="]), ("Smaller text", Smaller, &["-"]),
                ("Cycle theme", CycleTheme, &["T"]), ("Status line", ToggleStatus, &["i"]),
                ("Command palette", CommandLine, &[":"]), ("Help", Help, &["?"]),
                ("Back to library", Close, &["q"]),
            ]),
            ("Reading aloud", Context::Reader, vec![
                ("Play / pause", Speak, &["s"]), ("Next sentence", SentenceNext, &["]"]),
                ("Previous sentence", SentencePrev, &["["]), ("Faster", Faster, &[">"]),
                ("Slower", Slower, &["<"]), ("Choose voice", Voices, &["V"]),
                ("Stop reading aloud", ClearOrClose, &["escape"]),
            ]),
            ("Selection", Context::Selection, vec![
                ("Highlight / remove highlight", Highlight, &["h"]), ("Add note", Note, &["a"]),
                ("Copy", Copy, &["y"]), ("Delete highlight", Delete, &["x"]),
                ("Read from selection", SpeakFrom, &["s"]),
            ]),
            ("Contents", Context::Toc, vec![
                ("Next entry", Move(0, 1), &["j", "down"]), ("Previous entry", Move(0, -1), &["k", "up"]),
                ("Fold", Move(-1, 0), &["h", "left"]), ("Expand", Move(1, 0), &["l", "right"]),
                ("Go to entry", Open, &["enter"]), ("Close contents", Close, &["t", "q", "escape"]),
            ]),
            ("Reading settings", Context::Settings, vec![
                ("Next setting", Move(0, 1), &["j", "down"]), ("Previous setting", Move(0, -1), &["k", "up"]),
                ("Decrease / previous", Move(-1, 0), &["h", "left"]),
                ("Increase / next", Move(1, 0), &["l", "right", "enter"]),
                ("Close settings", Close, &[",", "q", "escape"]),
            ]),
            ("Annotations", Context::Annotations, vec![
                ("Next entry", Move(0, 1), &["j", "down"]), ("Previous entry", Move(0, -1), &["k", "up"]),
                ("Go to entry", Open, &["enter"]), ("Add note", Note, &["a"]),
                ("Delete entry", Delete, &["dd", "x", "delete"]), ("Close", Close, &["q", "A", "'", "escape"]),
            ]),
        ]
    }
}

pub struct HelpView {
    pub root: adw::Clamp,
    content: gtk::Box,
    scroller: gtk::ScrolledWindow,
}

impl HelpView {
    pub fn new() -> Rc<HelpView> {
        let content = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(2).build();
        let scroller = gtk::ScrolledWindow::builder().child(&content).hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true).max_content_height(560).vexpand(true).build();
        let title = gtk::Label::builder().label("Keyboard shortcuts").xalign(0.0).css_classes(["title"]).build();
        let hint = gtk::Label::builder().label("j/k scroll   ? or Esc close   ·   Rebind in config.toml")
            .xalign(0.0).wrap(true).css_classes(["caption"]).build();
        let card = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(8)
            .valign(gtk::Align::Fill).css_classes(["card", "help"]).build();
        card.append(&title);
        card.append(&scroller);
        card.append(&hint);
        let root = adw::Clamp::builder().maximum_size(760).child(&card).halign(gtk::Align::Fill)
            .valign(gtk::Align::Fill).margin_top(20).margin_bottom(20).margin_start(12).margin_end(12).visible(false).build();
        Rc::new(HelpView { root, content, scroller })
    }

    pub fn is_open(&self) -> bool { self.root.is_visible() }

    pub fn open(&self, context: Context, keymap: &Keymap) {
        while let Some(child) = self.content.first_child() { self.content.remove(&child); }
        for (section, context, rows) in sections(context) {
            self.content.append(&gtk::Label::builder().label(section).xalign(0.0).css_classes(["heading"]).build());
            for (what, action, defaults) in rows {
                let keys = keymap.labels(context, action, defaults);
                let key = gtk::Label::builder().label(&keys).xalign(0.0).wrap(true).max_width_chars(24).css_classes(["key"]).build();
                let what = gtk::Label::builder().label(what).xalign(0.0).wrap(true).hexpand(true).css_classes(["what"]).build();
                let row = gtk::Box::builder().spacing(16).build();
                row.append(&key);
                row.append(&what);
                self.content.append(&row);
            }
        }
        if context != Context::Library {
            self.content.append(&gtk::Label::builder()
                .label("With text selected: 1–5 choose a highlight colour; d, w and t open the default lookup providers.")
                .xalign(0.0).wrap(true).css_classes(["caption"]).build());
        }
        self.root.set_visible(true);
        self.scroller.vadjustment().set_value(0.0);
    }

    pub fn close(&self) { self.root.set_visible(false); }
    pub fn scroll(&self, lines: i32) {
        let adjustment = self.scroller.vadjustment();
        adjustment.set_value(adjustment.value() + lines as f64 * adjustment.step_increment().max(24.0));
    }
    pub fn top(&self) { self.scroller.vadjustment().set_value(0.0); }
    pub fn bottom(&self) {
        let adjustment = self.scroller.vadjustment();
        adjustment.set_value(adjustment.upper() - adjustment.page_size());
    }
}

//! The `?` help: every binding, in one panel over the book or the library.

use std::rc::Rc;

use gtk::prelude::*;

/// (section, [(keys, what)]). The defaults; `[keymap]` in the config rebinds
/// them, and the panel says so.
const SECTIONS: &[(&str, &[(&str, &str)])] = &[
    (
        "Moving",
        &[
            ("j l space →", "next page"),
            ("k h shift+space ←", "previous page"),
            ("J K / H L", "next / previous chapter"),
            ("gg / G", "start / end of the book"),
            ("ctrl+o / ctrl+i", "back / forward"),
        ],
    ),
    (
        "Panels",
        &[
            ("t", "contents"),
            ("/", "search the book"),
            ("n / N", "next / previous hit"),
            ("'", "bookmarks"),
            ("A", "highlights and notes"),
            ("i", "status line"),
            (",", "reading settings"),
            (":", "command palette"),
            ("?", "this help"),
            ("q / esc", "close a panel"),
        ],
    ),
    (
        "Selection",
        &[
            ("h", "highlight / remove it"),
            ("1 – 5", "highlight colour"),
            ("a", "add a note"),
            ("y", "copy"),
            ("x", "remove a highlight"),
            ("d / w / t", "dictionary / Wikipedia / translate"),
            ("s", "read from here"),
            ("mouse", "the card: colour, note, copy"),
        ],
    ),
    (
        "Reading aloud",
        &[
            ("s", "play / pause"),
            ("] / [", "next / previous sentence"),
            ("> / <", "faster / slower"),
            ("V", "voices"),
            ("esc", "stop"),
        ],
    ),
    (
        "Page",
        &[
            ("m", "bookmark this page"),
            ("+ / -", "bigger / smaller text"),
            ("T", "cycle theme"),
        ],
    ),
    (
        "Library",
        &[
            ("h j k l", "move"),
            ("enter", "open"),
            ("/", "filter"),
            ("o", "sort"),
            ("tab / shift+tab", "next / previous folder"),
            ("space", "open / close the folder"),
            ("a", "add a folder"),
            ("D", "remove the folder (files stay)"),
            ("gg / G", "first / last book"),
            ("gr", "rescan"),
            (":", "command palette"),
            ("q", "quit"),
        ],
    ),
];

pub struct HelpView {
    pub root: gtk::Box,
    scroller: gtk::ScrolledWindow,
}

impl HelpView {
    pub fn new() -> Rc<HelpView> {
        let content = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(2).build();
        for (section, rows) in SECTIONS {
            let heading = gtk::Label::builder().label(*section).xalign(0.0).css_classes(["heading"]).build();
            content.append(&heading);
            for (keys, what) in *rows {
                let key = gtk::Label::builder().label(*keys).xalign(0.0).css_classes(["key"]).build();
                let what = gtk::Label::builder().label(*what).xalign(0.0).hexpand(true).css_classes(["what"]).build();
                let row = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).spacing(16).build();
                row.append(&key);
                row.append(&what);
                content.append(&row);
            }
        }

        let scroller = gtk::ScrolledWindow::builder()
            .child(&content)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(560)
            .build();
        let title = gtk::Label::builder().label("Keys").xalign(0.0).css_classes(["title"]).build();
        let hint = gtk::Label::builder()
            .label("j/k scroll   ? close   —   rebind any of these in config.toml")
            .xalign(0.0)
            .css_classes(["caption"])
            .build();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .width_request(760)
            .visible(false)
            .css_classes(["card", "help"])
            .build();
        root.append(&title);
        root.append(&scroller);
        root.append(&hint);
        Rc::new(HelpView { root, scroller })
    }

    pub fn is_open(&self) -> bool {
        self.root.is_visible()
    }

    pub fn open(&self) {
        self.root.set_visible(true);
        self.scroller.vadjustment().set_value(0.0);
    }

    pub fn close(&self) {
        self.root.set_visible(false);
    }

    /// Scroll by whole lines (`j`/`k`), or a pageful (PageUp/PageDown).
    pub fn scroll(&self, lines: i32) {
        let adjustment = self.scroller.vadjustment();
        let step = adjustment.step_increment().max(24.0);
        adjustment.set_value(adjustment.value() + lines as f64 * step);
    }

    pub fn top(&self) {
        self.scroller.vadjustment().set_value(0.0);
    }

    pub fn bottom(&self) {
        let adjustment = self.scroller.vadjustment();
        adjustment.set_value(adjustment.upper() - adjustment.page_size());
    }
}

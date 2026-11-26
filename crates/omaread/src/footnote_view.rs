//! A footnote in a card by the link that points at it, so reading a note
//! doesn't lose the page.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;

use crate::reader::Rect;

const WIDTH: i32 = 440;

pub struct FootnoteView {
    pub root: gtk::Box,
    body: gtk::Label,
    scroller: gtk::ScrolledWindow,
    /// Where the note lives, for "go to the note".
    href: RefCell<Option<String>>,
}

impl FootnoteView {
    pub fn new() -> Rc<FootnoteView> {
        let title = gtk::Label::builder().label("Note").xalign(0.0).css_classes(["caption"]).build();
        let body = gtk::Label::builder()
            .xalign(0.0)
            .yalign(0.0)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .selectable(true)
            .css_classes(["lookup-body"])
            .build();
        let scroller = gtk::ScrolledWindow::builder()
            .child(&body)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(320)
            .build();
        let hint = gtk::Label::builder()
            .label("enter go to the note   y copy   j/k scroll   esc close")
            .xalign(0.0)
            .wrap(true)
            .css_classes(["caption"])
            .build();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Start)
            .width_request(WIDTH)
            .visible(false)
            .css_classes(["card", "footnote"])
            .build();
        root.append(&title);
        root.append(&scroller);
        root.append(&hint);
        Rc::new(FootnoteView { root, body, scroller, href: RefCell::default() })
    }

    pub fn is_open(&self) -> bool {
        self.root.is_visible()
    }

    /// Show the note below its link (above it near the bottom of the page).
    pub fn show(&self, href: &str, text: &str, rect: Option<Rect>, page: (i32, i32)) {
        self.href.replace(Some(href.to_string()));
        self.body.set_label(text);
        self.scroller.vadjustment().set_value(0.0);
        let (w, h) = page;
        let width = WIDTH.min(w - 16).max(200);
        self.root.set_width_request(width);
        let r = rect.unwrap_or(Rect { x: (w - width) / 2, y: h / 3, width: 0, height: 0 });
        let x = (r.x - 16).clamp(8, (w - width - 8).max(8));
        let below = r.y + r.height + 8;
        let y = if below + 240 > h { (r.y - 240).max(8) } else { below };
        self.root.set_margin_start(x);
        self.root.set_margin_top(y);
        self.root.set_visible(true);
    }

    pub fn href(&self) -> Option<String> {
        self.href.borrow().clone()
    }

    pub fn text(&self) -> String {
        self.body.label().to_string()
    }

    pub fn scroll(&self, delta: i32) {
        let adj = self.scroller.vadjustment();
        adj.set_value(adj.value() + delta as f64 * adj.step_increment().max(40.0));
    }

    pub fn close(&self) {
        self.root.set_visible(false);
    }
}

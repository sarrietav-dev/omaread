//! The lookup card: what a word means, who someone is, how it reads in the
//! other language. One tab per provider, switched with its key.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use omaread_core::lookup::{LookupResult, ProviderSpec};

pub struct LookupView {
    pub root: gtk::Box,
    tabs: gtk::Box,
    title: gtk::Label,
    body: gtk::Label,
    scroller: gtk::ScrolledWindow,
    hint: gtk::Label,
    providers: Vec<ProviderSpec>,
    /// The provider showing.
    current: Cell<usize>,
    /// What's being looked up, and in what language.
    query: RefCell<(String, Option<String>)>,
    /// The answer showing, for `o` and `y`.
    result: RefCell<Option<LookupResult>>,
}

impl LookupView {
    pub fn new(providers: Vec<ProviderSpec>) -> Rc<LookupView> {
        let tabs = gtk::Box::builder().spacing(12).css_classes(["lookup-tabs"]).build();
        for p in &providers {
            let tab = gtk::Label::builder().label(format!("{} {}", p.key, p.name)).css_classes(["tab"]).build();
            tabs.append(&tab);
        }
        let title = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .selectable(true)
            .css_classes(["title"])
            .build();
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
            .max_content_height(420)
            .build();
        let hint = gtk::Label::builder().xalign(0.0).css_classes(["caption"]).build();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .halign(gtk::Align::End)
            .valign(gtk::Align::Start)
            .width_request(460)
            .visible(false)
            .css_classes(["card", "side-panel", "lookup"])
            .build();
        root.append(&tabs);
        root.append(&title);
        root.append(&scroller);
        root.append(&hint);
        Rc::new(LookupView {
            root,
            tabs,
            title,
            body,
            scroller,
            hint,
            providers,
            current: Cell::new(0),
            query: RefCell::default(),
            result: RefCell::default(),
        })
    }

    pub fn providers(&self) -> &[ProviderSpec] {
        &self.providers
    }

    pub fn provider_for_key(&self, key: char) -> Option<usize> {
        self.providers.iter().position(|p| p.key.chars().eq(std::iter::once(key)))
    }

    pub fn is_open(&self) -> bool {
        self.root.is_visible()
    }

    pub fn query(&self) -> (String, Option<String>) {
        self.query.borrow().clone()
    }

    /// Show the card, waiting for provider `index` to answer `text`.
    pub fn open(&self, index: usize, text: &str, lang: Option<&str>) {
        self.current.set(index);
        self.query.replace((text.to_string(), lang.map(String::from)));
        self.result.take();
        for (i, tab) in std::iter::successors(self.tabs.first_child(), |w| w.next_sibling()).enumerate() {
            if i == index {
                tab.add_css_class("active");
            } else {
                tab.remove_css_class("active");
            }
        }
        let short: String = text.chars().take(80).collect();
        self.title.set_label(&short);
        self.body.set_label("Looking up…");
        self.body.remove_css_class("error");
        self.hint.set_label("esc close");
        self.root.set_visible(true);
        self.scroller.vadjustment().set_value(0.0);
    }

    /// An answer arrived; ignored if the card has moved on.
    pub fn show(&self, index: usize, text: &str, result: Result<LookupResult, String>) {
        if !self.is_open() || index != self.current.get() || self.query.borrow().0 != text {
            return;
        }
        match result {
            Ok(r) => {
                if !r.title.is_empty() {
                    self.title.set_label(&r.title);
                }
                self.body.set_label(&r.body);
                let mut hints = Vec::new();
                if r.url.is_some() {
                    hints.push("o open");
                }
                hints.extend(["y copy", "j/k scroll", "esc close"]);
                self.hint.set_label(&hints.join("   "));
                self.result.replace(Some(r));
            }
            Err(e) => {
                self.body.set_label(&e);
                self.body.add_css_class("error");
            }
        }
    }

    pub fn result(&self) -> Option<LookupResult> {
        self.result.borrow().clone()
    }

    pub fn scroll(&self, delta: i32) {
        let adj = self.scroller.vadjustment();
        adj.set_value(adj.value() + delta as f64 * adj.step_increment().max(40.0));
    }

    pub fn close(&self) {
        self.root.set_visible(false);
        self.result.take();
    }
}

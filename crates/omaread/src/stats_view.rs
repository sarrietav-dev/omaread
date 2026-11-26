//! `:stats`: how much you've read, as a card over the book or the library.

use std::rc::Rc;

use gtk::prelude::*;
use omaread_core::stats::{Report, minutes_text, sparkline};

pub struct StatsView {
    pub root: adw::Clamp,
    content: gtk::Box,
    scroller: gtk::ScrolledWindow,
}

impl StatsView {
    pub fn new() -> Rc<StatsView> {
        let content = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(4).build();
        let scroller = gtk::ScrolledWindow::builder()
            .child(&content)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(560)
            .build();
        let title = gtk::Label::builder().label("Reading").xalign(0.0).css_classes(["title"]).build();
        let hint = gtk::Label::builder().label("j/k scroll   esc close   ·   omaread stats --json").xalign(0.0).css_classes(["caption"]).build();
        let card = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(8).valign(gtk::Align::Center).css_classes(["card", "help"]).build();
        card.append(&title);
        card.append(&scroller);
        card.append(&hint);
        let root = adw::Clamp::builder()
            .maximum_size(560)
            .child(&card)
            .halign(gtk::Align::Fill)
            .valign(gtk::Align::Center)
            .margin_top(20)
            .margin_bottom(20)
            .margin_start(12)
            .margin_end(12)
            .visible(false)
            .build();
        Rc::new(StatsView { root, content, scroller })
    }

    pub fn is_open(&self) -> bool {
        self.root.is_visible()
    }

    pub fn open(&self, r: &Report) {
        while let Some(child) = self.content.first_child() {
            self.content.remove(&child);
        }
        let row = |what: &str, value: &str| {
            let what = gtk::Label::builder().label(what).xalign(0.0).hexpand(true).css_classes(["what"]).build();
            let value = gtk::Label::builder().label(value).xalign(1.0).css_classes(["key"]).build();
            let row = gtk::Box::builder().spacing(16).build();
            row.append(&what);
            row.append(&value);
            self.content.append(&row);
        };
        let heading = |text: &str| self.content.append(&gtk::Label::builder().label(text).xalign(0.0).css_classes(["heading"]).build());
        row("Today", &minutes_text(r.today_minutes));
        row("Last 7 days", &minutes_text(r.week_minutes));
        row("In all", &minutes_text(r.total_minutes));
        let days = |n: i64| if n == 1 { "1 day".to_string() } else { format!("{n} days") };
        row("Streak", &days(r.streak));
        row("Longest streak", &days(r.longest_streak));
        row(&format!("Last {} days", r.days.len()), &sparkline(&r.days));
        if !r.books.is_empty() {
            heading("Most read");
            for b in &r.books {
                row(&b.title, &minutes_text(b.minutes));
            }
        }
        heading(&format!("Finished this year: {}", r.finished_this_year));
        for f in &r.finished {
            let (y, m, d) = omaread_core::stats::civil_from_days(f.at.div_euclid(86_400));
            row(&f.title, &format!("{y:04}-{m:02}-{d:02}"));
        }
        self.scroller.vadjustment().set_value(0.0);
        self.root.set_visible(true);
    }

    pub fn scroll(&self, lines: i32) {
        let adj = self.scroller.vadjustment();
        adj.set_value(adj.value() + lines as f64 * adj.step_increment().max(24.0));
    }

    pub fn close(&self) {
        self.root.set_visible(false);
    }
}

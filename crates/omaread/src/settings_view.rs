//! The reading settings panel: a list of fields over the book, grouped into
//! sections, changed with h/l and applied as you go.
//!
//! Numeric fields carry a bar so you can see where a value sits in its range;
//! toggles read on/off; the scope is a two-way switch at the top. The line
//! under the list explains whatever the cursor is on.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use omaread_core::prefs::{self, Flow, Publisher, ReaderPrefs};
use omaread_core::themes::{self, ThemeChoice};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    AllBooks,
    ThisBook,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Scope,
    Theme,
    Font,
    Size,
    LineHeight,
    Justify,
    Hyphenate,
    Publisher,
    Margin,
    Width,
    Columns,
    Flow,
    Reset,
}

const FIELDS: [Field; 13] = [
    Field::Scope,
    Field::Theme,
    Field::Font,
    Field::Size,
    Field::LineHeight,
    Field::Justify,
    Field::Hyphenate,
    Field::Publisher,
    Field::Margin,
    Field::Width,
    Field::Columns,
    Field::Flow,
    Field::Reset,
];

/// The heading a field opens, or `None` when a section is already running.
fn section_of(field: Field) -> Option<&'static str> {
    match field {
        Field::Theme => Some("Style"),
        Field::Margin => Some("Page"),
        _ => None,
    }
}

/// Families worth offering, in order, when they're installed.
const FONTS: [&str; 16] = [
    "Literata",
    "Libertinus Serif",
    "Source Serif 4",
    "Merriweather",
    "EB Garamond",
    "Crimson Pro",
    "Charter",
    "Noto Serif",
    "Liberation Serif",
    "DejaVu Serif",
    "Inter",
    "Noto Sans",
    "Source Sans 3",
    "Liberation Sans",
    "DejaVu Sans",
    "monospace",
];

pub struct SettingsView {
    pub root: gtk::Box,
    list: gtk::ListBox,
    /// The value label per field, in `FIELDS` order.
    values: Vec<gtk::Label>,
    /// The range bar per field, where the field is numeric.
    bars: Vec<Option<gtk::ProgressBar>>,
    /// The two scope chips, "all books" then "this book".
    scope_chips: Vec<gtk::Label>,
    /// The list row each field lives on (heading rows take an index too).
    rows: Vec<usize>,
    scope_label: gtk::Label,
    note: gtk::Label,
    cursor: Cell<usize>,
    scope: Cell<Scope>,
    prefs: RefCell<ReaderPrefs>,
    themes: Vec<ThemeChoice>,
    fonts: Vec<String>,
    on_change: Box<dyn Fn(Scope, Option<ReaderPrefs>)>,
}

impl SettingsView {
    /// `on_change(scope, prefs)`: apply and store; `None` resets the book to the global set.
    pub fn new(on_change: impl Fn(Scope, Option<ReaderPrefs>) + 'static) -> Rc<SettingsView> {
        let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::Single).css_classes(["settings-list"]).build();

        let mut values = Vec::new();
        let mut bars = Vec::new();
        let mut scope_chips = Vec::new();
        let mut rows = Vec::new();
        let mut row_index = 0usize;
        for field in FIELDS {
            if let Some(heading) = section_of(field) {
                list.append(&heading_row(heading));
                row_index += 1;
            }

            let name = gtk::Label::builder().label(label_of(field)).xalign(0.0).hexpand(true).css_classes(["name"]).build();
            let value = gtk::Label::builder().xalign(1.0).css_classes(["value"]).build();
            let top = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).spacing(12).build();
            top.append(&name);
            if field == Field::Scope {
                let chips = gtk::Box::builder().spacing(6).halign(gtk::Align::End).css_classes(["scope-chips"]).build();
                for text in ["all books", "this book"] {
                    let chip = gtk::Label::builder().label(text).css_classes(["chip"]).build();
                    chips.append(&chip);
                    scope_chips.push(chip);
                }
                top.append(&chips);
            } else {
                top.append(&value);
            }

            let body = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(4).build();
            body.append(&top);
            let bar = (field != Field::Scope && is_numeric(field)).then(|| {
                let bar = gtk::ProgressBar::builder().show_text(false).valign(gtk::Align::Center).css_classes(["range"]).build();
                body.append(&bar);
                bar
            });

            list.append(&gtk::ListBoxRow::builder().child(&body).build());
            rows.push(row_index);
            row_index += 1;
            values.push(value);
            bars.push(bar);
        }

        let title = gtk::Label::builder().label("Reading").xalign(0.0).hexpand(true).css_classes(["title"]).build();
        let scope_label = gtk::Label::builder().xalign(1.0).css_classes(["caption", "scope-label"]).build();
        let head = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).spacing(12).build();
        head.append(&title);
        head.append(&scope_label);
        let note = gtk::Label::builder().xalign(0.0).wrap(true).css_classes(["caption", "settings-note"]).build();
        let hint = gtk::Label::builder()
            .label("j/k move   h/l change   , close")
            .xalign(0.0)
            .css_classes(["caption"])
            .build();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .halign(gtk::Align::End)
            .valign(gtk::Align::Start)
            .visible(false)
            .css_classes(["card", "side-panel"])
            .build();
        root.append(&head);
        root.append(&list);
        root.append(&note);
        root.append(&hint);

        let families: Vec<String> = root.pango_context().list_families().iter().map(|f| f.name().to_string()).collect();
        let mut fonts: Vec<String> = FONTS
            .iter()
            .filter(|f| **f == "monospace" || families.iter().any(|x| x.eq_ignore_ascii_case(f)))
            .map(|f| f.to_string())
            .collect();
        fonts.dedup();

        let view = Rc::new(SettingsView {
            root,
            list,
            values,
            bars,
            scope_chips,
            rows,
            scope_label,
            note,
            cursor: Cell::new(0),
            scope: Cell::new(Scope::AllBooks),
            prefs: RefCell::default(),
            themes: themes::choices(),
            fonts,
            on_change: Box::new(on_change),
        });
        view
    }

    pub fn is_open(&self) -> bool {
        self.root.is_visible()
    }

    pub fn open(&self, prefs: &ReaderPrefs, scope: Scope) {
        self.prefs.replace(prefs.clone());
        self.scope.set(scope);
        self.refresh();
        self.root.set_visible(true);
        self.select(self.cursor.get());
    }

    pub fn close(&self) {
        self.root.set_visible(false);
    }

    pub fn move_cursor(&self, delta: i32) {
        let n = FIELDS.len() as i32;
        let next = (self.cursor.get() as i32 + delta).clamp(0, n - 1) as usize;
        self.select(next);
    }

    fn select(&self, i: usize) {
        self.cursor.set(i);
        if let Some(row) = self.list.row_at_index(self.rows[i] as i32) {
            self.list.select_row(Some(&row));
            row.grab_focus();
        }
        self.note.set_label(description_of(FIELDS[i]));
    }

    /// h/l: step the field under the cursor. Enter acts like l.
    pub fn adjust(&self, step: i32) {
        let field = FIELDS[self.cursor.get()];
        if field == Field::Scope {
            match self.scope.get() {
                // The book starts from what's showing now.
                Scope::AllBooks => {
                    self.scope.set(Scope::ThisBook);
                    (self.on_change)(Scope::ThisBook, Some(self.prefs.borrow().clone()));
                    self.refresh();
                }
                // Back to the shared settings: the book's own go.
                Scope::ThisBook => (self.on_change)(Scope::ThisBook, None),
            }
            return;
        }
        if field == Field::Reset {
            if self.scope.get() == Scope::ThisBook {
                (self.on_change)(Scope::ThisBook, None);
            }
            return;
        }
        {
            let mut p = self.prefs.borrow_mut();
            step_field(&mut p, field, step, &self.themes, &self.fonts);
        }
        (self.on_change)(self.scope.get(), Some(self.prefs.borrow().clone()));
        self.refresh();
    }

    /// The effective prefs changed from outside (reset): show them.
    pub fn show(&self, prefs: &ReaderPrefs, scope: Scope) {
        self.prefs.replace(prefs.clone());
        self.scope.set(scope);
        self.refresh();
    }

    fn refresh(&self) {
        let p = self.prefs.borrow();
        for (i, field) in FIELDS.iter().enumerate() {
            let value = &self.values[i];
            value.remove_css_class("on");
            value.remove_css_class("off");
            value.remove_css_class("action");
            let text = match field {
                Field::Scope => String::new(),
                Field::Theme => themes::name_of(p.theme.as_deref()),
                Field::Font => p.font_family.clone().unwrap_or_else(|| "publisher".into()),
                Field::Size => format!("{} px", p.font_size),
                Field::LineHeight => format!("{:.1}", p.line_height),
                Field::Justify => {
                    value.add_css_class(on_off_class(p.justify));
                    on_off(p.justify)
                }
                Field::Hyphenate => {
                    value.add_css_class(on_off_class(p.hyphenate));
                    on_off(p.hyphenate)
                }
                Field::Publisher => match p.publisher {
                    Publisher::Keep => "keep".into(),
                    Publisher::Fonts => "override fonts".into(),
                    Publisher::All => "override all".into(),
                },
                Field::Margin => format!("{} px", p.margin),
                Field::Width => format!("{} px", p.max_width),
                Field::Columns => match p.columns {
                    0 => "auto".into(),
                    n => n.to_string(),
                },
                Field::Flow => match p.flow {
                    Flow::Paginated => "pages".into(),
                    Flow::Scrolled => "scroll".into(),
                },
                Field::Reset => {
                    value.add_css_class("action");
                    match self.scope.get() {
                        Scope::ThisBook => "enter".into(),
                        Scope::AllBooks => "—".into(),
                    }
                }
            };
            value.set_label(&if is_choice(*field) { format!("‹ {text} ›") } else { text });
            if let Some(bar) = &self.bars[i] {
                bar.set_fraction(fraction_of(*field, &p));
            }
        }
        self.scope_label.set_label(match self.scope.get() {
            Scope::AllBooks => "all books",
            Scope::ThisBook => "this book",
        });
        let active = match self.scope.get() {
            Scope::AllBooks => 0,
            Scope::ThisBook => 1,
        };
        for (i, chip) in self.scope_chips.iter().enumerate() {
            if i == active {
                chip.add_css_class("active");
            } else {
                chip.remove_css_class("active");
            }
        }
    }
}

fn heading_row(heading: &str) -> gtk::ListBoxRow {
    let label = gtk::Label::builder().label(heading).xalign(0.0).css_classes(["settings-heading"]).build();
    gtk::ListBoxRow::builder().child(&label).selectable(false).activatable(false).build()
}

fn label_of(field: Field) -> &'static str {
    match field {
        Field::Scope => "Applies to",
        Field::Theme => "Theme",
        Field::Font => "Font",
        Field::Size => "Size",
        Field::LineHeight => "Line height",
        Field::Justify => "Justify",
        Field::Hyphenate => "Hyphenate",
        Field::Publisher => "Publisher styles",
        Field::Margin => "Margins",
        Field::Width => "Max width",
        Field::Columns => "Columns",
        Field::Flow => "Layout",
        Field::Reset => "Use the defaults for this book",
    }
}

/// The one-line explanation under the list, for the field under the cursor.
fn description_of(field: Field) -> &'static str {
    match field {
        Field::Scope => "Apply changes to every book, or just this one",
        Field::Theme => "Follow the desktop, or wear a built-in palette",
        Field::Font => "The publisher's own font, or one installed here",
        Field::Size => "Height of the body text, in pixels",
        Field::LineHeight => "Line spacing, as a multiple of the size",
        Field::Justify => "Align text to both margins",
        Field::Hyphenate => "Break long words at the line's end",
        Field::Publisher => "How much of the book's own styling gives way",
        Field::Margin => "Space between the text and the page edge",
        Field::Width => "Widest a column of text is allowed to grow",
        Field::Columns => "Fit one or two columns, or fix the count",
        Field::Flow => "Turn pages, or scroll the chapter",
        Field::Reset => "Drop this book's own settings",
    }
}

fn on_off(b: bool) -> String {
    if b { "on".into() } else { "off".into() }
}

fn on_off_class(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

/// Where a value sits in its range, 0…1, for the field's bar.
fn fraction_of(field: Field, p: &ReaderPrefs) -> f64 {
    let (value, (lo, hi)) = match field {
        Field::Size => (p.font_size as f64, (prefs::FONT_SIZE.0 as f64, prefs::FONT_SIZE.1 as f64)),
        Field::LineHeight => (p.line_height, (prefs::LINE_HEIGHT.0, prefs::LINE_HEIGHT.1)),
        Field::Margin => (p.margin as f64, (prefs::MARGIN.0 as f64, prefs::MARGIN.1 as f64)),
        Field::Width => (p.max_width as f64, (prefs::MAX_WIDTH.0 as f64, prefs::MAX_WIDTH.1 as f64)),
        _ => return 0.0,
    };
    if hi <= lo { return 0.0 }
    ((value - lo) / (hi - lo)).clamp(0.0, 1.0)
}

fn is_numeric(field: Field) -> bool {
    matches!(field, Field::Size | Field::LineHeight | Field::Margin | Field::Width)
}

/// Choice fields step through a list, so they read `‹ value ›`.
fn is_choice(field: Field) -> bool {
    matches!(field, Field::Theme | Field::Font | Field::Columns | Field::Flow | Field::Publisher)
}

/// Step through a list, wrapping, from the current entry.
fn cycle<T: Clone + PartialEq>(options: &[T], current: &T, step: i32) -> T {
    let n = options.len() as i32;
    let i = options.iter().position(|o| o == current).map(|i| i as i32).unwrap_or(-1);
    let next = if i < 0 && step < 0 { n - 1 } else { (i + step).rem_euclid(n) };
    options[next as usize].clone()
}

fn step_field(p: &mut ReaderPrefs, field: Field, step: i32, themes: &[ThemeChoice], fonts: &[String]) {
    match field {
        Field::Theme => {
            let mut ids: Vec<Option<String>> = vec![None];
            ids.extend(themes.iter().map(|t| Some(t.id.clone())));
            p.theme = cycle(&ids, &p.theme, step);
        }
        Field::Font => {
            let mut options: Vec<Option<String>> = vec![None];
            options.extend(fonts.iter().cloned().map(Some));
            p.font_family = cycle(&options, &p.font_family, step);
        }
        Field::Size => p.font_size = (p.font_size as i32 + step).clamp(prefs::FONT_SIZE.0 as i32, prefs::FONT_SIZE.1 as i32) as u32,
        Field::LineHeight => {
            p.line_height = ((p.line_height * 10.0).round() + step as f64).clamp(prefs::LINE_HEIGHT.0 * 10.0, prefs::LINE_HEIGHT.1 * 10.0) / 10.0
        }
        Field::Justify => p.justify = !p.justify,
        Field::Hyphenate => p.hyphenate = !p.hyphenate,
        Field::Margin => p.margin = (p.margin as i32 + step * 8).clamp(prefs::MARGIN.0 as i32, prefs::MARGIN.1 as i32) as u32,
        Field::Width => p.max_width = (p.max_width as i32 + step * 40).clamp(prefs::MAX_WIDTH.0 as i32, prefs::MAX_WIDTH.1 as i32) as u32,
        Field::Columns => p.columns = cycle(&[0, 1, 2], &p.columns, step),
        Field::Flow => p.flow = cycle(&[Flow::Paginated, Flow::Scrolled], &p.flow, step),
        Field::Publisher => p.publisher = cycle(&[Publisher::Keep, Publisher::Fonts, Publisher::All], &p.publisher, step),
        Field::Scope | Field::Reset => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycling_wraps_both_ways() {
        assert_eq!(cycle(&[0, 1, 2], &2, 1), 0);
        assert_eq!(cycle(&[0, 1, 2], &0, -1), 2);
        assert_eq!(cycle(&[0, 1, 2], &9, 1), 0);
    }

    #[test]
    fn steps_stay_in_range() {
        let mut p = ReaderPrefs { font_size: 48, line_height: 2.4, ..Default::default() };
        step_field(&mut p, Field::Size, 1, &[], &[]);
        step_field(&mut p, Field::LineHeight, 1, &[], &[]);
        assert_eq!(p.font_size, 48);
        assert_eq!(p.line_height, 2.4);
        step_field(&mut p, Field::LineHeight, -1, &[], &[]);
        assert!((p.line_height - 2.3).abs() < 1e-9);
    }

    #[test]
    fn theme_cycles_from_omarchy_through_the_choices() {
        let themes = vec![ThemeChoice { id: "light".into(), name: "Light".into() }];
        let mut p = ReaderPrefs::default();
        step_field(&mut p, Field::Theme, 1, &themes, &[]);
        assert_eq!(p.theme.as_deref(), Some("light"));
        step_field(&mut p, Field::Theme, 1, &themes, &[]);
        assert_eq!(p.theme, None);
    }

    #[test]
    fn range_fractions_track_each_field() {
        let p = ReaderPrefs { font_size: prefs::FONT_SIZE.0, line_height: prefs::LINE_HEIGHT.1, ..Default::default() };
        assert_eq!(fraction_of(Field::Size, &p), 0.0);
        assert_eq!(fraction_of(Field::LineHeight, &p), 1.0);
        assert_eq!(fraction_of(Field::Theme, &p), 0.0);
    }
}

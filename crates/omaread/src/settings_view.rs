//! The reading settings panel: a list of fields over the book, changed with
//! h/l and applied as you go.

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
    Margin,
    Width,
    Columns,
    Flow,
    Publisher,
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
    Field::Margin,
    Field::Width,
    Field::Columns,
    Field::Flow,
    Field::Publisher,
    Field::Reset,
];

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
    values: Vec<gtk::Label>,
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
        for field in FIELDS {
            let name = gtk::Label::builder().label(label_of(field)).xalign(0.0).hexpand(true).build();
            let value = gtk::Label::builder().xalign(1.0).css_classes(["value"]).build();
            let row = gtk::Box::builder().spacing(12).build();
            row.append(&name);
            row.append(&value);
            list.append(&gtk::ListBoxRow::builder().child(&row).build());
            values.push(value);
        }

        let title = gtk::Label::builder().label("Reading").xalign(0.0).css_classes(["title"]).build();
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
        root.append(&title);
        root.append(&list);
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
        if let Some(row) = self.list.row_at_index(i as i32) {
            self.list.select_row(Some(&row));
        }
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
        for (field, label) in FIELDS.iter().zip(&self.values) {
            let text = match field {
                Field::Scope => match self.scope.get() {
                    Scope::AllBooks => "all books".into(),
                    Scope::ThisBook => "this book".into(),
                },
                Field::Theme => themes::name_of(p.theme.as_deref()),
                Field::Font => p.font_family.clone().unwrap_or_else(|| "publisher".into()),
                Field::Size => format!("{} px", p.font_size),
                Field::LineHeight => format!("{:.1}", p.line_height),
                Field::Justify => on_off(p.justify),
                Field::Hyphenate => on_off(p.hyphenate),
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
                Field::Publisher => match p.publisher {
                    Publisher::Keep => "keep".into(),
                    Publisher::Fonts => "override fonts".into(),
                    Publisher::All => "override all".into(),
                },
                Field::Reset => match self.scope.get() {
                    Scope::ThisBook => "enter".into(),
                    Scope::AllBooks => "—".into(),
                },
            };
            label.set_label(&format!("‹ {text} ›"));
        }
    }
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
        Field::Margin => "Margins",
        Field::Width => "Max width",
        Field::Columns => "Columns",
        Field::Flow => "Layout",
        Field::Publisher => "Publisher styles",
        Field::Reset => "Use the defaults for this book",
    }
}

fn on_off(b: bool) -> String {
    if b { "on".into() } else { "off".into() }
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
}

//! The library: a rail of shelves and a grid of covers.
//!
//! The grid is a virtualized `GridView`, so only visible covers are built and
//! decoded. Books live in one `ListStore`; the shelf, the filter and the sort
//! are a filter and a sorter over it.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::glib::BoxedAnyObject;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use omaread_core::library::Book;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shelf {
    Reading,
    All,
    Folder(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Recent,
    Title,
    Author,
    Progress,
    Added,
}

impl Sort {
    fn next(self) -> Sort {
        match self {
            Sort::Recent => Sort::Title,
            Sort::Title => Sort::Author,
            Sort::Author => Sort::Progress,
            Sort::Progress => Sort::Added,
            Sort::Added => Sort::Recent,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Sort::Recent => "recent",
            Sort::Title => "title",
            Sort::Author => "author",
            Sort::Progress => "progress",
            Sort::Added => "added",
        }
    }
}

struct State {
    shelf: Shelf,
    query: String,
    sort: Sort,
    /// Rail entries in order, so Tab can step through them.
    shelves: Vec<Shelf>,
}

pub struct LibraryView {
    pub root: gtk::Box,
    store: gio::ListStore,
    filter: gtk::CustomFilter,
    sorter: gtk::CustomSorter,
    selection: gtk::SingleSelection,
    grid: gtk::GridView,
    rail: gtk::ListBox,
    title: gtk::Label,
    hint: gtk::Label,
    entry: gtk::Entry,
    empty: gtk::Label,
    state: Rc<RefCell<State>>,
    /// Set while the rail is being rebuilt, so selecting rows doesn't refilter.
    rebuilding: Cell<bool>,
    /// A scan is running, so an empty library may just not be read yet.
    scanning: Cell<bool>,
}

thread_local! {
    /// Decoded covers, so scrolling back doesn't decode them again.
    static COVERS: RefCell<HashMap<PathBuf, gdk::Texture>> = RefCell::default();
}

fn cover_texture(path: &Path) -> Option<gdk::Texture> {
    COVERS.with(|c| {
        if let Some(t) = c.borrow().get(path) {
            return Some(t.clone());
        }
        let texture = gdk::Texture::from_filename(path).ok()?;
        c.borrow_mut().insert(path.to_path_buf(), texture.clone());
        Some(texture)
    })
}

fn book_of(obj: &glib::Object) -> std::cell::Ref<'_, Book> {
    obj.downcast_ref::<BoxedAnyObject>().expect("library items are books").borrow::<Book>()
}

impl LibraryView {
    pub fn new(on_open: impl Fn(Book) + 'static) -> Rc<LibraryView> {
        let state = Rc::new(RefCell::new(State {
            shelf: Shelf::All,
            query: String::new(),
            sort: Sort::Recent,
            shelves: Vec::new(),
        }));

        let store = gio::ListStore::new::<BoxedAnyObject>();
        let filter = gtk::CustomFilter::new({
            let state = state.clone();
            move |obj| matches(&book_of(obj), &state.borrow())
        });
        let sorter = gtk::CustomSorter::new({
            let state = state.clone();
            move |a, b| compare(&book_of(a), &book_of(b), state.borrow().sort).into()
        });
        let filtered = gtk::FilterListModel::new(Some(store.clone()), Some(filter.clone()));
        let sorted = gtk::SortListModel::new(Some(filtered), Some(sorter.clone()));
        let selection = gtk::SingleSelection::builder().model(&sorted).autoselect(true).build();

        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
            item.set_child(Some(&book_cell()));
        });
        factory.connect_bind(|_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
            if let (Some(obj), Some(cell)) = (item.item(), item.child()) {
                bind_cell(&cell, &book_of(&obj));
            }
        });

        let grid = gtk::GridView::builder()
            .model(&selection)
            .factory(&factory)
            .min_columns(1)
            .max_columns(16)
            .css_classes(["library-grid"])
            .build();
        let on_open = Rc::new(on_open);
        grid.connect_activate({
            let selection = selection.clone();
            let on_open = on_open.clone();
            move |_, position| {
                if let Some(obj) = selection.item(position) {
                    on_open(book_of(&obj).clone());
                }
            }
        });

        let scroller = gtk::ScrolledWindow::builder()
            .child(&grid)
            .hexpand(true)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();

        let title = gtk::Label::builder().xalign(0.0).hexpand(true).css_classes(["library-title"]).build();
        let hint = gtk::Label::builder().xalign(1.0).css_classes(["library-hint"]).build();
        let header = gtk::Box::builder().spacing(12).css_classes(["library-header"]).build();
        header.append(&title);
        header.append(&hint);

        let entry = gtk::Entry::builder()
            .placeholder_text("Filter by title or author")
            .visible(false)
            .css_classes(["library-filter"])
            .build();

        let empty = gtk::Label::builder()
            .visible(false)
            .vexpand(true)
            .wrap(true)
            .justify(gtk::Justification::Center)
            .css_classes(["library-empty"])
            .build();

        let main = gtk::Box::builder().orientation(gtk::Orientation::Vertical).hexpand(true).build();
        main.append(&header);
        main.append(&entry);
        main.append(&empty);
        main.append(&scroller);

        let rail = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(["library-rail"])
            .build();
        let rail_scroller = gtk::ScrolledWindow::builder()
            .child(&rail)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .css_classes(["library-rail-scroller"])
            .build();

        let root = gtk::Box::builder().css_classes(["library"]).build();
        root.append(&rail_scroller);
        root.append(&main);

        let view = Rc::new(LibraryView {
            root,
            store,
            filter,
            sorter,
            selection,
            grid,
            rail,
            title,
            hint,
            entry,
            empty,
            state,
            rebuilding: Cell::new(false),
            scanning: Cell::new(false),
        });

        view.rail.connect_row_selected({
            let view = Rc::downgrade(&view);
            move |_, row| {
                let (Some(view), Some(row)) = (view.upgrade(), row) else { return };
                if view.rebuilding.get() {
                    return;
                }
                let shelf = view.state.borrow().shelves.get(row.index() as usize).cloned();
                if let Some(shelf) = shelf {
                    view.set_shelf(shelf);
                }
            }
        });
        view.entry.connect_changed({
            let view = Rc::downgrade(&view);
            move |entry| {
                if let Some(view) = view.upgrade() {
                    view.state.borrow_mut().query = entry.text().to_lowercase();
                    view.refilter();
                }
            }
        });
        view
    }

    /// Replace the books, keeping the shelf, filter and sort.
    pub fn set_books(&self, mut books: Vec<Book>) {
        let selected = self.selected().map(|b| b.path);
        let first_load = self.store.n_items() == 0;
        books.sort_by(|a, b| a.path.cmp(&b.path));
        let items: Vec<BoxedAnyObject> = books.iter().cloned().map(BoxedAnyObject::new).collect();
        self.store.splice(0, self.store.n_items(), &items);

        if first_load && books.iter().any(Book::in_progress) {
            self.state.borrow_mut().shelf = Shelf::Reading;
        }
        self.rebuild_rail(&books);
        self.refilter();
        if let Some(path) = selected {
            self.select_path(&path);
        }
    }

    /// Update one book in place (after reading it), without rebuilding the grid.
    pub fn update_book(&self, book: Book) {
        for i in 0..self.store.n_items() {
            let Some(obj) = self.store.item(i) else { continue };
            if book_of(&obj).path == book.path {
                self.store.splice(i, 1, &[BoxedAnyObject::new(book)]);
                break;
            }
        }
        // Counts (Reading, above all) may have moved.
        let books: Vec<Book> = (0..self.store.n_items()).filter_map(|i| self.store.item(i)).map(|o| book_of(&o).clone()).collect();
        self.rebuild_rail(&books);
        self.refilter();
        self.sorter.changed(gtk::SorterChange::Different);
    }

    pub fn set_scanning(&self, scanning: bool) {
        self.scanning.set(scanning);
        self.update_header();
    }

    pub fn selected(&self) -> Option<Book> {
        self.selection.selected_item().map(|obj| book_of(&obj).clone())
    }

    pub fn open_selected(&self) {
        let position = self.selection.selected();
        if position != gtk::INVALID_LIST_POSITION {
            self.grid.emit_by_name::<()>("activate", &[&position]);
        }
    }

    pub fn focus(&self) {
        self.grid.grab_focus();
        self.scroll_to(self.selection.selected());
    }

    /// The entry's text field is a child of the entry, so check by ancestry.
    pub fn filter_has_focus(&self) -> bool {
        self.entry.root().and_then(|r| r.focus()).is_some_and(|f| f == self.entry.clone().upcast::<gtk::Widget>() || f.is_ancestor(&self.entry))
    }

    pub fn start_filter(&self) {
        self.entry.set_visible(true);
        self.entry.grab_focus();
    }

    /// Esc in the filter: clear it and give the keys back to the grid.
    pub fn cancel_filter(&self) {
        self.entry.set_text("");
        self.entry.set_visible(false);
        self.focus();
    }

    /// Enter in the filter: keep it, go back to the grid.
    pub fn accept_filter(&self) {
        if self.entry.text().is_empty() {
            self.entry.set_visible(false);
        }
        self.focus();
    }

    pub fn cycle_sort(&self) {
        let next = self.state.borrow().sort.next();
        self.state.borrow_mut().sort = next;
        self.sorter.changed(gtk::SorterChange::Different);
        self.scroll_to(0);
        self.update_header();
    }

    pub fn step_shelf(&self, delta: i32) {
        let (shelves, current) = {
            let s = self.state.borrow();
            (s.shelves.clone(), s.shelf.clone())
        };
        if shelves.is_empty() {
            return;
        }
        let i = shelves.iter().position(|s| *s == current).unwrap_or(0) as i32;
        let n = shelves.len() as i32;
        let next = (i + delta).rem_euclid(n);
        if let Some(row) = self.rail.row_at_index(next) {
            self.rail.select_row(Some(&row));
        }
    }

    /// Move the cursor by `dx` cells and `dy` rows.
    pub fn move_cursor(&self, dx: i32, dy: i32) {
        let n = self.selection.n_items() as i32;
        if n == 0 {
            return;
        }
        let current = match self.selection.selected() {
            gtk::INVALID_LIST_POSITION => 0,
            p => p as i32,
        };
        let next = (current + dx + dy * self.columns()).clamp(0, n - 1);
        self.scroll_to(next as u32);
    }

    pub fn cursor_to_start(&self) {
        self.scroll_to(0);
    }

    pub fn cursor_to_end(&self) {
        let n = self.selection.n_items();
        if n > 0 {
            self.scroll_to(n - 1);
        }
    }

    fn scroll_to(&self, position: u32) {
        if position >= self.selection.n_items() {
            return;
        }
        self.grid.scroll_to(position, gtk::ListScrollFlags::FOCUS | gtk::ListScrollFlags::SELECT, None);
    }

    /// How many covers fit in a row, from the size of a laid-out cell.
    fn columns(&self) -> i32 {
        let cell = self.grid.first_child().map(|c| c.width()).filter(|w| *w > 0);
        match cell {
            Some(w) => (self.grid.width() / w).max(1),
            None => 1,
        }
    }

    fn select_path(&self, path: &Path) {
        for i in 0..self.selection.n_items() {
            if let Some(obj) = self.selection.item(i)
                && book_of(&obj).path == path
            {
                self.selection.set_selected(i);
                return;
            }
        }
    }

    fn set_shelf(&self, shelf: Shelf) {
        self.state.borrow_mut().shelf = shelf;
        self.refilter();
        self.scroll_to(0);
    }

    fn refilter(&self) {
        self.filter.changed(gtk::FilterChange::Different);
        self.update_header();
    }

    fn update_header(&self) {
        let s = self.state.borrow();
        let name = match &s.shelf {
            _ if !s.query.is_empty() => "Search".to_string(),
            Shelf::Reading => "Reading".to_string(),
            Shelf::All => "All books".to_string(),
            Shelf::Folder(f) => f.clone(),
        };
        let n = self.selection.n_items();
        let count = if n == 1 { "1 book".to_string() } else { format!("{n} books") };
        self.title.set_label(&format!("{name}  ·  {count}"));
        self.hint.set_label(&format!("sorted by {}   / filter   o sort   tab shelves", s.sort.label()));

        let total = self.store.n_items();
        let message = if total == 0 && self.scanning.get() {
            Some("Reading your library…")
        } else if total == 0 {
            Some("No books yet.\nAdd EPUBs to ~/Documents/Books, or set [library] folders\nin ~/.config/omaread/config.toml.")
        } else if n == 0 {
            Some("Nothing matches.")
        } else {
            None
        };
        self.empty.set_visible(message.is_some());
        self.empty.set_label(message.unwrap_or(""));
    }

    fn rebuild_rail(&self, books: &[Book]) {
        self.rebuilding.set(true);
        while let Some(row) = self.rail.row_at_index(0) {
            self.rail.remove(&row);
        }

        // Every folder, with its parents, so "Business" lists all of its sub-shelves.
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for b in books.iter().filter(|b| !b.shelf.is_empty()) {
            let parts: Vec<&str> = b.shelf.split(" / ").collect();
            for depth in 1..=parts.len() {
                *counts.entry(parts[..depth].join(" / ")).or_default() += 1;
            }
        }
        let reading = books.iter().filter(|b| b.in_progress()).count();

        let mut shelves = vec![Shelf::Reading, Shelf::All];
        self.rail.append(&rail_row("Reading", reading, 0));
        self.rail.append(&rail_row("All books", books.len(), 0));
        for (folder, n) in &counts {
            let depth = folder.matches(" / ").count();
            let name = folder.rsplit(" / ").next().unwrap_or(folder);
            self.rail.append(&rail_row(name, *n, depth + 1));
            shelves.push(Shelf::Folder(folder.clone()));
        }

        let current = self.state.borrow().shelf.clone();
        let current = if shelves.contains(&current) { current } else { Shelf::All };
        let index = shelves.iter().position(|s| *s == current).unwrap_or(1);
        {
            let mut s = self.state.borrow_mut();
            s.shelves = shelves;
            s.shelf = current;
        }
        if let Some(row) = self.rail.row_at_index(index as i32) {
            self.rail.select_row(Some(&row));
        }
        self.rebuilding.set(false);
    }
}

fn matches(book: &Book, s: &State) -> bool {
    // A filter searches the whole library, whatever shelf is open.
    let on_shelf = !s.query.is_empty() || match &s.shelf {
        Shelf::Reading => book.in_progress(),
        Shelf::All => true,
        Shelf::Folder(f) => book.shelf == *f || book.shelf.strip_prefix(f.as_str()).is_some_and(|rest| rest.starts_with(" / ")),
    };
    if !on_shelf {
        return false;
    }
    if s.query.is_empty() {
        return true;
    }
    s.query.split_whitespace().all(|word| {
        book.title.to_lowercase().contains(word) || book.authors.iter().any(|a| a.to_lowercase().contains(word))
    })
}

fn compare(a: &Book, b: &Book, sort: Sort) -> std::cmp::Ordering {
    let title = || natural_key(&a.title).cmp(&natural_key(&b.title));
    match sort {
        Sort::Recent => b.opened_at().cmp(&a.opened_at()).then(b.added_at.cmp(&a.added_at)).then_with(title),
        Sort::Title => title(),
        Sort::Author => a.authors.first().map(|s| s.to_lowercase()).cmp(&b.authors.first().map(|s| s.to_lowercase())).then_with(title),
        Sort::Progress => b.fraction().total_cmp(&a.fraction()).then_with(title),
        Sort::Added => b.added_at.cmp(&a.added_at).then_with(title),
    }
}

/// "The Go Programming Language" sorts under G.
fn natural_key(title: &str) -> String {
    let lower = title.to_lowercase();
    for article in ["the ", "a ", "an ", "el ", "la ", "los ", "las "] {
        if let Some(rest) = lower.strip_prefix(article) {
            return rest.to_string();
        }
    }
    lower
}

fn rail_row(name: &str, count: usize, depth: usize) -> gtk::ListBoxRow {
    let row = gtk::Box::builder().spacing(8).build();
    let label = gtk::Label::builder()
        .label(name)
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .margin_start((depth.saturating_sub(1) * 12) as i32)
        .build();
    let count = gtk::Label::builder().label(count.to_string()).css_classes(["count"]).build();
    row.append(&label);
    row.append(&count);
    let row = gtk::ListBoxRow::builder().child(&row).css_classes(["shelf-row"]).build();
    if depth == 0 {
        row.add_css_class("special");
    }
    row
}

/// One grid cell. `bind_cell` relies on this child order.
fn book_cell() -> gtk::Box {
    let cell = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .css_classes(["book"])
        .build();

    let cover = gtk::Overlay::builder().css_classes(["cover"]).build();
    let picture = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Cover)
        .can_shrink(true)
        .width_request(132)
        .height_request(198)
        .build();
    let placeholder = gtk::Label::builder()
        .wrap(true)
        .justify(gtk::Justification::Center)
        .valign(gtk::Align::Center)
        .halign(gtk::Align::Center)
        .lines(6)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["cover-placeholder"])
        .build();
    cover.set_child(Some(&picture));
    cover.add_overlay(&placeholder);
    let progress = gtk::ProgressBar::builder().valign(gtk::Align::End).css_classes(["book-progress"]).build();
    cover.add_overlay(&progress);

    let title = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .lines(2)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(1)
        .css_classes(["book-title"])
        .build();
    let author = gtk::Label::builder()
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(1)
        .css_classes(["book-author"])
        .build();

    cell.append(&cover);
    cell.append(&title);
    cell.append(&author);
    cell
}

fn bind_cell(cell: &gtk::Widget, book: &Book) {
    let Some(cover) = cell.first_child().and_downcast::<gtk::Overlay>() else { return };
    let Some(title) = cover.next_sibling().and_downcast::<gtk::Label>() else { return };
    let Some(author) = title.next_sibling().and_downcast::<gtk::Label>() else { return };
    let Some(picture) = cover.child().and_downcast::<gtk::Picture>() else { return };
    let mut overlays = picture.next_sibling();
    let placeholder = overlays.as_ref().and_then(|w| w.clone().downcast::<gtk::Label>().ok());
    overlays = overlays.and_then(|w| w.next_sibling());
    let progress = overlays.and_then(|w| w.downcast::<gtk::ProgressBar>().ok());

    let texture = book.cover.as_deref().and_then(cover_texture);
    picture.set_paintable(texture.as_ref());
    if let Some(placeholder) = placeholder {
        placeholder.set_visible(texture.is_none());
        placeholder.set_label(&book.title);
    }
    if let Some(progress) = progress {
        progress.set_visible(book.progress.is_some());
        progress.set_fraction(book.fraction());
    }

    title.set_label(&book.title);
    let byline = match &book.error {
        Some(e) => e.clone(),
        None => book.authors.join(", "),
    };
    author.set_label(&byline);
    if book.error.is_some() {
        cell.add_css_class("unreadable");
    } else {
        cell.remove_css_class("unreadable");
    }
    cell.set_tooltip_text(Some(&book.title));
}

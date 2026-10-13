//! The window: the library, or a book filling it with the chrome floating over.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::{gio, glib};
use omaread_core::config::Config;
use omaread_core::db::{Db, Progress};
use omaread_core::command::{self, Command};
use omaread_core::library::{self, Book, Paths};
use omaread_core::speed::{ReadingSpeed, format_minutes};
use omaread_core::theme::Theme;

use crate::keys::{Action, Context, Keymap};
use crate::library_view::LibraryView;
use crate::reader::{Event, Location, Reader};
use crate::service::Service;
use crate::toc_view::{TocItem, TocView};
use crate::theming::Theming;

const LIBRARY: &str = "library";
const READER: &str = "reader";

/// How long reading position changes settle before they're written.
const SAVE_DELAY: Duration = Duration::from_millis(800);

const SPEED_SETTING: &str = "reading_speed";
/// foliate-js counts a "location" as this many bytes of text.
const BYTES_PER_LOCATION: f64 = 1500.0;
/// The fixed pace foliate-js's time estimates assume, which we rescale.
const FOLIATE_BYTES_PER_MINUTE: f64 = 1600.0;

pub struct Window {
    pub window: adw::ApplicationWindow,
    stack: gtk::Stack,
    library: Rc<LibraryView>,
    reader: Reader,
    status: Status,
    toc: Rc<TocView>,
    toc_items: RefCell<Vec<TocItem>>,
    cmdline: gtk::Entry,
    toast: gtk::Label,
    toast_timer: RefCell<Option<glib::SourceId>>,
    theming: Rc<RefCell<Theming>>,
    keymap: RefCell<Keymap>,
    db: Option<Db>,
    service: RefCell<Option<Rc<Service>>>,
    /// The open book and where we are in it.
    current: RefCell<Option<Current>>,
    save_timer: RefCell<Option<glib::SourceId>>,
    /// The window's own handle, for timers that must not keep it alive.
    me: RefCell<std::rc::Weak<Window>>,
    speed: Cell<ReadingSpeed>,
    /// Where the last page started, and when, to learn reading speed.
    page: RefCell<Option<PageSeen>>,
    /// The next relocation follows a page turn (not a jump), so it can teach the pace.
    turned: Cell<bool>,
    last_location: RefCell<Option<Location>>,
}

struct PageSeen {
    at: Instant,
    fraction: f64,
}

struct Current {
    book: Book,
    progress: Progress,
    dirty: bool,
}

struct Status {
    bar: gtk::Box,
    section: gtk::Label,
    position: gtk::Label,
}

impl Window {
    pub fn new(app: &adw::Application) -> Rc<Window> {
        let theming = Rc::new(RefCell::new(Theming::load()));
        let config = Config::load();
        let paths = Paths::from_env();
        let db = match paths.open_db() {
            Ok(db) => Some(db),
            Err(e) => {
                eprintln!("omaread: no library database ({e}); reading positions won't be saved");
                None
            }
        };

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Omaread")
            .default_width(1100)
            .default_height(820)
            .build();
        window.add_css_class("omaread");

        // The page and the library call back into the window, which doesn't
        // exist yet; they get a weak handle that's filled in below.
        let this: Rc<RefCell<Option<Rc<Window>>>> = Rc::default();
        let with_window = {
            let this = Rc::downgrade(&this);
            move || this.upgrade().and_then(|cell| cell.borrow().clone())
        };

        let reader = Reader::new({
            let with_window = with_window.clone();
            move |event| {
                if let Some(win) = with_window() {
                    win.on_event(event);
                }
            }
        });
        let library = LibraryView::new({
            let with_window = with_window.clone();
            move |book| {
                if let Some(win) = with_window() {
                    win.open_book(book);
                }
            }
        });

        let status = status_line();
        let toc = TocView::new({
            let with_window = with_window.clone();
            move |href| {
                if let Some(win) = with_window() {
                    win.toc.close();
                    win.reader.call("goTo", &serde_json::to_string(&href).unwrap_or_default());
                    win.reader.view.grab_focus();
                }
            }
        });
        let cmdline = gtk::Entry::builder()
            .valign(gtk::Align::End)
            .visible(false)
            .css_classes(["cmdline"])
            .build();
        let reader_overlay = gtk::Overlay::new();
        reader_overlay.set_child(Some(&reader.view));
        reader_overlay.add_overlay(&status.bar);
        reader_overlay.add_overlay(&toc.root);
        reader_overlay.add_overlay(&cmdline);

        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(120)
            .build();
        stack.add_named(&library.root, Some(LIBRARY));
        stack.add_named(&reader_overlay, Some(READER));
        stack.set_visible_child_name(LIBRARY);

        let toast = gtk::Label::builder()
            .halign(gtk::Align::Center)
            .valign(gtk::Align::End)
            .visible(false)
            .wrap(true)
            .css_classes(["toast"])
            .build();
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&stack));
        overlay.add_overlay(&toast);
        window.set_content(Some(&overlay));

        // What we knew last time, at once; a scan follows in the background.
        if let Some(books) = db.as_ref().and_then(|db| library::load(db).ok()) {
            library.set_books(books);
        }

        let win = Rc::new(Window {
            window,
            stack,
            library,
            reader,
            status,
            toc,
            toc_items: RefCell::default(),
            cmdline,
            toast,
            toast_timer: RefCell::default(),
            theming,
            keymap: RefCell::default(),
            db,
            service: RefCell::default(),
            current: RefCell::default(),
            save_timer: RefCell::default(),
            me: RefCell::default(),
            speed: Cell::new(ReadingSpeed::default()),
            page: RefCell::default(),
            turned: Cell::new(false),
            last_location: RefCell::default(),
        });
        if let Some(speed) = win.db.as_ref().and_then(|db| db.setting(SPEED_SETTING).ok().flatten()).and_then(|s| ReadingSpeed::from_setting(&s)) {
            win.speed.set(speed);
        }
        win.cmdline.connect_activate({
            let win = Rc::downgrade(&win);
            move |entry| {
                if let Some(win) = win.upgrade() {
                    let text = entry.text().to_string();
                    win.close_cmdline();
                    win.run_command(&text);
                }
            }
        });
        win.me.replace(Rc::downgrade(&win));
        *this.borrow_mut() = Some(win.clone());

        let service = Service::start(paths, config.library_folders(), {
            let win = Rc::downgrade(&win);
            move |books| {
                if let Some(win) = win.upgrade() {
                    win.library.set_scanning(false);
                    win.library.set_books(books);
                }
            }
        });
        win.library.set_scanning(true);
        service.scan();
        win.service.replace(Some(service));

        win.window.connect_close_request({
            let win = Rc::downgrade(&win);
            move |_| {
                if let Some(win) = win.upgrade() {
                    win.save_progress();
                }
                glib::Propagation::Proceed
            }
        });
        // `this` holds the window for the callbacks; let it go with the window.
        win.window.connect_destroy(move |_| {
            this.borrow_mut().take();
        });

        win.install_keys();
        Theming::follow(&win.theming, {
            let win = Rc::downgrade(&win);
            move |theme| {
                if let Some(win) = win.upgrade() {
                    win.push_palette(theme);
                }
            }
        });
        win.library.focus();
        win
    }

    /// Open a file given on the command line or by another app.
    pub fn open(&self, file: &gio::File) {
        let Some(path) = file.path() else {
            self.show_toast("Only local files can be opened", true);
            return;
        };
        self.open_book(self.book_for(&path));
    }

    /// The library's record for a file, or one made up from the file itself.
    fn book_for(&self, path: &Path) -> Book {
        if let Some(book) = self.db.as_ref().and_then(|db| library::load(db).ok()).and_then(|books| books.into_iter().find(|b| b.path == path)) {
            return book;
        }
        let (identity, meta) = library::identify(path);
        let meta = meta.unwrap_or_default();
        Book {
            path: path.to_path_buf(),
            progress: self.db.as_ref().and_then(|db| db.progress(&identity).ok().flatten()),
            identity,
            title: if meta.title.is_empty() { file_stem(path) } else { meta.title },
            authors: meta.authors,
            language: meta.language,
            series: meta.series,
            series_index: meta.series_index,
            shelf: String::new(),
            cover: None,
            error: None,
            added_at: library::now(),
        }
    }

    fn open_book(&self, book: Book) {
        if let Some(error) = &book.error {
            self.show_toast(&format!("Can't open “{}”: {error}", book.title), true);
            return;
        }
        self.save_progress();
        // Progress may have moved since the library row was loaded.
        let saved = self.db.as_ref().and_then(|db| db.progress(&book.identity).ok().flatten()).or(book.progress.clone());
        let cfi = saved.as_ref().and_then(|p| p.cfi.clone());

        self.window.set_title(Some(&book.title));
        self.toc.close();
        self.toc.set_items(&[]);
        self.toc_items.borrow_mut().clear();
        self.page.take();
        self.last_location.take();
        self.status.section.set_label("");
        self.status.position.set_label("");
        self.reader.open(book.path.clone(), cfi.as_deref());

        let progress = Progress { opened_at: library::now(), ..saved.unwrap_or_default() };
        self.current.replace(Some(Current { book, progress, dirty: true }));
        self.save_progress();

        self.stack.set_visible_child_name(READER);
        self.reader.view.grab_focus();
    }

    /// Back to the library, keeping where we were.
    fn close_book(&self) {
        self.save_progress();
        self.save_speed();
        self.toc.close();
        self.close_cmdline();
        if let Some(current) = self.current.take() {
            let mut book = current.book;
            book.progress = Some(current.progress);
            self.library.update_book(book);
        }
        self.reader.close();
        self.window.set_title(Some("Omaread"));
        self.stack.set_visible_child_name(LIBRARY);
        self.library.focus();
    }

    fn context(&self) -> Context {
        if self.stack.visible_child_name().as_deref() != Some(READER) {
            Context::Library
        } else if self.toc.is_open() {
            Context::Toc
        } else {
            Context::Reader
        }
    }

    fn push_palette(&self, theme: &Theme) {
        let palette = serde_json::to_string(&theme.book_palette()).unwrap_or_default();
        self.reader.call("setPalette", &palette);
    }

    fn on_event(&self, event: Event) {
        match event {
            Event::Ready => self.push_palette(self.theming.borrow().theme()),
            Event::BookReady { toc, .. } => {
                self.toc.set_items(&toc);
                self.toc_items.replace(toc);
                self.reader.view.grab_focus();
            }
            Event::Notice { message } => self.show_toast(&message, false),
            Event::BookError { message, stack } => {
                eprintln!("omaread: could not open book: {message}\n{stack}");
                self.show_toast(&format!("Could not open this book: {message}"), true);
                self.current.take();
                self.stack.set_visible_child_name(LIBRARY);
                self.library.focus();
            }
            Event::Relocate(location) => {
                self.learn_pace(&location);
                self.update_status(&location);
                self.record(&location);
                self.last_location.replace(Some(location));
            }
            Event::ExternalLink { href } => self.open_link(&href),
            Event::Selection { .. } => {}
            Event::PageCrashed { gave_up: false } => self.show_toast("The page crashed and was reloaded", true),
            Event::PageCrashed { gave_up: true } => {
                self.show_toast("This book keeps crashing the page, so it was closed", true);
                self.close_book();
            }
        }
    }

    /// Links out of a book go to the browser, and only web and mail links:
    /// a book must not be able to launch files or other handlers.
    fn open_link(&self, href: &str) {
        let scheme = href.split_once(':').map(|(s, _)| s.to_ascii_lowercase()).unwrap_or_default();
        if !matches!(scheme.as_str(), "http" | "https" | "mailto") {
            self.show_toast(&format!("Not opening a {scheme}: link from a book"), true);
            return;
        }
        gtk::UriLauncher::new(href).launch(Some(&self.window), None::<&gio::Cancellable>, |_| {});
    }

    fn record(&self, location: &Location) {
        {
            let mut current = self.current.borrow_mut();
            let Some(current) = current.as_mut() else { return };
            current.progress.cfi = location.cfi.clone().or(current.progress.cfi.take());
            current.progress.fraction = location.fraction.clamp(0.0, 1.0);
            current.dirty = true;
        }
        if let Some(timer) = self.save_timer.take() {
            timer.remove();
        }
        // Debounced: paging quickly writes once, when it stops.
        let win = self.me.borrow().clone();
        let timer = glib::timeout_add_local_once(SAVE_DELAY, move || {
            if let Some(win) = win.upgrade() {
                win.save_progress_from_timer();
            }
        });
        self.save_timer.replace(Some(timer));
    }

    fn save_progress_from_timer(&self) {
        self.save_timer.take();
        self.save_progress();
    }

    fn save_progress(&self) {
        if let Some(timer) = self.save_timer.take() {
            timer.remove();
        }
        let mut current = self.current.borrow_mut();
        let Some(current) = current.as_mut().filter(|c| c.dirty) else { return };
        if let Some(db) = &self.db {
            match db.save_progress(&current.book.identity, &current.progress) {
                Ok(()) => current.dirty = false,
                Err(e) => eprintln!("omaread: could not save reading position: {e}"),
            }
        }
    }

    /// A page turned after being read teaches the reading pace.
    fn learn_pace(&self, l: &Location) {
        let now = Instant::now();
        let turned = self.turned.replace(false);
        let previous = self.page.replace(Some(PageSeen { at: now, fraction: l.fraction }));
        let (Some(previous), Some(loc)) = (previous, l.location) else { return };
        if !turned || l.fraction <= previous.fraction {
            return;
        }
        let book_bytes = loc.total as f64 * BYTES_PER_LOCATION;
        let bytes = (l.fraction - previous.fraction) * book_bytes;
        let mut speed = self.speed.get();
        if speed.observe(bytes, now.duration_since(previous.at).as_secs_f64()) {
            self.speed.set(speed);
            if speed.samples % 10 == 0 {
                self.save_speed();
            }
        }
    }

    fn save_speed(&self) {
        if let Some(db) = &self.db
            && let Err(e) = db.set_setting(SPEED_SETTING, &self.speed.get().to_setting())
        {
            eprintln!("omaread: could not save reading speed: {e}");
        }
    }

    fn update_status(&self, l: &Location) {
        self.status.section.set_label(l.toc_label.as_deref().unwrap_or(""));
        let percent = (l.fraction * 100.0).round() as u32;
        let mut parts = Vec::new();
        if let Some(time) = l.time {
            // foliate-js's minutes at its pace, back to bytes, then at yours.
            let speed = self.speed.get();
            let section = speed.minutes_for(time.section * FOLIATE_BYTES_PER_MINUTE);
            let total = speed.minutes_for(time.total * FOLIATE_BYTES_PER_MINUTE);
            parts.push(format!("{} left in chapter", format_minutes(section)));
            parts.push(format!("{} left", format_minutes(total)));
        }
        match (&l.page_label, l.location) {
            (Some(page), _) => parts.push(format!("p. {page}")),
            (None, Some(loc)) if loc.total > 0 => parts.push(format!("{} / {}", loc.current + 1, loc.total)),
            _ => {}
        }
        parts.push(format!("{percent}%"));
        self.status.position.set_label(&parts.join("  ·  "));
    }

    fn open_toc(&self) {
        if self.toc.is_empty() {
            self.show_toast("This book has no table of contents", false);
            return;
        }
        let href = self.last_location.borrow().as_ref().and_then(|l| l.toc_href.clone());
        self.toc.open(href.as_deref());
    }

    fn open_cmdline(&self) {
        self.cmdline.set_text(":");
        self.cmdline.set_visible(true);
        self.cmdline.grab_focus();
        self.cmdline.set_position(-1);
    }

    fn close_cmdline(&self) {
        if WidgetExt::is_visible(&self.cmdline) {
            self.cmdline.set_visible(false);
            self.reader.view.grab_focus();
        }
    }

    fn cmdline_has_focus(&self) -> bool {
        WidgetExt::is_visible(&self.cmdline)
            && gtk::prelude::GtkWindowExt::focus(&self.window)
                .is_some_and(|f| f == self.cmdline.clone().upcast::<gtk::Widget>() || f.is_ancestor(&self.cmdline))
    }

    fn run_command(&self, text: &str) {
        let text = text.trim();
        if text.trim_start_matches(':').trim().is_empty() {
            return;
        }
        match command::parse(text) {
            Ok(Command::GotoFraction(f)) => self.reader.call("goToFraction", &f.to_string()),
            Ok(Command::GotoPage(label)) => self.reader.call("goToPage", &serde_json::to_string(&label).unwrap_or_default()),
            Ok(Command::GotoChapter(n)) => {
                let href = crate::toc_view::chapter_href(&self.toc_items.borrow(), n);
                match href {
                    Some(href) => self.reader.call("goTo", &serde_json::to_string(&href).unwrap_or_default()),
                    None => self.show_toast(&format!("There is no chapter {n}"), true),
                }
            }
            Err(e) => self.show_toast(&e, true),
        }
    }

    fn install_keys(self: &Rc<Self>) {
        let keys = gtk::EventControllerKey::new();
        // Capture: the window hears keys before the page or the grid do.
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let win = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, mods| match win.upgrade() {
            Some(win) => win.handle_key(key, mods),
            None => glib::Propagation::Proceed,
        });
        self.window.add_controller(keys);
    }

    pub fn handle_key(&self, key: gtk::gdk::Key, mods: gtk::gdk::ModifierType) -> glib::Propagation {
        let context = self.context();

        // Typing a command: Esc cancels, Enter runs (the entry's own activate).
        if self.cmdline_has_focus() {
            if key == gtk::gdk::Key::Escape {
                self.close_cmdline();
                return glib::Propagation::Stop;
            }
            return glib::Propagation::Proceed;
        }

        // Typing in the filter: only Esc and Enter are ours.
        if context == Context::Library && self.library.filter_has_focus() {
            return match key {
                gtk::gdk::Key::Escape => {
                    self.library.cancel_filter();
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter | gtk::gdk::Key::Down | gtk::gdk::Key::Tab => {
                    self.library.accept_filter();
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            };
        }

        let action = self.keymap.borrow_mut().press(context, key, mods);
        match action {
            Some(action) => {
                self.run(context, action);
                glib::Propagation::Stop
            }
            // While reading, the page never acts on keys itself.
            None if context == Context::Reader => glib::Propagation::Stop,
            None => glib::Propagation::Proceed,
        }
    }

    fn run(&self, context: Context, action: Action) {
        let call = |method: &str| self.reader.call(method, "");
        match (context, action) {
            (Context::Reader, Action::NextPage) => {
                self.turned.set(true);
                call("next")
            }
            (Context::Reader, Action::Toc) => self.open_toc(),
            (Context::Reader, Action::CommandLine) => self.open_cmdline(),
            (Context::Toc, Action::Move(0, dy)) => self.toc.move_cursor(dy),
            (Context::Toc, Action::Move(dx, _)) => self.toc.fold(dx > 0),
            (Context::Toc, Action::First) => self.toc.move_cursor(i32::MIN / 2),
            (Context::Toc, Action::Last) => self.toc.move_cursor(i32::MAX / 2),
            (Context::Toc, Action::Open) => self.toc.activate(),
            (Context::Toc, Action::Close) => {
                self.toc.close();
                self.reader.view.grab_focus();
            }
            (Context::Reader, Action::PrevPage) => call("prev"),
            (Context::Reader, Action::NextSection) => call("nextSection"),
            (Context::Reader, Action::PrevSection) => call("prevSection"),
            (Context::Reader, Action::First) => call("first"),
            (Context::Reader, Action::Last) => call("last"),
            (Context::Reader, Action::Back) => call("back"),
            (Context::Reader, Action::Forward) => call("forward"),
            (Context::Reader, Action::ToggleStatus) => {
                let bar = &self.status.bar;
                bar.set_visible(!bar.is_visible());
            }
            (Context::Reader, Action::Close) => {
                if self.toast.is_visible() {
                    self.toast.set_visible(false);
                } else {
                    self.close_book();
                }
            }
            (Context::Library, Action::Move(dx, dy)) => self.library.move_cursor(dx, dy),
            (Context::Library, Action::First) => self.library.cursor_to_start(),
            (Context::Library, Action::Last) => self.library.cursor_to_end(),
            (Context::Library, Action::Open) => self.library.open_selected(),
            (Context::Library, Action::Filter) => self.library.start_filter(),
            (Context::Library, Action::Sort) => self.library.cycle_sort(),
            (Context::Library, Action::NextShelf) => self.library.step_shelf(1),
            (Context::Library, Action::PrevShelf) => self.library.step_shelf(-1),
            (Context::Library, Action::Rescan) => {
                if let Some(service) = self.service.borrow().as_ref() {
                    service.scan();
                }
                self.show_toast("Rescanning the library", false);
            }
            (Context::Library, Action::Close) => self.window.close(),
            (_, Action::Quit) => {
                self.save_progress();
                self.save_speed();
                if let Some(app) = self.window.application() {
                    app.quit();
                }
            }
            _ => {}
        }
    }

    fn show_toast(&self, text: &str, error: bool) {
        self.toast.set_label(text);
        if error {
            self.toast.add_css_class("error");
        } else {
            self.toast.remove_css_class("error");
        }
        self.toast.set_visible(true);
        if let Some(timer) = self.toast_timer.take() {
            timer.remove();
        }
        let toast = self.toast.clone();
        let timer = glib::timeout_add_local_once(Duration::from_secs(4), move || toast.set_visible(false));
        self.toast_timer.replace(Some(timer));
    }
}

fn file_stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn status_line() -> Status {
    let bar = gtk::Box::builder()
        .valign(gtk::Align::End)
        .spacing(12)
        .css_classes(["reader-status"])
        .build();
    let section = gtk::Label::builder()
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["section"])
        .build();
    let position = gtk::Label::builder().xalign(1.0).css_classes(["numeric"]).build();
    bar.append(&section);
    bar.append(&position);
    Status { bar, section, position }
}

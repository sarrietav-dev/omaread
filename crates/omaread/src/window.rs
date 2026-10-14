//! The window: the library, or a book filling it with the chrome floating over.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::{gio, glib};
use omaread_core::config::{self, Config};
use omaread_core::db::{Db, Progress};
use omaread_core::annotations::{self, Annotation, Kind};
use omaread_core::command::{self, Command, ExportFormat};
use omaread_core::library::{self, Book, Paths};
use omaread_core::lookup::{self as lookups, LookupResult};
use omaread_core::prefs::ReaderPrefs;
use omaread_core::speed::{ReadingSpeed, format_minutes};
use omaread_core::theme::Theme;

use crate::annotations_view::{AnnotationsPanel, NoteEditor, SelectionAction, SelectionBar, SelectionInfo};
use crate::help_view::HelpView;
use crate::keys::{Action, Context, Keymap};
use crate::library_view::LibraryView;
use crate::reader::{Event, Location, Reader, Rect, Selection, Sentence};
use crate::tts::{self, Tts};
use crate::lookup_view::LookupView;
use crate::palette::{Palette, Run};
use crate::search_view::{Hit, SearchView};
use crate::service::Service;
use crate::settings_view::{Scope, SettingsView};
use crate::toc_view::{TocItem, TocView};
use crate::theming::Theming;
use crate::voice_view::{VoiceAction, VoicePanel, VoiceState};

const LIBRARY: &str = "library";
const READER: &str = "reader";

/// How long reading position changes settle before they're written.
const SAVE_DELAY: Duration = Duration::from_millis(800);

const SPEED_SETTING: &str = "reading_speed";
const PREFS_SETTING: &str = "reader_prefs";
const FOLIATE_IMPORTED: &str = "foliate_imported";
const TTS_RATE: &str = "tts_rate";
const TTS_VOICE: &str = "tts_voice";
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
    palette: Rc<Palette>,
    help: Rc<HelpView>,
    tts: Tts,
    speech: RefCell<Option<Speech>>,
    tts_bar: gtk::Label,
    /// The voice panel: the list of voices and the pace.
    voice: Rc<VoicePanel>,
    /// Voices for the book's language, as speech-dispatcher lists them.
    voices: RefCell<Vec<omaread_core::ssip::Voice>>,
    tts_rate: Cell<i32>,
    tts_voice: RefCell<Option<String>>,
    lookup: Rc<LookupView>,
    /// Lookup answers from worker threads: (provider, query, answer).
    lookup_tx: async_channel::Sender<(usize, String, Result<LookupResult, String>)>,
    search: Rc<SearchView>,
    settings: Rc<SettingsView>,
    selection_bar: SelectionBar,
    note_editor: NoteEditor,
    annotations_panel: Rc<AnnotationsPanel>,
    /// The open book's highlights and bookmarks.
    annotations: RefCell<Vec<Annotation>>,
    /// What the selection bar is about: fresh text, or a highlight.
    focus: RefCell<Option<Focus>>,
    /// What the note editor will save to.
    note_target: RefCell<Option<Focus>>,
    last_color: RefCell<String>,
    /// Reading preferences for every book, and the open book's own, if any.
    prefs: RefCell<ReaderPrefs>,
    book_prefs: RefCell<Option<ReaderPrefs>>,
    toast: gtk::Label,
    toast_timer: RefCell<Option<glib::SourceId>>,
    theming: Rc<RefCell<Theming>>,
    keymap: RefCell<Keymap>,
    /// The config file's stamp when the keymap was built, to notice edits.
    config_stamp: Cell<Option<u64>>,
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

#[derive(Clone)]
enum Focus {
    Text(Selection),
    Highlight(i64),
}

/// Reading aloud: the block being read, and where in it.
struct Speech {
    playing: bool,
    sentences: Vec<Sentence>,
    index: usize,
    /// The sentence speech-dispatcher is reading (our number for it).
    seq: u64,
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
        let mut config = Config::load();
        let paths = Paths::from_env();
        let db = match paths.open_db() {
            Ok(db) => Some(db),
            Err(e) => {
                eprintln!("omaread: no library database ({e}); reading positions won't be saved");
                None
            }
        };
        config.adopt_old_default(db.as_ref().is_some_and(|db| db.stamps().is_ok_and(|s| !s.is_empty())));

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
        let library = LibraryView::new(
            {
                let with_window = with_window.clone();
                move |book| {
                    if let Some(win) = with_window() {
                        win.open_book(book);
                    }
                }
            },
            {
                let with_window = with_window.clone();
                move || {
                    if let Some(win) = with_window() {
                        win.add_folder();
                    }
                }
            },
            {
                let with_window = with_window.clone();
                move |path| {
                    if let Some(win) = with_window() {
                        win.remove_folder(&path);
                    }
                }
            },
        );

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
        let palette = Palette::new();
        let help = HelpView::new();
        let (tts_tx, tts_rx) = async_channel::unbounded::<tts::Event>();
        let tts = Tts::start(tts_tx);
        let tts_bar = gtk::Label::builder()
            .halign(gtk::Align::Center)
            .valign(gtk::Align::End)
            .visible(false)
            .css_classes(["tts-bar"])
            .build();
        let lookup = LookupView::new(config.lookup.providers());
        let (lookup_tx, lookup_rx) = async_channel::unbounded::<(usize, String, Result<LookupResult, String>)>();
        let search = SearchView::new();
        let voice = VoicePanel::new({
            let with_window = with_window.clone();
            move |action| {
                if let Some(win) = with_window() {
                    win.on_voice_action(action);
                }
            }
        });
        let settings = SettingsView::new({
            let with_window = with_window.clone();
            move |scope, prefs| {
                if let Some(win) = with_window() {
                    win.change_prefs(scope, prefs);
                }
            }
        });
        let reader_overlay = gtk::Overlay::new();
        reader_overlay.set_child(Some(&reader.view));
        reader_overlay.add_overlay(&status.bar);
        reader_overlay.add_overlay(&toc.root);
        reader_overlay.add_overlay(&search.root);
        reader_overlay.add_overlay(&lookup.root);
        reader_overlay.add_overlay(&tts_bar);
        reader_overlay.add_overlay(&voice.root);
        reader_overlay.add_overlay(&settings.root);
        let selection_bar = SelectionBar::new({
            let with_window = with_window.clone();
            move |action| {
                if let Some(win) = with_window() {
                    win.on_selection_action(action);
                }
            }
        });
        let note_editor = NoteEditor::new();
        let annotations_panel = AnnotationsPanel::new();
        reader_overlay.add_overlay(&annotations_panel.root);
        reader_overlay.add_overlay(&selection_bar.root);
        reader_overlay.add_overlay(&note_editor.root);

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
        overlay.add_overlay(&palette.root);
        overlay.add_overlay(&help.root);
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
            palette,
            help,
            tts,
            speech: RefCell::default(),
            tts_bar,
            voice,
            voices: RefCell::default(),
            tts_rate: Cell::new(0),
            tts_voice: RefCell::default(),
            lookup,
            lookup_tx,
            search,
            settings,
            prefs: RefCell::new(
                db.as_ref()
                    .and_then(|db| db.setting(PREFS_SETTING).ok().flatten())
                    .and_then(|s| ReaderPrefs::from_json(&s))
                    .unwrap_or_default(),
            ),
            book_prefs: RefCell::default(),
            selection_bar,
            note_editor,
            annotations_panel,
            annotations: RefCell::default(),
            focus: RefCell::default(),
            note_target: RefCell::default(),
            last_color: RefCell::new("yellow".into()),
            toast,
            toast_timer: RefCell::default(),
            theming,
            keymap: RefCell::new(Keymap::new(&config.keymap)),
            config_stamp: Cell::new(Config::fingerprint()),
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
        if let Some(db) = &win.db {
            win.tts_rate.set(db.setting(TTS_RATE).ok().flatten().and_then(|r| r.parse().ok()).unwrap_or(0));
            win.tts_voice.replace(db.setting(TTS_VOICE).ok().flatten());
        }
        glib::spawn_future_local({
            let win = Rc::downgrade(&win);
            async move {
                while let Ok(event) = tts_rx.recv().await {
                    let Some(win) = win.upgrade() else { break };
                    win.on_tts(event);
                }
            }
        });
        glib::spawn_future_local({
            let win = Rc::downgrade(&win);
            async move {
                while let Ok((index, query, result)) = lookup_rx.recv().await {
                    let Some(win) = win.upgrade() else { break };
                    if let (Ok(r), Some(db), Some(p)) = (&result, &win.db, win.lookup.providers().get(index)) {
                        db.cache_lookup(&p.name, &query, r, library::now());
                    }
                    win.lookup.show(index, &query, result);
                }
            }
        });
        win.palette.entry.connect_activate({
            let win = Rc::downgrade(&win);
            move |_| {
                if let Some(win) = win.upgrade() {
                    win.run_palette();
                }
            }
        });
        win.search.entry.connect_activate({
            let win = Rc::downgrade(&win);
            move |_| {
                if let Some(win) = win.upgrade() {
                    win.start_search();
                }
            }
        });
        win.me.replace(Rc::downgrade(&win));
        *this.borrow_mut() = Some(win.clone());

        // Follow config.toml: a rebinding takes effect without a restart.
        glib::timeout_add_local(Duration::from_millis(1000), {
            let win = Rc::downgrade(&win);
            move || {
                let Some(win) = win.upgrade() else { return glib::ControlFlow::Break };
                win.reload_config();
                glib::ControlFlow::Continue
            }
        });

        win.library.set_folders(config.library_folders());
        let service = Service::start(paths, config.library_folders(), {
            let win = Rc::downgrade(&win);
            move |books| {
                if let Some(win) = win.upgrade() {
                    win.library.set_scanning(false);
                    win.library.set_books(books);
                    win.import_foliate_once();
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
        self.stop_speech();
        self.toc.close();
        self.toc.set_items(&[]);
        self.toc_items.borrow_mut().clear();
        self.page.take();
        self.last_location.take();
        self.settings.close();
        self.voice.close();
        self.book_prefs.replace(
            self.db
                .as_ref()
                .and_then(|db| db.book_prefs(&book.identity).ok().flatten())
                .and_then(|s| ReaderPrefs::from_json(&s)),
        );
        self.apply_prefs();
        self.close_overlays();
        let list = self.db.as_ref().and_then(|db| db.annotations(&book.identity).ok()).unwrap_or_default();
        self.annotations.replace(list);
        self.push_annotations();
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
        self.close_overlays();
        self.lookup.close();
        self.stop_speech();
        self.save_progress();
        self.save_speed();
        self.toc.close();
        self.palette.close();
        self.help.close();
        self.search.close();
        self.search.clear();
        if let Some(current) = self.current.take() {
            let mut book = current.book;
            book.progress = Some(current.progress);
            self.library.update_book(book);
        }
        self.settings.close();
        self.voice.close();
        self.book_prefs.take();
        self.reader.close();
        if self.theming.borrow_mut().set_book_theme(None) {
            self.push_palette(&self.theming.borrow().theme().clone());
        }
        self.window.set_title(Some("Omaread"));
        self.stack.set_visible_child_name(LIBRARY);
        self.library.focus();
    }

    fn context(&self) -> Context {
        if self.help.is_open() {
            Context::Help
        } else if self.stack.visible_child_name().as_deref() != Some(READER) {
            Context::Library
        } else if self.toc.is_open() {
            Context::Toc
        } else if self.search.is_open() {
            Context::Search
        } else if self.lookup.is_open() {
            Context::Lookup
        } else if self.voice.is_open() {
            Context::Voice
        } else if self.settings.is_open() {
            Context::Settings
        } else if self.annotations_panel.is_open() {
            Context::Annotations
        } else if self.selection_bar.is_open() {
            Context::Selection
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
            Event::Ready => {
                let theme = self.theming.borrow().theme().clone();
                self.push_palette(&theme);
                self.reader.call("setPrefs", &serde_json::to_string(&self.effective_prefs()).unwrap_or_default());
            }
            Event::BookReady { toc, .. } => {
                self.toc.set_items(&toc);
                self.toc_items.replace(toc);
                self.reader.view.grab_focus();
                // Imported annotations come without a chapter; ask the book.
                let unplaced: Vec<String> =
                    self.annotations.borrow().iter().filter(|a| a.chapter.is_empty()).map(|a| a.cfi.clone()).collect();
                if !unplaced.is_empty() {
                    self.reader.call("locate", &serde_json::to_string(&unplaced).unwrap_or_default());
                }
            }
            Event::Selection(selection) => self.show_selection(selection),
            Event::SelectionCleared => {
                if matches!(*self.focus.borrow(), Some(Focus::Text(_))) {
                    self.selection_bar.hide();
                    self.focus.take();
                }
            }
            Event::AnnotationClick { cfi, rect } => {
                if let Some(a) = self.highlight_at(&cfi) {
                    self.show_card(&a.note, Some(&a.color), true, rect);
                    self.focus.replace(Some(Focus::Highlight(a.id)));
                }
            }
            Event::Located { items } => self.place_annotations(items),
            Event::TtsBlock { sentences } => {
                let mut speech = self.speech.borrow_mut();
                if let Some(s) = speech.as_mut() {
                    s.sentences = sentences;
                    s.index = 0;
                }
                let playing = speech.as_ref().is_some_and(|s| s.playing);
                drop(speech);
                if playing {
                    self.speak_current();
                }
            }
            Event::TtsEnd => {
                self.stop_speech();
                self.show_toast("Finished reading aloud", false);
            }
            Event::SearchResults { chapter, items } => self.search.add(&chapter, items),
            Event::SearchProgress { progress } => self.search.progress(progress),
            Event::SearchDone { error, .. } => {
                self.search.done(error.as_deref());
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
                if matches!(*self.focus.borrow(), Some(Focus::Text(_))) {
                    self.selection_bar.hide();
                    self.focus.take();
                }
                self.learn_pace(&location);
                self.update_status(&location);
                self.record(&location);
                self.last_location.replace(Some(location));
            }
            Event::ExternalLink { href } => self.open_link(&href),
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
            if let Some(chapter) = &location.toc_label {
                current.progress.chapter = Some(chapter.clone());
            }
            if let Some(time) = location.time {
                current.progress.minutes_left = Some(self.speed.get().minutes_for(time.total * FOLIATE_BYTES_PER_MINUTE));
            }
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
        if !l.bookmarks.is_empty() {
            parts.insert(0, "bookmarked".into());
        }
        self.status.position.set_label(&parts.join("  ·  "));
    }

    /// The preferences in force: the book's own, or everyone's.
    fn effective_prefs(&self) -> ReaderPrefs {
        self.book_prefs.borrow().clone().unwrap_or_else(|| self.prefs.borrow().clone())
    }

    fn apply_prefs(&self) {
        let prefs = self.effective_prefs();
        self.reader.call("setPrefs", &serde_json::to_string(&prefs).unwrap_or_default());
        let changed = self.theming.borrow_mut().set_book_theme(prefs.theme.as_deref());
        if changed {
            let theme = self.theming.borrow().theme().clone();
            self.push_palette(&theme);
        }
    }

    /// From the settings panel. `None` drops the book's own preferences.
    fn change_prefs(&self, scope: Scope, prefs: Option<ReaderPrefs>) {
        let identity = self.current.borrow().as_ref().map(|c| c.book.identity.clone());
        match (scope, prefs) {
            (Scope::AllBooks, Some(p)) => {
                if let Some(db) = &self.db {
                    let _ = db.set_setting(PREFS_SETTING, &p.to_json());
                }
                self.prefs.replace(p);
            }
            (Scope::ThisBook, Some(p)) => {
                if let (Some(db), Some(id)) = (&self.db, &identity) {
                    let _ = db.set_book_prefs(id, Some(&p.to_json()));
                }
                self.book_prefs.replace(Some(p));
            }
            (_, None) => {
                if let (Some(db), Some(id)) = (&self.db, &identity) {
                    let _ = db.set_book_prefs(id, None);
                }
                self.book_prefs.take();
                self.settings.show(&self.prefs.borrow(), Scope::AllBooks);
            }
        }
        self.apply_prefs();
    }

    /// A quick change from a key: to the book's own set if it has one.
    fn tweak_prefs(&self, change: impl FnOnce(&mut ReaderPrefs)) {
        let scope = if self.book_prefs.borrow().is_some() { Scope::ThisBook } else { Scope::AllBooks };
        let mut prefs = self.effective_prefs();
        change(&mut prefs);
        let prefs = prefs.sanitized();
        self.change_prefs(scope, Some(prefs.clone()));
        if self.settings.is_open() {
            self.settings.show(&prefs, scope);
        }
    }

    fn open_settings(&self) {
        self.voice.close();
        let scope = if self.book_prefs.borrow().is_some() { Scope::ThisBook } else { Scope::AllBooks };
        self.settings.open(&self.effective_prefs(), scope);
    }

    fn open_toc(&self) {
        if self.toc.is_empty() {
            self.show_toast("This book has no table of contents", false);
            return;
        }
        let href = self.last_location.borrow().as_ref().and_then(|l| l.toc_href.clone());
        self.toc.open(href.as_deref());
    }

    fn open_help(&self) {
        self.help.open();
    }

    fn close_help(&self) {
        self.help.close();
        if self.stack.visible_child_name().as_deref() == Some(READER) {
            self.reader.view.grab_focus();
        } else {
            self.library.focus();
        }
    }

    fn close_palette(&self) {
        let from = self.palette.context();
        self.palette.close();
        match from {
            Some(Context::Library) => self.library.focus(),
            _ => {
                self.reader.view.grab_focus();
            }
        }
    }

    /// Enter in the palette: a command with an argument, or the chosen entry.
    fn run_palette(&self) {
        let text = self.palette.text();
        let context = self.palette.context().unwrap_or(Context::Reader);
        let typed = text.trim();
        let entry = self.palette.selected();
        self.close_palette();
        if context != Context::Library && !typed.is_empty() && command::parse(typed).is_ok() {
            self.run_command(typed);
            return;
        }
        match entry.map(|e| e.run) {
            Some(Run::Action(action)) => self.run(if context == Context::Library { Context::Library } else { Context::Reader }, action),
            Some(Run::Line("")) => self.show_toast("Type where to go: 42%, p 120, ch 7", false),
            Some(Run::Line(line)) => self.run_command(line),
            None if !typed.is_empty() => self.run_command(typed),
            None => {}
        }
    }

    /// Look the focused text up with the provider on `key`.
    fn look_up(&self, key: char) {
        let Some(index) = self.lookup.provider_for_key(key) else { return };
        let (text, lang) = match self.focus.borrow().as_ref() {
            Some(Focus::Text(s)) => (s.text.clone(), s.lang.clone()),
            Some(Focus::Highlight(id)) => match self.annotations.borrow().iter().find(|a| a.id == *id) {
                Some(a) => (a.text.clone(), self.current.borrow().as_ref().and_then(|c| c.book.language.clone())),
                None => return,
            },
            // From the card itself: same text, another provider.
            None if self.lookup.is_open() => self.lookup.query(),
            None => return,
        };
        let lang = lang.or_else(|| self.current.borrow().as_ref().and_then(|c| c.book.language.clone()));
        self.selection_bar.hide();
        self.focus.take();
        self.lookup_with(index, text, lang);
    }

    fn lookup_with(&self, index: usize, text: String, lang: Option<String>) {
        let Some(spec) = self.lookup.providers().get(index).cloned() else { return };
        // A `url` provider is just the browser.
        if spec.kind == "url" {
            match lookups::run(&spec, &text, lang.as_deref()) {
                Ok(LookupResult { open: Some(url), .. }) => self.open_link(&url),
                Ok(_) => {}
                Err(e) => self.show_toast(&e, true),
            }
            return;
        }
        self.lookup.open(index, &text, lang.as_deref());
        if let Some(cached) = self.db.as_ref().and_then(|db| db.cached_lookup(&spec.name, &text, library::now())) {
            self.lookup.show(index, &text, Ok(cached));
            return;
        }
        let tx = self.lookup_tx.clone();
        std::thread::spawn(move || {
            let result = lookups::run(&spec, &text, lang.as_deref());
            let _ = tx.send_blocking((index, text, result));
        });
    }

    // --- Reading aloud ------------------------------------------------------

    fn on_tts(&self, event: tts::Event) {
        match event {
            tts::Event::Ended(seq) => {
                let next = {
                    let mut speech = self.speech.borrow_mut();
                    let Some(s) = speech.as_mut().filter(|s| s.playing && s.seq == seq) else { return };
                    s.index += 1;
                    s.index < s.sentences.len()
                };
                if next {
                    self.speak_current();
                } else {
                    self.reader.call("ttsNext", "");
                }
            }
            tts::Event::Voices(all) => {
                let lang = self.current.borrow().as_ref().and_then(|c| c.book.language.clone()).unwrap_or_default();
                let lang = lang.split(['-', '_']).next().unwrap_or("").to_lowercase();
                let mut voices: Vec<_> = all.iter().filter(|v| !lang.is_empty() && v.language.to_lowercase().starts_with(&lang)).cloned().collect();
                if voices.is_empty() {
                    voices = all;
                }
                self.voices.replace(voices);
                self.update_tts_bar();
            }
            tts::Event::Error(e) => {
                self.show_toast(&e, true);
                self.stop_speech();
            }
        }
    }

    fn toggle_speech(&self, from_selection: bool) {
        let state = self.speech.borrow().as_ref().map(|s| s.playing);
        match state {
            None => self.start_speech(from_selection),
            Some(true) if !from_selection => {
                if let Some(s) = self.speech.borrow_mut().as_mut() {
                    s.playing = false;
                    s.seq += 1;
                }
                self.tts.send(tts::Command::Stop);
                self.open_voice();
                self.update_tts_bar();
            }
            Some(false) if !from_selection => {
                if let Some(s) = self.speech.borrow_mut().as_mut() {
                    s.playing = true;
                }
                self.open_voice();
                self.speak_current();
            }
            Some(_) => {
                self.tts.send(tts::Command::Stop);
                self.start_speech(true);
            }
        }
    }

    fn start_speech(&self, from_selection: bool) {
        let lang = self.current.borrow().as_ref().and_then(|c| c.book.language.clone());
        if let Some(lang) = lang {
            self.tts.send(tts::Command::Language(lang));
        }
        self.tts.send(tts::Command::Rate(self.tts_rate.get()));
        if let Some(v) = self.tts_voice.borrow().clone() {
            self.tts.send(tts::Command::Voice(v));
        }
        if self.voices.borrow().is_empty() {
            self.tts.send(tts::Command::ListVoices);
        }
        let seq = self.speech.borrow().as_ref().map(|s| s.seq + 1).unwrap_or(1);
        self.speech.replace(Some(Speech { playing: true, sentences: Vec::new(), index: 0, seq }));
        self.close_selection();
        self.reader.call("ttsStart", &serde_json::json!({ "fromSelection": from_selection }).to_string());
        // Reading aloud opens the controls: voice and pace are right there.
        self.open_voice();
        self.update_tts_bar();
    }

    /// Mark the current sentence on the page and send it to be spoken.
    fn speak_current(&self) {
        let (mark, text, seq) = {
            let mut speech = self.speech.borrow_mut();
            let Some(s) = speech.as_mut() else { return };
            let Some(sentence) = s.sentences.get(s.index) else { return };
            s.seq += 1;
            (sentence.mark.clone(), sentence.text.clone(), s.seq)
        };
        self.reader.call("ttsMark", &serde_json::to_string(&mark).unwrap_or_default());
        self.tts.send(tts::Command::Speak { seq, text });
        self.update_tts_bar();
    }

    /// `]` / `[`: a sentence on or back, within the block or into the next.
    fn step_sentence(&self, delta: i32) {
        let Some((playing, index, len)) = self.speech.borrow().as_ref().map(|s| (s.playing, s.index, s.sentences.len())) else {
            return;
        };
        self.tts.send(tts::Command::Stop);
        let target = index as i64 + delta as i64;
        if target < 0 {
            self.reader.call("ttsPrev", "");
            return;
        }
        if target as usize >= len {
            self.reader.call("ttsNext", "");
            return;
        }
        if let Some(s) = self.speech.borrow_mut().as_mut() {
            s.index = target as usize;
            s.seq += 1;
        }
        if playing {
            self.speak_current();
        } else {
            let mark = self.speech.borrow().as_ref().and_then(|s| s.sentences.get(s.index).and_then(|x| x.mark.clone()));
            self.reader.call("ttsMark", &serde_json::to_string(&mark).unwrap_or_default());
        }
    }

    fn stop_speech(&self) {
        if self.speech.take().is_some() {
            self.tts.send(tts::Command::Stop);
            self.reader.call("ttsStop", "");
        }
        self.tts_bar.set_visible(false);
        self.refresh_voice_panel();
    }

    fn change_rate(&self, delta: i32) {
        let rate = (self.tts_rate.get() + delta).clamp(-100, 100);
        self.tts_rate.set(rate);
        self.tts.send(tts::Command::Rate(rate));
        if let Some(db) = &self.db {
            let _ = db.set_setting(TTS_RATE, &rate.to_string());
        }
        // Takes effect from the next sentence; restart this one at the new pace.
        if self.speech.borrow().as_ref().is_some_and(|s| s.playing) {
            self.tts.send(tts::Command::Stop);
            self.speak_current();
        }
        self.update_tts_bar();
    }

    fn next_voice(&self) {
        eprintln!("omaread: DEBUG next_voice n={}", self.voices.borrow().len());
        let voices = self.voices.borrow().clone();
        if voices.is_empty() {
            self.tts.send(tts::Command::ListVoices);
            return;
        }
        let current = self.tts_voice.borrow().clone();
        let i = voices.iter().position(|v| Some(&v.name) == current.as_ref()).map(|i| (i + 1) % voices.len()).unwrap_or(0);
        self.set_voice(voices[i].name.clone());
    }

    /// Adopt a voice, at once if something's playing, and remember it.
    fn set_voice(&self, voice: String) {
        self.tts.send(tts::Command::Voice(voice.clone()));
        if let Some(db) = &self.db {
            let _ = db.set_setting(TTS_VOICE, &voice);
        }
        self.tts_voice.replace(Some(voice));
        if self.speech.borrow().as_ref().is_some_and(|s| s.playing) {
            self.tts.send(tts::Command::Stop);
            self.speak_current();
        }
        self.update_tts_bar();
    }

    // --- The voice panel ----------------------------------------------------

    fn open_voice(&self) {
        eprintln!("omaread: DEBUG open_voice (was_open={})", self.voice.is_open());
        self.toc.close();
        self.search.close();
        self.annotations_panel.close();
        self.settings.close();
        // Ask for the list if this is the first time; it arrives as an event.
        if self.voices.borrow().is_empty() {
            self.tts.send(tts::Command::ListVoices);
        }
        self.voice.open();
        self.update_tts_bar();
    }

    /// Enter or a click in the panel: use the highlighted voice.
    fn pick_voice(&self) {
        if let Some(voice) = self.voice.selected() {
            self.set_voice(voice.name);
        }
    }

    fn on_voice_action(&self, action: VoiceAction) {
        match action {
            VoiceAction::Pick(name) => self.set_voice(name),
            VoiceAction::Rate(delta) => self.change_rate(delta),
            VoiceAction::Toggle => self.toggle_speech(false),
            VoiceAction::Sentence(delta) => self.step_sentence(delta),
        }
    }

    /// Bring the panel in step with what's playing, if it's up.
    fn refresh_voice_panel(&self) {
        if !self.voice.is_open() {
            return;
        }
        let voices = self.voices.borrow().clone();
        let current = self.tts_voice.borrow().clone();
        self.voice.set_voices(&voices, current.as_deref());
        let (state, sentence) = match self.speech.borrow().as_ref() {
            None => (VoiceState::NotReading, None),
            Some(s) => {
                let sentence = (!s.sentences.is_empty()).then(|| (s.index, s.sentences.len()));
                let state = if s.playing { VoiceState::Playing } else { VoiceState::Paused };
                (state, sentence)
            }
        };
        self.voice.set_state(state, sentence, self.tts_rate.get());
    }

    fn update_tts_bar(&self) {
        self.refresh_voice_panel();
        let speech = self.speech.borrow();
        let Some(speech) = speech.as_ref() else {
            self.tts_bar.set_visible(false);
            return;
        };
        let voice = self
            .tts_voice
            .borrow()
            .as_ref()
            .and_then(|name| self.voices.borrow().iter().find(|v| &v.name == name).map(|v| v.label()))
            .unwrap_or_else(|| "default voice".into());
        let rate = self.tts_rate.get();
        let speed = if rate == 0 { "normal speed".to_string() } else { format!("speed {rate:+}") };
        let state = if speech.playing { "Reading aloud" } else { "Paused" };
        let place = if speech.sentences.is_empty() { String::new() } else { format!(" {}/{}", speech.index + 1, speech.sentences.len()) };
        let toggle = if speech.playing { "s pause" } else { "s resume" };
        self.tts_bar.set_label(&format!(
            "{state}{place}  ·  {voice}  ·  {speed}      {toggle}   V voices   esc stop"
        ));
        // The panel is the fuller view: when it's up, the bar would only repeat it.
        self.tts_bar.set_visible(!self.voice.is_open());
    }

    fn open_search(&self) {
        self.toc.close();
        self.annotations_panel.close();
        self.voice.close();
        self.search.open();
    }

    fn start_search(&self) {
        let query = self.search.query();
        if query.is_empty() {
            self.search.clear();
            self.reader.call("clearSearch", "");
            return;
        }
        let o = self.search.options();
        self.search.begin();
        let args = serde_json::json!({
            "query": query,
            "matchCase": o.match_case,
            "matchWholeWords": o.whole_words,
            "matchDiacritics": o.diacritics,
        });
        self.reader.call("search", &args.to_string());
    }

    fn go_to_hit(&self, hit: Option<Hit>) {
        match hit {
            Some(hit) => self.reader.call("goTo", &serde_json::to_string(&hit.cfi).unwrap_or_default()),
            None => self.show_toast("No search results; / to search", false),
        }
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
            Ok(Command::Export(format)) => self.export(format),
            Err(e) => self.show_toast(&e, true),
        }
    }

    fn page_size(&self) -> (i32, i32) {
        (self.reader.view.width(), self.reader.view.height())
    }

    /// Every panel and bar over the book, closed.
    fn close_overlays(&self) {
        self.selection_bar.hide();
        self.note_editor.close();
        self.annotations_panel.close();
        self.focus.take();
        self.note_target.take();
    }

    fn push_annotations(&self) {
        let list = self.annotations.borrow();
        let highlights: Vec<serde_json::Value> = list
            .iter()
            .filter(|a| a.kind == Kind::Highlight)
            .map(|a| serde_json::json!({ "cfi": a.cfi, "color": a.color }))
            .collect();
        let bookmarks: Vec<&str> = list.iter().filter(|a| a.kind == Kind::Bookmark).map(|a| a.cfi.as_str()).collect();
        self.reader.call("setHighlights", &serde_json::to_string(&highlights).unwrap_or_default());
        self.reader.call("setBookmarks", &serde_json::to_string(&bookmarks).unwrap_or_default());
    }

    fn identity(&self) -> Option<String> {
        self.current.borrow().as_ref().map(|c| c.book.identity.clone())
    }

    /// Store an annotation and keep the in-memory list in step.
    fn store(&self, mut a: Annotation) -> Option<Annotation> {
        let identity = self.identity()?;
        if let Some(db) = &self.db {
            match db.save_annotation(&identity, &a) {
                Ok(id) => a.id = id,
                Err(e) => {
                    self.show_toast(&format!("Could not save: {e}"), true);
                    return None;
                }
            }
        }
        let mut list = self.annotations.borrow_mut();
        match list.iter_mut().find(|x| x.kind == a.kind && x.cfi == a.cfi) {
            Some(x) => *x = a.clone(),
            None => list.push(a.clone()),
        }
        list.sort_by(|x, y| x.fraction.total_cmp(&y.fraction).then(x.created_at.cmp(&y.created_at)));
        drop(list);
        self.refresh_panel();
        Some(a)
    }

    fn remove(&self, id: i64) -> Option<Annotation> {
        let identity = self.identity()?;
        let removed = {
            let mut list = self.annotations.borrow_mut();
            let i = list.iter().position(|a| a.id == id)?;
            list.remove(i)
        };
        if let Some(db) = &self.db {
            let _ = db.delete_annotation(&identity, id);
        }
        match removed.kind {
            Kind::Highlight => self.reader.call("removeHighlight", &serde_json::to_string(&removed.cfi).unwrap_or_default()),
            Kind::Bookmark => self.push_annotations(),
        }
        self.refresh_panel();
        Some(removed)
    }

    fn refresh_panel(&self) {
        if self.annotations_panel.is_open() {
            self.annotations_panel.set(&self.annotations.borrow());
        }
    }

    fn place_annotations(&self, items: Vec<crate::reader::Located>) {
        let updated: Vec<Annotation> = {
            let mut list = self.annotations.borrow_mut();
            items
                .into_iter()
                .filter_map(|item| {
                    let a = list.iter_mut().find(|a| a.cfi == item.cfi)?;
                    a.chapter = item.chapter;
                    a.fraction = item.fraction;
                    Some(a.clone())
                })
                .collect()
        };
        for a in updated {
            self.store(a);
        }
    }

    fn show_selection(&self, selection: Selection) {
        if selection.text.trim().is_empty() {
            return;
        }
        // A selection that lands exactly on a highlight opens as that
        // highlight, so `h` toggles it off instead of stacking a second one.
        let existing = self.highlight_at(&selection.cfi);
        let (note, color) = match &existing {
            Some(a) => (a.note.as_str(), Some(a.color.as_str())),
            None => ("", None),
        };
        self.show_card(note, color, existing.is_some(), selection.rect);
        self.focus.replace(Some(Focus::Text(selection)));
    }

    /// The highlight drawn at exactly this CFI, if any.
    fn highlight_at(&self, cfi: &str) -> Option<Annotation> {
        self.annotations.borrow().iter().find(|a| a.kind == Kind::Highlight && a.cfi == cfi).cloned()
    }

    /// Fill and place the card over the book.
    fn show_card(&self, note: &str, color: Option<&str>, existing: bool, rect: Option<Rect>) {
        let hint = if existing {
            "1–5 recolor   h remove   a note   y copy   x delete   esc".to_string()
        } else {
            format!("1–5 color   h highlight   a note   y copy   {}   esc", self.lookup_hint())
        };
        self.selection_bar.show(SelectionInfo { note, color, existing, hint: &hint, rect }, self.page_size());
    }

    /// A click in the selection card.
    fn on_selection_action(&self, action: SelectionAction) {
        match action {
            SelectionAction::Color(i) => self.color(Some(i)),
            SelectionAction::Toggle => self.toggle_highlight(),
            SelectionAction::Note(text) => self.note_selection(&text),
            SelectionAction::Copy => self.copy_focus(),
            SelectionAction::Delete => self.delete_focus(),
        }
    }

    /// `h`: highlight a fresh selection, or remove the highlight the card is
    /// about.
    fn toggle_highlight(&self) {
        let focus = self.focus.borrow().clone();
        let existing = match &focus {
            Some(Focus::Highlight(id)) => Some(*id),
            Some(Focus::Text(s)) => self.highlight_at(&s.cfi).map(|a| a.id),
            None => None,
        };
        match existing {
            Some(id) => {
                self.remove(id);
                self.close_selection();
            }
            None => self.color(None),
        }
    }

    /// The note field in the card was saved.
    fn note_selection(&self, note: &str) {
        let focus = self.focus.borrow().clone();
        match focus {
            Some(Focus::Text(s)) => {
                // Nothing to save yet; keep the card open.
                if note.trim().is_empty() {
                    return;
                }
                let color = self.last_color.borrow().clone();
                self.highlight_from(&s, &color, note);
            }
            Some(Focus::Highlight(id)) => {
                let found = self.annotations.borrow().iter().find(|a| a.id == id).cloned();
                if let Some(mut a) = found {
                    a.note = note.to_string();
                    a.updated_at = library::now();
                    self.store(a);
                }
            }
            None => return,
        }
        self.close_selection();
    }

    /// `x` on a clicked highlight.
    fn delete_focus(&self) {
        let focus = self.focus.borrow().clone();
        if let Some(Focus::Highlight(id)) = focus {
            self.remove(id);
            self.close_selection();
        }
    }

    /// "d dictionary   w wikipedia   t translate", from the providers.
    fn lookup_hint(&self) -> String {
        self.lookup.providers().iter().map(|p| format!("{} {}", p.key, p.name.to_lowercase())).collect::<Vec<_>>().join("   ")
    }

    fn highlight_from(&self, s: &Selection, color: &str, note: &str) -> Option<Annotation> {
        let now = library::now();
        let a = self.store(Annotation {
            id: 0,
            kind: Kind::Highlight,
            cfi: s.cfi.clone(),
            color: color.to_string(),
            text: s.text.trim().to_string(),
            note: note.to_string(),
            chapter: s.chapter.clone(),
            fraction: s.fraction,
            created_at: now,
            updated_at: now,
        })?;
        self.reader.call("addHighlight", &serde_json::json!({ "cfi": a.cfi, "color": a.color }).to_string());
        Some(a)
    }

    /// h / 1–5 on a selection or a highlight.
    fn color(&self, color: Option<usize>) {
        let color = match color {
            Some(i) => annotations::COLORS[i.min(4)].to_string(),
            None => self.last_color.borrow().clone(),
        };
        self.last_color.replace(color.clone());
        // A note typed into the card is saved with the highlight.
        let note = self.selection_bar.note();
        let focus = self.focus.take();
        self.selection_bar.hide();
        match focus {
            Some(Focus::Text(s)) => {
                self.highlight_from(&s, &color, &note);
            }
            Some(Focus::Highlight(id)) => {
                let found = self.annotations.borrow().iter().find(|a| a.id == id).cloned();
                if let Some(mut a) = found {
                    a.color = color;
                    a.updated_at = library::now();
                    if let Some(a) = self.store(a) {
                        self.reader.call("addHighlight", &serde_json::json!({ "cfi": a.cfi, "color": a.color }).to_string());
                    }
                }
            }
            None => {}
        }
    }

    fn open_note(&self, target: Focus) {
        let (quote, note) = match &target {
            Focus::Text(s) => (s.text.clone(), String::new()),
            Focus::Highlight(id) => match self.annotations.borrow().iter().find(|a| a.id == *id) {
                Some(a) => (a.text.clone(), a.note.clone()),
                None => return,
            },
        };
        self.selection_bar.hide();
        self.focus.take();
        self.note_target.replace(Some(target));
        self.note_editor.open(&quote, &note);
    }

    fn save_note(&self) {
        let note = self.note_editor.text().trim().to_string();
        self.note_editor.close();
        match self.note_target.take() {
            Some(Focus::Text(s)) => {
                let color = self.last_color.borrow().clone();
                self.highlight_from(&s, &color, &note);
            }
            Some(Focus::Highlight(id)) => {
                let found = self.annotations.borrow().iter().find(|a| a.id == id).cloned();
                if let Some(mut a) = found {
                    a.note = note;
                    a.updated_at = library::now();
                    self.store(a);
                }
            }
            None => {}
        }
        self.focus_after_overlay();
    }

    fn cancel_note(&self) {
        self.note_editor.close();
        self.note_target.take();
        self.focus_after_overlay();
    }

    fn focus_after_overlay(&self) {
        if self.annotations_panel.is_open() {
            self.annotations_panel.move_cursor(0);
        } else {
            self.reader.view.grab_focus();
        }
    }

    fn copy_focus(&self) {
        let text = match self.focus.borrow().as_ref() {
            Some(Focus::Text(s)) => s.text.clone(),
            Some(Focus::Highlight(id)) => self.annotations.borrow().iter().find(|a| a.id == *id).map(|a| a.text.clone()).unwrap_or_default(),
            None => return,
        };
        self.window.clipboard().set_text(text.trim());
        self.show_toast("Copied", false);
        self.close_selection();
    }

    fn close_selection(&self) {
        self.selection_bar.hide();
        self.focus.take();
        self.reader.call("clearSelection", "");
        self.reader.view.grab_focus();
    }

    fn toggle_bookmark(&self) {
        let location = self.last_location.borrow().clone();
        let Some(location) = location else { return };
        let here: Vec<i64> = self
            .annotations
            .borrow()
            .iter()
            .filter(|a| a.kind == Kind::Bookmark && location.bookmarks.contains(&a.cfi))
            .map(|a| a.id)
            .collect();
        if here.is_empty() {
            let Some(cfi) = location.page_start.clone() else { return };
            let now = library::now();
            self.store(Annotation {
                id: 0,
                kind: Kind::Bookmark,
                cfi: cfi.clone(),
                color: String::new(),
                text: String::new(),
                note: String::new(),
                chapter: location.toc_label.clone().unwrap_or_default(),
                fraction: location.fraction,
                created_at: now,
                updated_at: now,
            });
            self.push_annotations();
            let mut shown = location;
            shown.bookmarks = vec![cfi];
            self.update_status(&shown);
            self.last_location.replace(Some(shown));
            self.show_toast("Bookmarked", false);
        } else {
            for id in here {
                self.remove(id);
            }
            let mut shown = location;
            shown.bookmarks.clear();
            self.update_status(&shown);
            self.last_location.replace(Some(shown));
            self.show_toast("Bookmark removed", false);
        }
    }

    fn export(&self, format: ExportFormat) {
        let Some(book) = self.current.borrow().as_ref().map(|c| c.book.clone()) else { return };
        let list = self.annotations.borrow().clone();
        if !list.iter().any(|a| a.kind == Kind::Highlight) && format == ExportFormat::Markdown {
            self.show_toast("Nothing to export yet: no highlights in this book", true);
            return;
        }
        let (text, ext) = match format {
            ExportFormat::Markdown => (annotations::to_markdown(&book.title, &book.authors, &list), "md"),
            ExportFormat::Json => (annotations::to_json(&book, &list), "json"),
        };
        let dir = annotations::export_dir();
        let path = dir.join(annotations::file_name(&book.title, ext));
        match std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, text)) {
            Ok(()) => self.show_toast(&format!("Exported to {}", path.display()), false),
            Err(e) => self.show_toast(&format!("Could not export: {e}"), true),
        }
    }

    /// Once: bring Foliate's positions, highlights and bookmarks over.
    fn import_foliate_once(&self) {
        let Some(db) = &self.db else { return };
        if db.setting(FOLIATE_IMPORTED).ok().flatten().is_some() {
            return;
        }
        let dir = annotations::foliate_dir();
        if !dir.is_dir() {
            let _ = db.set_setting(FOLIATE_IMPORTED, "none");
            return;
        }
        let books = library::load(db).unwrap_or_default();
        if books.is_empty() {
            return; // Wait for a scan, or nothing will match.
        }
        match annotations::import_foliate(db, &dir, &books, library::now()) {
            Ok(r) => {
                let _ = db.set_setting(FOLIATE_IMPORTED, &library::now().to_string());
                if r.books > 0 {
                    self.show_toast(
                        &format!(
                            "From Foliate: {} books, {} positions, {} highlights, {} bookmarks",
                            r.books, r.positions, r.annotations, r.bookmarks
                        ),
                        false,
                    );
                    if let Ok(books) = library::load(db) {
                        self.library.set_books(books);
                    }
                }
            }
            Err(e) => eprintln!("omaread: Foliate import failed: {e}"),
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

    /// Re-read config.toml when it changes: rebuild the keymap, and follow
    /// the library's folders.
    fn reload_config(&self) {
        let stamp = Config::fingerprint();
        if stamp == self.config_stamp.get() {
            return;
        }
        self.config_stamp.set(stamp);
        let config = Config::load();
        *self.keymap.borrow_mut() = Keymap::new(&config.keymap);
        self.apply_folders(config.library_folders());
    }

    fn apply_folders(&self, folders: Vec<PathBuf>) {
        let service = self.service.borrow();
        let Some(service) = service.as_ref() else { return };
        if service.folders() == folders {
            return;
        }
        self.library.set_folders(folders.clone());
        self.library.set_scanning(true);
        service.set_folders(folders);
    }

    /// The current folders, for adding to or taking from.
    fn folders(&self) -> Vec<PathBuf> {
        self.service.borrow().as_ref().map(|s| s.folders()).unwrap_or_default()
    }

    /// Write the folders to config.toml and read them from now on.
    fn save_folders(&self, folders: Vec<PathBuf>) -> bool {
        if let Err(e) = config::save_library_folders(&Config::path(), &folders) {
            self.show_toast(&format!("Couldn't save the folders to config.toml: {e}"), true);
            return false;
        }
        self.config_stamp.set(Config::fingerprint());
        self.apply_folders(folders);
        true
    }

    /// Pick a folder (through the desktop's portal) and add it.
    fn add_folder(&self) {
        let dialog = gtk::FileDialog::builder().title("Add a folder of books").modal(true).build();
        if let Some(home) = std::env::var_os("HOME") {
            dialog.set_initial_folder(Some(&gio::File::for_path(home)));
        }
        let win = self.me.borrow().clone();
        dialog.select_folder(Some(&self.window), gio::Cancellable::NONE, move |result| {
            let Some(win) = win.upgrade() else { return };
            match result.ok().and_then(|f| f.path()) {
                Some(path) => win.add_folder_path(path),
                // Cancelled, or a place with no local path (a remote share).
                None => win.library.focus(),
            }
        });
    }

    fn add_folder_path(&self, path: PathBuf) {
        match library::add_folder(&self.folders(), &path) {
            library::AddFolder::Within(root) if root == path => {
                self.show_toast(&format!("{} is already in the library", config::contract(&path)), false);
            }
            library::AddFolder::Within(root) => {
                self.show_toast(&format!("Already in the library, inside {}", config::contract(&root)), false);
            }
            library::AddFolder::Folders(folders) => {
                if !self.save_folders(folders) {
                    return;
                }
                self.show_toast(&format!("Added {}", config::contract(&path)), false);
            }
        }
        self.library.show_folder(&path);
        self.library.focus();
    }

    /// Stop reading a folder. Its files stay where they are, and its
    /// positions and highlights are kept for when it comes back.
    fn remove_folder(&self, path: &Path) {
        let folders: Vec<PathBuf> = self.folders().into_iter().filter(|f| f != path).collect();
        if self.save_folders(folders) {
            self.show_toast(&format!("Removed {} from the library; the files are untouched", config::contract(path)), false);
        }
        self.library.focus();
    }

    pub fn handle_key(&self, key: gtk::gdk::Key, mods: gtk::gdk::ModifierType) -> glib::Propagation {
        let context = self.context();
        eprintln!("omaread: DEBUG key={key:?} context={context:?}");

        // Writing a note: Esc cancels, Ctrl+Enter or Ctrl+S saves.
        if self.note_editor.is_open() {
            let ctrl = mods.contains(gtk::gdk::ModifierType::CONTROL_MASK);
            match key {
                gtk::gdk::Key::Escape => self.cancel_note(),
                gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter if ctrl => self.save_note(),
                gtk::gdk::Key::s if ctrl => self.save_note(),
                _ => return glib::Propagation::Proceed,
            }
            return glib::Propagation::Stop;
        }

        // The card's note field takes text; Esc closes the card.
        if self.selection_bar.note_has_focus() {
            return match key {
                gtk::gdk::Key::Escape => {
                    self.close_selection();
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            };
        }

        // The palette: Esc closes, arrows (and ctrl+n/p) choose, Enter runs.
        if self.palette.has_focus() {
            let ctrl = mods.contains(gtk::gdk::ModifierType::CONTROL_MASK);
            match key {
                gtk::gdk::Key::Escape => self.close_palette(),
                gtk::gdk::Key::Down => self.palette.move_cursor(1),
                gtk::gdk::Key::Up => self.palette.move_cursor(-1),
                gtk::gdk::Key::n if ctrl => self.palette.move_cursor(1),
                gtk::gdk::Key::p if ctrl => self.palette.move_cursor(-1),
                _ => return glib::Propagation::Proceed,
            }
            return glib::Propagation::Stop;
        }

        // Typing a search: Esc closes, Enter searches, Alt toggles options.
        if self.search.is_open() && self.search.entry_has_focus() {
            let alt = mods.contains(gtk::gdk::ModifierType::ALT_MASK);
            match key {
                gtk::gdk::Key::Escape => {
                    self.search.close();
                    self.reader.view.grab_focus();
                }
                gtk::gdk::Key::Down | gtk::gdk::Key::Tab => self.search.focus_results(),
                _ if alt => match key.to_unicode() {
                    Some(c @ ('c' | 'w' | 'd')) => {
                        self.search.toggle(c);
                        self.start_search();
                    }
                    _ => return glib::Propagation::Proceed,
                },
                _ => return glib::Propagation::Proceed,
            }
            return glib::Propagation::Stop;
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
            // While reading (or over the help), the page never acts on keys itself.
            None if matches!(context, Context::Reader | Context::Help) => glib::Propagation::Stop,
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
            (Context::Reader | Context::Library, Action::Help) => self.open_help(),
            (Context::Help, Action::Move(_, dy)) => self.help.scroll(dy),
            (Context::Help, Action::First) => self.help.top(),
            (Context::Help, Action::Last) => self.help.bottom(),
            (Context::Help, Action::Close) => self.close_help(),
            (Context::Reader | Context::Library, Action::CommandLine) => self.palette.open(context),
            (Context::Reader, Action::Search) => self.open_search(),
            (Context::Reader | Context::Search, Action::NextHit) => self.go_to_hit(self.search.step(1)),
            (Context::Reader | Context::Search, Action::PrevHit) => self.go_to_hit(self.search.step(-1)),
            (Context::Search, Action::Move(_, dy)) => self.search.move_cursor(dy),
            (Context::Search, Action::Open) => {
                let hit = self.search.selected();
                self.search.close();
                self.reader.view.grab_focus();
                self.go_to_hit(hit);
            }
            (Context::Search, Action::Search) => self.search.open(),
            (Context::Search, Action::Close) => {
                self.search.close();
                self.reader.view.grab_focus();
            }
            (Context::Toc, Action::Move(0, dy)) => self.toc.move_cursor(dy),
            (Context::Toc, Action::Move(dx, _)) => self.toc.fold(dx > 0),
            (Context::Toc, Action::First) => self.toc.move_cursor(i32::MIN / 2),
            (Context::Toc, Action::Last) => self.toc.move_cursor(i32::MAX / 2),
            (Context::Toc, Action::Open) => self.toc.activate(),
            (Context::Reader, Action::Settings) => self.open_settings(),
            (Context::Reader, Action::Bigger) => self.tweak_prefs(|p| p.font_size += 1),
            (Context::Reader, Action::Smaller) => self.tweak_prefs(|p| p.font_size = p.font_size.saturating_sub(1)),
            (Context::Reader, Action::CycleTheme) => self.tweak_prefs(|p| {
                p.theme = match p.theme.as_deref() {
                    None => Some("light".into()),
                    Some("light") => Some("dark".into()),
                    Some("dark") => Some("sepia".into()),
                    _ => None,
                }
            }),
            (Context::Reader, Action::ToggleBookmark) => self.toggle_bookmark(),
            (Context::Reader, Action::Highlights) => self.annotations_panel.open(Kind::Highlight, &self.annotations.borrow()),
            (Context::Reader, Action::Bookmarks) => self.annotations_panel.open(Kind::Bookmark, &self.annotations.borrow()),
            (Context::Selection, Action::Highlight) => self.toggle_highlight(),
            (Context::Selection, Action::Color(i)) => self.color(Some(i)),
            (Context::Selection, Action::Note) => {
                // Clone out of the RefCell before open_note() borrows it again:
                // a `self.focus.borrow()` scrutinee would still be alive here
                // and open_note()'s `focus.take()` would panic.
                let target = self.focus.borrow().clone();
                if let Some(target) = target {
                    self.open_note(target);
                }
            }
            (Context::Selection, Action::Copy) => self.copy_focus(),
            (Context::Selection, Action::Delete) => self.delete_focus(),
            (Context::Selection, Action::Close) => self.close_selection(),
            (Context::Selection, Action::SpeakFrom) => self.toggle_speech(true),
            (Context::Reader, Action::Speak) => self.toggle_speech(false),
            (Context::Reader, Action::SentenceNext) => self.step_sentence(1),
            (Context::Reader, Action::SentencePrev) => self.step_sentence(-1),
            (Context::Reader, Action::Faster) => self.change_rate(10),
            (Context::Reader, Action::Slower) => self.change_rate(-10),
            (Context::Reader, Action::NextVoice) => self.next_voice(),
            (Context::Reader, Action::Voices) => self.open_voice(),
            (Context::Voice, Action::Move(_, dy)) => self.voice.move_cursor(dy),
            (Context::Voice, Action::Open) => self.pick_voice(),
            (Context::Voice, Action::NextVoice) => self.next_voice(),
            (Context::Voice, Action::Faster) => self.change_rate(10),
            (Context::Voice, Action::Slower) => self.change_rate(-10),
            (Context::Voice, Action::Speak) => self.toggle_speech(false),
            (Context::Voice, Action::SentenceNext) => self.step_sentence(1),
            (Context::Voice, Action::SentencePrev) => self.step_sentence(-1),
            (Context::Voice, Action::Close) => {
                self.voice.close();
                self.update_tts_bar();
                self.reader.view.grab_focus();
            }
            (Context::Selection | Context::Lookup, Action::Lookup(key)) => self.look_up(key),
            (Context::Lookup, Action::Move(_, dy)) => self.lookup.scroll(dy),
            (Context::Lookup, Action::OpenLink) => {
                if let Some(url) = self.lookup.result().and_then(|r| r.url) {
                    self.open_link(&url);
                }
            }
            (Context::Lookup, Action::Copy) => {
                if let Some(r) = self.lookup.result() {
                    self.window.clipboard().set_text(&r.body);
                    self.show_toast("Copied", false);
                }
            }
            (Context::Lookup, Action::Close) => {
                self.lookup.close();
                self.reader.call("clearSelection", "");
                self.reader.view.grab_focus();
            }
            (Context::Annotations, Action::Move(_, dy)) => self.annotations_panel.move_cursor(dy),
            (Context::Annotations, Action::Last) => self.annotations_panel.move_cursor(i32::MAX / 2),
            (Context::Annotations, Action::First) => self.annotations_panel.move_cursor(i32::MIN / 2),
            (Context::Annotations, Action::Open) => {
                if let Some(a) = self.annotations_panel.selected() {
                    self.annotations_panel.close();
                    self.reader.call("goTo", &serde_json::to_string(&a.cfi).unwrap_or_default());
                    self.reader.view.grab_focus();
                }
            }
            (Context::Annotations, Action::Note) => {
                if let Some(a) = self.annotations_panel.selected().filter(|a| a.kind == Kind::Highlight) {
                    self.open_note(Focus::Highlight(a.id));
                }
            }
            (Context::Annotations, Action::Delete) => {
                if let Some(a) = self.annotations_panel.selected() {
                    self.remove(a.id);
                }
            }
            (Context::Annotations, Action::Close) => {
                self.annotations_panel.close();
                self.reader.view.grab_focus();
            }
            (Context::Settings, Action::Move(0, dy)) => self.settings.move_cursor(dy),
            (Context::Settings, Action::Move(dx, _)) => self.settings.adjust(dx.signum()),
            (Context::Settings, Action::Close) => {
                self.settings.close();
                self.reader.view.grab_focus();
            }
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
            (Context::Reader, Action::ClearOrClose) if self.speech.borrow().is_some() => self.stop_speech(),
            (Context::Reader, Action::ClearOrClose) if self.search.has_hits() => {
                self.search.clear();
                self.reader.call("clearSearch", "");
            }
            (Context::Reader, Action::ClearOrClose) => self.run(context, Action::Close),
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
            (Context::Library, Action::AddFolder) => self.add_folder(),
            (Context::Library, Action::ToggleFolder) => self.library.toggle_folder(),
            (Context::Library, Action::RemoveFolder) => match self.library.current_folder() {
                Some(path) if self.folders().contains(&path) => self.remove_folder(&path),
                Some(path) => {
                    let root = self.folders().into_iter().find(|r| path.starts_with(r));
                    let root = root.map(|r| config::contract(&r)).unwrap_or_default();
                    self.show_toast(&format!("Only the folders you added can be removed; this one is inside {root}"), false);
                }
                None => self.show_toast("Pick a folder in the sidebar first", false),
            },
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

    #[cfg(debug_assertions)]
    pub fn debug_eval(&self, expr: &str) {
        self.reader.debug_eval(expr);
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
        // A fired one-shot source is gone: the timer clears its own slot, so
        // the next toast never removes a source that no longer exists.
        let win = self.me.borrow().clone();
        let timer = glib::timeout_add_local_once(Duration::from_secs(4), move || {
            if let Some(win) = win.upgrade() {
                win.toast_timer.take();
                win.toast.set_visible(false);
            }
        });
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

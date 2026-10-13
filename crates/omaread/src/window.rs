//! The window: the book fills it, and everything else floats over it.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gio, glib};
use omaread_core::theme::Theme;

use crate::keys::{Action, Keymap};
use crate::reader::{Event, Location, Reader};
use crate::theming::Theming;

pub struct Window {
    pub window: adw::ApplicationWindow,
    reader: Reader,
    empty: gtk::Box,
    status: Status,
    toast: gtk::Label,
    toast_timer: RefCell<Option<glib::SourceId>>,
    theming: Rc<RefCell<Theming>>,
    keymap: RefCell<Keymap>,
}

struct Status {
    bar: gtk::Box,
    section: gtk::Label,
    position: gtk::Label,
}

impl Window {
    pub fn new(app: &adw::Application) -> Rc<Window> {
        let theming = Rc::new(RefCell::new(Theming::load()));

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Omaread")
            .default_width(1100)
            .default_height(820)
            .build();
        window.add_css_class("omaread");

        let overlay = gtk::Overlay::new();
        let empty = empty_state();
        let status = status_line();
        let toast = gtk::Label::builder()
            .halign(gtk::Align::Center)
            .valign(gtk::Align::End)
            .visible(false)
            .wrap(true)
            .css_classes(["toast"])
            .build();

        // The page is created inside the event closure's reach, so build the
        // window first and hand the reader a weak handle back to it.
        let this: Rc<RefCell<Option<Rc<Window>>>> = Rc::default();
        let reader = Reader::new({
            let this = Rc::downgrade(&this);
            move |event| {
                let Some(cell) = this.upgrade() else { return };
                let Some(win) = cell.borrow().clone() else { return };
                win.on_event(event);
            }
        });

        overlay.set_child(Some(&reader.view));
        overlay.add_overlay(&empty);
        overlay.add_overlay(&status.bar);
        overlay.add_overlay(&toast);
        reader.view.set_visible(false);
        window.set_content(Some(&overlay));

        let win = Rc::new(Window {
            window,
            reader,
            empty,
            status,
            toast,
            toast_timer: RefCell::default(),
            theming,
            keymap: RefCell::default(),
        });
        *this.borrow_mut() = Some(win.clone());
        // `this` holds the window for the reader's callbacks; drop it with the window.
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
        win
    }

    pub fn open(&self, file: &gio::File) {
        let Some(path) = file.path() else {
            self.show_toast("Only local files can be opened", true);
            return;
        };
        self.open_path(path);
    }

    fn open_path(&self, path: PathBuf) {
        if let Some(name) = path.file_stem().and_then(|n| n.to_str()) {
            self.window.set_title(Some(name));
        }
        self.reader.open(path, None);
        self.empty.set_visible(false);
        self.reader.view.set_visible(true);
        self.status.bar.set_visible(true);
    }

    fn push_palette(&self, theme: &Theme) {
        let palette = serde_json::to_string(&theme.book_palette()).unwrap_or_default();
        self.reader.call("setPalette", &palette);
    }

    fn on_event(&self, event: Event) {
        match event {
            Event::Ready => self.push_palette(self.theming.borrow().theme()),
            Event::BookReady { metadata, .. } => {
                if !metadata.title.is_empty() {
                    self.window.set_title(Some(&metadata.title));
                }
                self.reader.view.grab_focus();
            }
            Event::BookError { message, stack } => {
                eprintln!("omaread: could not open book: {message}\n{stack}");
                self.show_toast(&format!("Could not open this book: {message}"), true);
            }
            Event::Relocate(location) => self.update_status(&location),
            Event::ExternalLink { href } => {
                let launcher = gtk::UriLauncher::new(&href);
                launcher.launch(Some(&self.window), None::<&gio::Cancellable>, |_| {});
            }
            Event::Selection { .. } => {}
        }
    }

    fn update_status(&self, l: &Location) {
        self.status.section.set_label(l.toc_label.as_deref().unwrap_or(""));
        let percent = (l.fraction * 100.0).round() as u32;
        let position = match (&l.page_label, l.location) {
            (Some(page), _) => format!("p. {page}  ·  {percent}%"),
            (None, Some(loc)) if loc.total > 0 => format!("{} / {}  ·  {percent}%", loc.current + 1, loc.total),
            _ => format!("{percent}%"),
        };
        self.status.position.set_label(&position);
    }

    fn install_keys(self: &Rc<Self>) {
        let keys = gtk::EventControllerKey::new();
        // Capture: the window hears keys before the page does.
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let win = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, mods| {
            let Some(win) = win.upgrade() else { return glib::Propagation::Proceed };
            let action = win.keymap.borrow_mut().press(key, mods);
            match action {
                Some(action) => {
                    win.run(action);
                    glib::Propagation::Stop
                }
                // Swallow plain keys while reading so the page never acts on them.
                None if win.reader.has_book() => glib::Propagation::Stop,
                None => glib::Propagation::Proceed,
            }
        });
        self.window.add_controller(keys);
    }

    fn run(&self, action: Action) {
        let call = |method: &str| self.reader.call(method, "");
        match action {
            Action::NextPage => call("next"),
            Action::PrevPage => call("prev"),
            Action::NextSection => call("nextSection"),
            Action::PrevSection => call("prevSection"),
            Action::First => call("first"),
            Action::Last => call("last"),
            Action::Back => call("back"),
            Action::Forward => call("forward"),
            Action::ToggleStatus => {
                let bar = &self.status.bar;
                bar.set_visible(!bar.is_visible() && self.reader.has_book());
            }
            Action::Close => {
                if self.toast.is_visible() {
                    self.toast.set_visible(false);
                } else {
                    self.window.close();
                }
            }
            Action::Quit => {
                if let Some(app) = self.window.application() {
                    app.quit();
                }
            }
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
        let timer = glib::timeout_add_local_once(Duration::from_secs(5), move || toast.set_visible(false));
        self.toast_timer.replace(Some(timer));
    }
}

fn empty_state() -> gtk::Box {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .css_classes(["empty"])
        .build();
    let label = |text: &str, class: &str| {
        gtk::Label::builder().label(text).xalign(0.0).css_classes([class]).build()
    };
    card.append(&label("Omaread", "display"));
    card.append(&label("A quiet place to read.", "heading"));
    let hint = gtk::Box::builder().spacing(8).css_classes(["gap"]).build();
    hint.append(&label("Open a book:", "caption"));
    hint.append(&label("omaread book.epub", "key"));
    card.append(&hint);
    card.append(&label("The library arrives next.", "caption"));
    card
}

fn status_line() -> Status {
    let bar = gtk::Box::builder()
        .valign(gtk::Align::End)
        .spacing(12)
        .visible(false)
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

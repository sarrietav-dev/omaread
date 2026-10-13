mod assets;
mod keys;
mod library_view;
mod reader;
mod service;
mod theming;
mod window;

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

const APP_ID: &str = "io.github.sarrietav_dev.Omaread";

fn main() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("scan") {
        return scan();
    }

    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();

    // One window per process for now; a second `omaread book.epub` opens the
    // book in it (GApplication forwards the files to the running instance).
    let window: Rc<RefCell<Option<Rc<window::Window>>>> = Rc::default();
    let ensure = {
        let window = window.clone();
        move |app: &adw::Application| -> Rc<window::Window> {
            if let Some(win) = window.borrow().as_ref() {
                return win.clone();
            }
            let win = window::Window::new(app);
            #[cfg(debug_assertions)]
            install_debug_actions(app, &win);
            let slot = window.clone();
            win.window.connect_destroy(move |_| {
                slot.borrow_mut().take();
            });
            *window.borrow_mut() = Some(win.clone());
            win
        }
    };

    app.connect_activate({
        let ensure = ensure.clone();
        move |app| ensure(app).window.present()
    });
    app.connect_open(move |app, files, _| {
        let win = ensure(app);
        if let Some(file) = files.first() {
            win.open(file);
        }
        win.window.present();
    });

    app.run()
}

/// `omaread scan`: refresh the library without opening a window.
fn scan() -> glib::ExitCode {
    let config = omaread_core::config::Config::load();
    let paths = omaread_core::library::Paths::from_env();
    let started = std::time::Instant::now();
    let result = paths
        .open_db()
        .and_then(|mut db| omaread_core::library::scan(&mut db, &config.library_folders(), &paths.covers));
    match result {
        Ok(r) => {
            println!(
                "{} books, {} read, {} removed, {} unreadable ({} ms)",
                r.total,
                r.read,
                r.removed,
                r.failed,
                started.elapsed().as_millis()
            );
            glib::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("omaread: scan failed: {e}");
            glib::ExitCode::FAILURE
        }
    }
}

/// Debug builds only: drive the window over D-Bus without keyboard focus.
///
/// ```sh
/// gdbus call --session --dest io.github.sarrietav_dev.Omaread \
///   --object-path /io/github/sarrietav_dev/Omaread \
///   --method org.gtk.Actions.Activate debug-key "[<'j'>]" "{}"
/// ```
/// Keys are GDK names (`j`, `Return`, `Tab`); `<Shift>` and `<Control>`
/// prefixes work as in accelerators. Text after `type:` goes to the focused
/// entry.
#[cfg(debug_assertions)]
fn install_debug_actions(app: &adw::Application, win: &Rc<window::Window>) {
    let action = gio::SimpleAction::new("debug-key", Some(glib::VariantTy::STRING));
    let win = Rc::downgrade(win);
    action.connect_activate(move |_, param| {
        let (Some(win), Some(spec)) = (win.upgrade(), param.and_then(|p| p.str().map(String::from))) else { return };
        if let Some(text) = spec.strip_prefix("type:") {
            if let Some(focus) = gtk::prelude::GtkWindowExt::focus(&win.window).and_downcast::<gtk::Text>() {
                focus.set_text(&format!("{}{text}", focus.text()));
                focus.set_position(-1);
            }
            return;
        }
        match gtk::accelerator_parse(&spec) {
            Some((key, mods)) => {
                win.handle_key(key, mods);
            }
            None => eprintln!("omaread: debug-key: can't parse {spec:?}"),
        }
    });
    app.add_action(&action);
}

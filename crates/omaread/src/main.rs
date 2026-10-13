mod annotations_view;
mod assets;
mod keys;
mod library_view;
mod reader;
mod service;
mod settings_view;
mod theming;
mod toc_view;
mod window;

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

const APP_ID: &str = "io.github.sarrietav_dev.Omaread";

fn main() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("scan") => return scan(),
        Some("import-foliate") => return import_foliate(),
        _ => {}
    }

    // Debug builds can run beside the real app under another id (for tests).
    let app_id = match std::env::var("OMAREAD_APP_ID") {
        Ok(id) if cfg!(debug_assertions) && gio::Application::id_is_valid(&id) => id,
        _ => APP_ID.to_string(),
    };
    let app = adw::Application::builder()
        .application_id(app_id.as_str())
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

/// `omaread import-foliate`: bring Foliate's positions, highlights and
/// bookmarks over (again). Anything already here is kept.
fn import_foliate() -> glib::ExitCode {
    let paths = omaread_core::library::Paths::from_env();
    let config = omaread_core::config::Config::load();
    let result = paths.open_db().and_then(|mut db| {
        omaread_core::library::scan(&mut db, &config.library_folders(), &paths.covers)?;
        let books = omaread_core::library::load(&db)?;
        let dir = omaread_core::annotations::foliate_dir();
        omaread_core::annotations::import_foliate(&db, &dir, &books, omaread_core::library::now())
    });
    match result {
        Ok(r) => {
            println!(
                "{} books matched ({} not in the library): {} positions, {} highlights, {} bookmarks",
                r.books, r.unmatched, r.positions, r.annotations, r.bookmarks
            );
            glib::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("omaread: import failed: {e}");
            glib::ExitCode::FAILURE
        }
    }
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
    let weak = Rc::downgrade(win);
    action.connect_activate(move |_, param| {
        let win = weak.clone();
        let (Some(win), Some(spec)) = (win.upgrade(), param.and_then(|p| p.str().map(String::from))) else { return };
        if let Some(text) = spec.strip_prefix("type:") {
            let focus = gtk::prelude::GtkWindowExt::focus(&win.window);
            if let Some(field) = focus.clone().and_downcast::<gtk::Text>() {
                field.set_text(&format!("{}{text}", field.text()));
                field.set_position(-1);
            } else if let Some(view) = focus.and_downcast::<gtk::TextView>() {
                view.buffer().insert_at_cursor(text);
            }
            return;
        }
        match gtk::accelerator_parse(&spec) {
            Some((key, mods)) => {
                // Keys the window lets through would go to the focused field;
                // the one that matters for tests is Enter.
                if win.handle_key(key, mods) == glib::Propagation::Proceed
                    && key == gtk::gdk::Key::Return
                    && let Some(entry) = gtk::prelude::GtkWindowExt::focus(&win.window)
                        .and_then(|f| f.ancestor(gtk::Entry::static_type()))
                        .and_downcast::<gtk::Entry>()
                {
                    entry.emit_activate();
                }
            }
            None => eprintln!("omaread: debug-key: can't parse {spec:?}"),
        }
    });
    app.add_action(&action);

    // `debug-js "<expression>"`: evaluate in the page, print the JSON result.
    let js = gio::SimpleAction::new("debug-js", Some(glib::VariantTy::STRING));
    let js_win = win.clone();
    let win = Rc::downgrade(win);
    js.connect_activate(move |_, param| {
        let (Some(win), Some(expr)) = (win.upgrade(), param.and_then(|p| p.str().map(String::from))) else { return };
        win.debug_eval(&expr);
    });
    app.add_action(&js);

    // `debug-snapshot "/path.png"`: render the window to a file, visible or not.
    let snap = gio::SimpleAction::new("debug-snapshot", Some(glib::VariantTy::STRING));
    let win = Rc::downgrade(&js_win);
    snap.connect_activate(move |_, param| {
        let (Some(win), Some(path)) = (win.upgrade(), param.and_then(|p| p.str().map(String::from))) else { return };
        // The content, not the toplevel: a paintable of a window draws nothing.
        let Some(widget) = win.window.content() else { return };
        let (w, h) = (widget.width() as f64, widget.height() as f64);
        let paintable = gtk::WidgetPaintable::new(Some(&widget));
        let snapshot = gtk::Snapshot::new();
        paintable.snapshot(&snapshot, w, h);
        let Some(node) = snapshot.to_node() else { return eprintln!("debug-snapshot: nothing drawn ({w}x{h}, mapped: {})", widget.is_mapped()) };
        let Some(renderer) = widget.native().and_then(|n| n.renderer()) else { return };
        let texture = renderer.render_texture(&node, None);
        match texture.save_to_png(&path) {
            Ok(()) => println!("debug-snapshot: {path}"),
            Err(e) => eprintln!("debug-snapshot: {e}"),
        }
    });
    app.add_action(&snap);
}

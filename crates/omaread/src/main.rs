mod annotations_view;
mod assets;
mod help_view;
mod keys;
mod library_view;
mod lookup_view;
mod palette;
mod reader;
mod search_view;
mod service;
mod settings_view;
mod theming;
mod toc_view;
mod tts;
mod voice_view;
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
        Some("lookup") => return lookup(&args[2..]),
        Some("status") => return status(),
        Some("check-library") => return check_library(),
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

/// `omaread status [--json]`: the books you're reading, latest first. The bar
/// widget reads this.
fn status() -> glib::ExitCode {
    let paths = omaread_core::library::Paths::from_env();
    let books = match paths.open_db().and_then(|db| omaread_core::library::load(&db)) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("omaread: {e}");
            return glib::ExitCode::FAILURE;
        }
    };
    let mut reading: Vec<_> = books.into_iter().filter(|b| b.in_progress()).collect();
    reading.sort_by(|a, b| b.opened_at().cmp(&a.opened_at()));
    let json: Vec<serde_json::Value> = reading
        .iter()
        .take(8)
        .map(|b| {
            let p = b.progress.clone().unwrap_or_default();
            serde_json::json!({
                "title": b.title,
                "authors": b.authors,
                "path": b.path,
                "cover": b.cover,
                "fraction": p.fraction,
                "chapter": p.chapter,
                "minutesLeft": p.minutes_left,
                "openedAt": p.opened_at,
            })
        })
        .collect();
    if std::env::args().any(|a| a == "--json") {
        println!("{}", serde_json::json!({ "reading": json }));
    } else {
        for b in &reading {
            println!("{:>3.0}%  {}", b.fraction() * 100.0, b.title);
        }
    }
    glib::ExitCode::SUCCESS
}

/// `omaread lookup <key> <text…> [--lang xx]`: what the lookup card would show.
fn lookup(args: &[String]) -> glib::ExitCode {
    let (Some(key), rest) = (args.first(), &args[args.len().min(1)..]) else {
        eprintln!("usage: omaread lookup <key> <text…> [--lang xx]");
        return glib::ExitCode::FAILURE;
    };
    let mut lang = None;
    let mut words = Vec::new();
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        if a == "--lang" {
            lang = it.next().cloned();
        } else {
            words.push(a.clone());
        }
    }
    let providers = omaread_core::config::Config::load().lookup.providers();
    let Some(spec) = providers.iter().find(|p| p.key == *key) else {
        let keys: Vec<String> = providers.iter().map(|p| format!("{} ({})", p.key, p.name)).collect();
        eprintln!("no provider on key {key}; there are: {}", keys.join(", "));
        return glib::ExitCode::FAILURE;
    };
    match omaread_core::lookup::run(spec, &words.join(" "), lang.as_deref()) {
        Ok(r) => {
            if !r.title.is_empty() {
                println!("{}\n", r.title);
            }
            println!("{}", r.open.as_deref().unwrap_or(&r.body));
            if let Some(url) = r.url {
                println!("\n{url}");
            }
            glib::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            glib::ExitCode::FAILURE
        }
    }
}

/// The configured folders, after naming the old default for a library
/// built from it (else a first scan with no folders would empty it).
fn library_folders(db: &omaread_core::db::Db) -> Vec<std::path::PathBuf> {
    let mut config = omaread_core::config::Config::load();
    config.adopt_old_default(db.stamps().is_ok_and(|s| !s.is_empty()));
    config.library_folders()
}

/// `omaread import-foliate`: bring Foliate's positions, highlights and
/// bookmarks over (again). Anything already here is kept.
fn import_foliate() -> glib::ExitCode {
    let paths = omaread_core::library::Paths::from_env();
    let result = paths.open_db().and_then(|mut db| {
        let folders = library_folders(&db);
        omaread_core::library::scan(&mut db, &folders, &paths.covers)?;
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
    let paths = omaread_core::library::Paths::from_env();
    let started = std::time::Instant::now();
    let result = paths.open_db().and_then(|mut db| {
        let folders = library_folders(&db);
        omaread_core::library::scan(&mut db, &folders, &paths.covers)
    });
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

/// `omaread check-library [--json]`: open every book in the library and say
/// what's wrong with the ones that won't. The smoke test the plan asks for.
fn check_library() -> glib::ExitCode {
    use omaread_core::{epub, library};

    let paths = library::Paths::from_env();
    let scanned = paths.open_db().and_then(|mut db| {
        let folders = library_folders(&db);
        let report = library::scan(&mut db, &folders, &paths.covers)?;
        Ok((report, library::load(&db)?))
    });
    let (report, books) = match scanned {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("omaread: {e}");
            return glib::ExitCode::FAILURE;
        }
    };

    let mut checked: Vec<serde_json::Value> = Vec::new();
    let mut problems = 0usize;
    for book in &books {
        let (title, chapters, issues) = match &book.error {
            Some(e) => (book.title.clone(), 0, vec![e.clone()]),
            None => match epub::Epub::open(&book.path).and_then(|mut e| e.check()) {
                Ok(check) => (check.title, check.chapters, check.problems),
                Err(e) => (book.title.clone(), 0, vec![e.to_string()]),
            },
        };
        if !issues.is_empty() {
            problems += 1;
            eprintln!("  ✗ {} — {}", title, issues.join("; "));
        }
        checked.push(serde_json::json!({
            "title": title,
            "path": book.path,
            "chapters": chapters,
            "problems": issues,
        }));
    }

    if std::env::args().any(|a| a == "--json") {
        println!("{}", serde_json::json!({ "books": checked, "problems": problems }));
    } else {
        println!("{} books checked ({} files), {} with problems", books.len(), report.total, problems);
    }
    if problems > 0 { glib::ExitCode::FAILURE } else { glib::ExitCode::SUCCESS }
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

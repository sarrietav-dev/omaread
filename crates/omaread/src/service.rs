//! Keeping the library in step with the disk, off the UI thread.
//!
//! Scans run on a worker thread with their own database connection and hand
//! the finished book list back over a channel. A filesystem watch on the
//! library folders asks for a new scan, debounced so copying a folder of
//! books triggers one scan, not hundreds.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use notify::{RecursiveMode, Watcher};
use omaread_core::library::{self, Book, Paths};

pub struct Service {
    paths: Paths,
    folders: Vec<PathBuf>,
    books: async_channel::Sender<Result<Vec<Book>, String>>,
    scanning: Rc<Cell<bool>>,
    /// A scan was asked for while one was running.
    again: Rc<Cell<bool>>,
    _watcher: Option<notify::RecommendedWatcher>,
}

impl Service {
    /// Start watching. `on_books` gets every scan's result on the UI thread.
    pub fn start(paths: Paths, folders: Vec<PathBuf>, on_books: impl Fn(Vec<Book>) + 'static) -> Rc<Service> {
        let (books_tx, books_rx) = async_channel::unbounded::<Result<Vec<Book>, String>>();
        let (changed_tx, changed_rx) = async_channel::unbounded::<()>();

        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            let relevant = event.is_ok_and(|e| {
                !e.kind.is_access()
                    && e.paths.iter().any(|p| {
                        p.extension().is_some_and(|x| x.eq_ignore_ascii_case("epub")) || p.extension().is_none()
                    })
            });
            if relevant {
                let _ = changed_tx.send_blocking(());
            }
        })
        .ok()
        .map(|mut w| {
            for folder in &folders {
                if let Err(e) = w.watch(folder, RecursiveMode::Recursive) {
                    eprintln!("omaread: not watching {}: {e}", folder.display());
                }
            }
            w
        });

        let service = Rc::new(Service {
            paths,
            folders,
            books: books_tx,
            scanning: Rc::default(),
            again: Rc::default(),
            _watcher: watcher,
        });

        glib::spawn_future_local({
            let service = Rc::downgrade(&service);
            async move {
                while let Ok(result) = books_rx.recv().await {
                    match result {
                        Ok(books) => on_books(books),
                        // Keep showing what we had; the next scan may do better.
                        Err(e) => eprintln!("omaread: library scan failed: {e}"),
                    }
                    let Some(service) = service.upgrade() else { break };
                    service.scanning.set(false);
                    if service.again.take() {
                        service.scan();
                    }
                }
            }
        });

        glib::spawn_future_local({
            let service = Rc::downgrade(&service);
            async move {
                while changed_rx.recv().await.is_ok() {
                    // Let a burst of changes settle, then drain it.
                    glib::timeout_future(Duration::from_millis(1500)).await;
                    while changed_rx.try_recv().is_ok() {}
                    match service.upgrade() {
                        Some(service) => service.scan(),
                        None => break,
                    }
                }
            }
        });

        service
    }

    pub fn scan(&self) {
        if self.scanning.replace(true) {
            self.again.set(true);
            return;
        }
        let paths = self.paths.clone();
        let folders = self.folders.clone();
        let tx = self.books.clone();
        std::thread::spawn(move || {
            let result = paths.open_db().and_then(|mut db| {
                library::scan(&mut db, &folders, &paths.covers)?;
                library::load(&db)
            });
            let _ = tx.send_blocking(result.map_err(|e| e.to_string()));
        });
    }
}

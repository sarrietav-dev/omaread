mod assets;
mod keys;
mod reader;
mod theming;
mod window;

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

const APP_ID: &str = "io.github.sarrietav_dev.Omaread";

fn main() -> glib::ExitCode {
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

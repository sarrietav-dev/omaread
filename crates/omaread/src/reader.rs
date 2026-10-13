//! The book page: a WebKit view running foliate-js, and the typed bridge to it.
//!
//! Rust → page: `call("next")` runs `globalThis.omaread.next()`.
//! Page → Rust: every message is one JSON object, parsed into [`Event`].

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use serde::Deserialize;
use webkit::prelude::*;

use crate::assets;
use crate::toc_view::TocItem;

// Fields the window doesn't read yet are kept for the milestones that will
// (library metadata, time left, lookups).

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[allow(dead_code)]
pub enum Event {
    Ready,
    BookReady {
        metadata: Metadata,
        #[serde(rename = "pageTotal")]
        page_total: Option<String>,
        #[serde(default)]
        toc: Vec<TocItem>,
    },
    /// Something the reader should be told, from the page.
    Notice {
        message: String,
    },
    BookError {
        message: String,
        #[serde(default)]
        stack: String,
    },
    Relocate(Location),
    ExternalLink {
        href: String,
    },
    Selection(Selection),
    SelectionCleared,
    /// A highlight was clicked.
    AnnotationClick {
        cfi: String,
        rect: Option<Rect>,
    },
    /// The next block's sentences to read aloud.
    TtsBlock {
        sentences: Vec<Sentence>,
    },
    /// Nothing left to read.
    TtsEnd,
    SearchResults {
        chapter: String,
        items: Vec<crate::search_view::Hit>,
    },
    SearchProgress {
        progress: f64,
    },
    #[allow(dead_code)]
    SearchDone {
        #[serde(default)]
        count: usize,
        error: Option<String>,
    },
    /// Chapter and position for CFIs, answering `locate`.
    Located {
        items: Vec<Located>,
    },
    /// Not from the page: its process died. `gave_up` when it kept dying and
    /// the book was closed rather than reopened again.
    #[serde(skip)]
    PageCrashed { gave_up: bool },
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct Selection {
    pub text: String,
    pub cfi: String,
    pub lang: Option<String>,
    #[serde(default)]
    pub chapter: String,
    #[serde(default)]
    pub fraction: f64,
    pub rect: Option<Rect>,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct Sentence {
    pub mark: Option<String>,
    pub text: String,
}

/// Where something is on the page, in the page's pixels.
#[derive(Debug, Default, Clone, Copy, Deserialize)]
#[allow(dead_code)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct Located {
    pub cfi: String,
    #[serde(default)]
    pub chapter: String,
    #[serde(default)]
    pub fraction: f64,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[allow(dead_code)]
pub struct Metadata {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub authors: Vec<String>,
    pub language: Option<String>,
    pub identifier: Option<String>,
}

/// `null` from the page reads as the default: foliate-js reports a location
/// before a section has finished laying out, with its numbers still null.
fn null_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct Location {
    #[serde(default, deserialize_with = "null_default")]
    pub fraction: f64,
    pub cfi: Option<String>,
    pub location: Option<Span>,
    pub section: Option<Span>,
    pub toc_label: Option<String>,
    pub toc_href: Option<String>,
    /// Bookmarks on the page in view.
    #[serde(default)]
    pub bookmarks: Vec<String>,
    /// Where the page in view starts: what a bookmark points at.
    pub page_start: Option<String>,
    pub page_label: Option<String>,
    /// foliate-js's estimate in minutes at its fixed pace; we rescale it.
    pub time: Option<TimeLeft>,
}

#[derive(Debug, Default, Clone, Copy, Deserialize)]
pub struct TimeLeft {
    #[serde(default, deserialize_with = "null_default")]
    pub section: f64,
    #[serde(default, deserialize_with = "null_default")]
    pub total: f64,
}

#[derive(Debug, Default, Clone, Copy, Deserialize)]
pub struct Span {
    #[serde(default, deserialize_with = "null_default")]
    pub current: u64,
    #[serde(default, deserialize_with = "null_default")]
    pub total: u64,
}

#[derive(Clone)]
pub struct Reader {
    pub view: webkit::WebView,
    state: Rc<RefCell<State>>,
}

#[derive(Default)]
struct State {
    /// The file the page's picker is answered with.
    path: Option<PathBuf>,
    /// True once reader.html has loaded and can take calls.
    ready: bool,
    /// Calls made before the page was ready, replayed when it is.
    queued: Vec<String>,
    /// Where the reader is, so a crashed page comes back to the same place.
    last_cfi: Option<String>,
    /// When the page last crashed, to stop reopening a book that crashes it.
    crashes: Vec<std::time::Instant>,
}

/// More crashes than this within [`CRASH_WINDOW`] and the book is closed.
const MAX_CRASHES: usize = 2;
const CRASH_WINDOW: std::time::Duration = std::time::Duration::from_secs(60);

impl Reader {
    pub fn new(on_event: impl Fn(Event) + 'static) -> Reader {
        let context = webkit::WebContext::new();
        assets::register(&context);

        let content = webkit::UserContentManager::new();
        let settings = webkit::Settings::builder()
            .enable_developer_extras(cfg!(debug_assertions))
            .enable_write_console_messages_to_stdout(cfg!(debug_assertions))
            .enable_back_forward_navigation_gestures(false)
            .enable_html5_local_storage(false)
            .enable_html5_database(false)
            // Smooth wheel/trackpad scrolling in scrolled mode; without it each
            // notch jumps and feels hard next to Foliate.
            .enable_smooth_scrolling(true)
            .serif_font_family("Literata")
            .default_font_family("Literata")
            .default_font_size(18)
            .build();

        let view = webkit::WebView::builder()
            .web_context(&context)
            .user_content_manager(&content)
            .settings(&settings)
            .hexpand(true)
            .vexpand(true)
            .build();
        // Until the page paints, show the window's background, not white.
        view.set_background_color(&gtk::gdk::RGBA::new(0.0, 0.0, 0.0, 0.0));

        let reader = Reader { view, state: Rc::default() };

        let on_event = Rc::new(on_event);
        content.connect_script_message_received(Some("omaread"), {
            let reader = reader.clone();
            let on_event = on_event.clone();
            move |_, value| {
                let json = value.to_str();
                match serde_json::from_str::<Event>(&json) {
                    Ok(Event::Ready) => {
                        reader.page_ready();
                        on_event(Event::Ready);
                    }
                    Ok(Event::Relocate(location)) => {
                        reader.state.borrow_mut().last_cfi = location.cfi.clone();
                        on_event(Event::Relocate(location));
                    }
                    Ok(event) => on_event(event),
                    Err(e) => eprintln!("omaread: unreadable message from page ({e}): {json}"),
                }
            }
        });
        content.register_script_message_handler("omaread", None);

        reader.view.connect_run_file_chooser({
            let state = reader.state.clone();
            move |_, request| {
                match state.borrow().path.as_ref().and_then(|p| p.to_str()) {
                    Some(path) => request.select_files(&[path]),
                    None => request.cancel(),
                }
                true
            }
        });

        // Navigation stays inside the app; links out go to the browser.
        reader.view.connect_decide_policy(|_, decision, kind| {
            if kind != webkit::PolicyDecisionType::NavigationAction {
                return false;
            }
            let Some(nav) = decision.downcast_ref::<webkit::NavigationPolicyDecision>() else {
                return false;
            };
            let uri = nav
                .navigation_action()
                .and_then(|a| a.request())
                .and_then(|r| r.uri())
                .map(|u| u.to_string())
                .unwrap_or_default();
            if uri.starts_with("omaread:") || uri.starts_with("blob:") || uri.starts_with("about:") {
                return false;
            }
            decision.ignore();
            true
        });

        reader.view.connect_context_menu(|_, _, _| true);

        reader.view.connect_web_process_terminated({
            let reader = reader.clone();
            let on_event = on_event.clone();
            move |_, reason| {
                let gave_up = {
                    let mut state = reader.state.borrow_mut();
                    let now = std::time::Instant::now();
                    state.crashes.retain(|t| now.duration_since(*t) < CRASH_WINDOW);
                    state.crashes.push(now);
                    state.crashes.len() > MAX_CRASHES
                };
                eprintln!("omaread: the page process ended ({reason:?}){}", if gave_up { "; giving up on this book" } else { "; reloading" });
                if gave_up {
                    reader.close();
                } else {
                    reader.reload();
                }
                on_event(Event::PageCrashed { gave_up });
            }
        });

        reader.view.load_uri(assets::READER_URI);
        reader
    }

    /// Open a book file. Works before the page has loaded too.
    pub fn open(&self, path: PathBuf, last_location: Option<&str>) {
        self.state.borrow_mut().path = Some(path);
        let opts = serde_json::json!({ "lastLocation": last_location });
        self.call("open", &opts.to_string());
    }

    /// Drop the book and start from a fresh page.
    pub fn close(&self) {
        {
            let mut state = self.state.borrow_mut();
            state.path = None;
            state.last_cfi = None;
            state.ready = false;
            state.queued.clear();
        }
        self.view.load_uri(assets::READER_URI);
    }

    /// `globalThis.omaread.<method>(<json args>)`.
    pub fn call(&self, method: &str, args_json: &str) {
        // `void`: most calls return promises, which evaluate_javascript cannot convert.
        let script = format!("void globalThis.omaread.{method}({args_json})");
        let mut state = self.state.borrow_mut();
        if !state.ready {
            state.queued.push(script);
            return;
        }
        drop(state);
        self.run(&script);
    }

    fn run(&self, script: &str) {
        let script_for_log = script.to_string();
        self.view.evaluate_javascript(script, None, None, None::<&gtk::gio::Cancellable>, move |result| {
            if let Err(e) = result {
                eprintln!("omaread: page call failed: {script_for_log}: {e}");
            }
        });
    }

    #[cfg(debug_assertions)]
    pub fn debug_eval(&self, expr: &str) {
        let script = format!("JSON.stringify({expr})");
        self.view.evaluate_javascript(&script, None, None, None::<&gtk::gio::Cancellable>, |result| match result {
            Ok(value) => println!("debug-js: {}", value.to_str()),
            Err(e) => println!("debug-js error: {e}"),
        });
    }

    fn page_ready(&self) {
        let queued = {
            let mut state = self.state.borrow_mut();
            state.ready = true;
            std::mem::take(&mut state.queued)
        };
        // The palette is pushed on every ready by the window; replay the rest.
        for script in queued {
            self.run(&script);
        }
    }

    fn reload(&self) {
        self.state.borrow_mut().ready = false;
        self.view.load_uri(assets::READER_URI);
        let (path, cfi) = {
            let state = self.state.borrow();
            (state.path.clone(), state.last_cfi.clone())
        };
        if let Some(path) = path {
            let reader = self.clone();
            // Queued until the page is back; the window restores the palette on Ready.
            glib::idle_add_local_once(move || reader.open(path, cfi.as_deref()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_location_before_layout_still_parses() {
        let json = r#"{"type":"relocate","fraction":null,"cfi":"epubcfi(/6/22!/4)","section":{"current":10,"total":45},
            "location":{"current":null,"next":null,"total":1055},"time":{"section":null,"total":null},"bookmarks":[]}"#;
        let Ok(Event::Relocate(l)) = serde_json::from_str::<Event>(json) else { panic!("did not parse") };
        assert_eq!(l.fraction, 0.0);
        assert_eq!(l.location.unwrap().total, 1055);
    }
}

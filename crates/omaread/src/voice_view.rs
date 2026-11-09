//! Reading aloud: the panel that lists the voices, sets the pace and shows
//! what is playing. Opened with `V`, driven with j/k, enter, `<`/`>` and `s`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use omaread_core::voice::Voice;

/// A click in the voice panel, for the window to act on.
#[derive(Debug, Clone)]
pub enum VoiceAction {
    /// Use this voice now.
    Pick(String),
    /// A nudge on the reader's pace: negative is slower.
    Rate(i32),
    /// Play or pause.
    Toggle,
/// A sentence back (negative) or on.
    Sentence(i32),
    Stop,
    Voices,
    CloseVoices,
}

/// What the panel's status line is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceState {
    /// Nothing is being read aloud.
    NotReading,
    Playing,
    Preparing,
    Paused,
}

/// The compact transport stays separate from the voice chooser.
pub struct SpeechPlayer {
    pub root: gtk::Box,
    status: gtk::Label,
    detail: gtk::Label,
    speed: gtk::Label,
    toggle: gtk::Button,
    toggle_icon: gtk::Image,
}

impl SpeechPlayer {
    pub fn new(on_action: impl Fn(VoiceAction) + 'static) -> Rc<Self> {
        let action: Rc<dyn Fn(VoiceAction)> = Rc::new(on_action);
        let status = gtk::Label::builder()
            .label("Preparing speech…")
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["speech-title"])
            .build();
        let detail = gtk::Label::builder()
            .xalign(1.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["speech-detail"])
            .build();
        let info = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).spacing(12).build();
        info.append(&status);
        info.append(&detail);

        let controls = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .column_spacing(6)
            .row_spacing(4)
            .min_children_per_line(3)
            .max_children_per_line(8)
            .halign(gtk::Align::Center)
            .build();
        let icon_button = |icon: &str, tip: &str, event: VoiceAction| {
            let button = gtk::Button::builder().tooltip_text(tip).css_classes(["speech-button"]).build();
            button.set_child(Some(&gtk::Image::from_icon_name(icon)));
            let action = action.clone();
            button.connect_clicked(move |_| action(event.clone()));
            button
        };
        let text_button = |label: &str, tip: &str, event: VoiceAction| {
            let button = gtk::Button::builder().label(label).tooltip_text(tip).css_classes(["speech-button"]).build();
            let action = action.clone();
            button.connect_clicked(move |_| action(event.clone()));
            button
        };
        let back = icon_button("media-skip-backward-symbolic", "Previous sentence ([)", VoiceAction::Sentence(-1));
        let toggle = icon_button("media-playback-pause-symbolic", "Play or pause (s)", VoiceAction::Toggle);
        toggle.add_css_class("speech-primary");
        let next = icon_button("media-skip-forward-symbolic", "Next sentence (])", VoiceAction::Sentence(1));
        let slower = text_button("−", "Slower (<)", VoiceAction::Rate(-10));
        let speed = gtk::Label::builder().label("1.0×").css_classes(["speech-speed"]).build();
        let faster = text_button("+", "Faster (>)", VoiceAction::Rate(10));
        let voices = text_button("Voices", "Choose a voice (V)", VoiceAction::Voices);
        let stop = icon_button("media-playback-stop-symbolic", "Stop reading aloud (Esc)", VoiceAction::Stop);
        for button in [&back, &toggle, &next, &slower] { controls.append(button); }
        controls.append(&speed);
        for button in [&faster, &voices, &stop] { controls.append(button); }

        let panel = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .halign(gtk::Align::Center)
            .css_classes(["speech-panel"])
            .build();
        panel.append(&info);
        panel.append(&controls);
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .halign(gtk::Align::Fill)
            .visible(false)
            .css_classes(["speech-player"])
            .build();
        root.append(&panel);
        let toggle_icon = toggle.child().and_downcast::<gtk::Image>().expect("the transport icon");
        Rc::new(Self { root, status, detail, speed, toggle, toggle_icon })
    }

    pub fn set_state(&self, state: VoiceState, voice: &str, speed: &str, place: Option<(usize, usize)>, error: Option<&str>) {
        let sentence = place.map(|(i, n)| format!(" · sentence {} of {}", i + 1, n)).unwrap_or_default();
        self.status.set_label(error.unwrap_or(&match state {
            VoiceState::NotReading => "Not reading aloud".to_string(),
            VoiceState::Preparing => "Preparing speech…".to_string(),
            VoiceState::Playing => format!("Reading aloud{sentence}"),
            VoiceState::Paused => format!("Paused{sentence}"),
        }));
        let playing = matches!(state, VoiceState::Playing | VoiceState::Preparing);
        if let Some(error) = error {
            let retry = gtk::Label::new(Some("Retry"));
            self.toggle.set_child(Some(&retry));
            self.toggle.set_tooltip_text(Some(error));
        } else {
            self.toggle_icon.set_icon_name(Some(if playing {
                "media-playback-pause-symbolic"
            } else {
                "media-playback-start-symbolic"
            }));
            self.toggle.set_child(Some(&self.toggle_icon));
            self.toggle.set_tooltip_text(Some(if playing { "Pause (s)" } else { "Resume from this sentence (s)" }));
        }
        self.speed.set_label(speed);
        let resume = if state == VoiceState::Paused && error.is_none() { " · resumes this sentence" } else { "" };
        self.detail.set_label(&format!("{voice}{resume}"));
    }
}

pub struct VoicePanel {
    pub root: gtk::Box,
    status: gtk::Label,
    play: gtk::Button,
    speed: gtk::Label,
    rate_bar: gtk::ProgressBar,
    list: gtk::ListBox,
    empty: gtk::Label,
    voices: RefCell<Vec<Voice>>,
    current: RefCell<Option<String>>,
    cursor: Cell<i32>,
    on_action: Rc<dyn Fn(VoiceAction)>,
}

impl VoicePanel {
    pub fn new(on_action: impl Fn(VoiceAction) + 'static) -> Rc<VoicePanel> {
        let on_action: Rc<dyn Fn(VoiceAction)> = Rc::new(on_action);

        let title = gtk::Label::builder().label("Voices").xalign(0.0).hexpand(true).css_classes(["title"]).build();
        let close = gtk::Button::builder().label("×").tooltip_text("Close voices (Esc)").build();
        close.connect_clicked({
            let on_action = on_action.clone();
            move |_| on_action(VoiceAction::CloseVoices)
        });
        let play = gtk::Button::builder().label("Read aloud").tooltip_text("Start, pause or resume (s)").build();
        play.connect_clicked({
            let on_action = on_action.clone();
            move |_| on_action(VoiceAction::Toggle)
        });
        let head = gtk::Box::builder().spacing(8).build();
        head.append(&title);
        head.append(&play);
        head.append(&close);
        let status = gtk::Label::builder().xalign(0.0).css_classes(["voice-status"]).build();

        let slower = gtk::Button::builder().label("−").tooltip_text("slower ( < )").build();
        let faster = gtk::Button::builder().label("+").tooltip_text("faster ( > )").build();
        let speed = gtk::Label::builder().xalign(1.0).css_classes(["value"]).build();
        let speed_name = gtk::Label::builder().label("Speed").xalign(0.0).hexpand(true).css_classes(["name"]).build();
        let speed_head = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).spacing(6).css_classes(["voice-speed"]).build();
        speed_head.append(&speed_name);
        speed_head.append(&slower);
        speed_head.append(&speed);
        speed_head.append(&faster);
        let rate_bar = gtk::ProgressBar::builder().show_text(false).valign(gtk::Align::Center).css_classes(["range"]).build();
        slower.connect_clicked({
            let on_action = on_action.clone();
            move |_| on_action(VoiceAction::Rate(-10))
        });
        faster.connect_clicked({
            let on_action = on_action.clone();
            move |_| on_action(VoiceAction::Rate(10))
        });

        let heading = gtk::Label::builder().label("Available voices").xalign(0.0).css_classes(["voice-section"]).build();
        let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::Single).css_classes(["voice-list"]).build();
        let scroller = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(300)
            .build();
        let empty = gtk::Label::builder()
            .label("Loading voices…")
            .xalign(0.0)
            .wrap(true)
            .css_classes(["caption"])
            .build();
        let hint = gtk::Label::builder()
            .label("j/k voice   enter use   [ ] sentence   s play/pause   V next   esc close")
            .xalign(0.0)
            .wrap(true)
            .css_classes(["caption"])
            .build();

        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .halign(gtk::Align::End)
            .valign(gtk::Align::Start)
            .visible(false)
            .css_classes(["card", "side-panel", "voice-panel"])
            .build();
        root.append(&head);
        root.append(&status);
        root.append(&speed_head);
        root.append(&rate_bar);
        root.append(&heading);
        root.append(&empty);
        root.append(&scroller);
        root.append(&hint);

        Rc::new(VoicePanel {
            root,
            status,
            play,
            speed,
            rate_bar,
            list,
            empty,
            voices: RefCell::default(),
            current: RefCell::default(),
            cursor: Cell::new(0),
            on_action,
        })
    }

    pub fn is_open(&self) -> bool {
        self.root.is_visible()
    }

    pub fn open(&self) {
        self.root.set_visible(true);
        if let Some(i) = self.active_index() {
            self.select(i);
        } else if !self.voices.borrow().is_empty() {
            self.select(0);
        }
    }

    pub fn close(&self) {
        self.root.set_visible(false);
    }

    /// Replace the list when the voices or the current one changed.
    pub fn set_voices(&self, voices: &[Voice], current: Option<&str>) {
        if voices.is_empty() {
            self.empty.set_label("No voices available. Check your speech engine and try again.");
        }
        if *self.voices.borrow() == voices && self.current.borrow().as_deref() == current {
            return;
        }
        self.voices.replace(voices.to_vec());
        self.current.replace(current.map(str::to_string));
        while let Some(row) = self.list.row_at_index(0) {
            self.list.remove(&row);
        }
        for voice in voices {
            let active = current == Some(voice.name.as_str());
            self.list.append(&row_for(voice, active, &self.on_action));
        }
        self.empty.set_visible(voices.is_empty());
        let n = voices.len() as i32;
        if n > 0 {
            // Follow the voice in use; otherwise keep where the cursor was.
            let target = self.active_index().unwrap_or_else(|| self.cursor.get().clamp(0, n - 1));
            self.select(target);
        }
    }

    /// What is playing, and how fast. `rate` is the reader's -100…100.
    pub fn set_state(&self, state: VoiceState, sentence: Option<(usize, usize)>, rate: i32, speed_text: &str) {
        let place = sentence.map(|(i, n)| format!(" · sentence {} of {}", i + 1, n)).unwrap_or_default();
        let (text, class) = match state {
            VoiceState::NotReading => ("Not reading aloud".to_string(), "paused"),
            VoiceState::Playing => (format!("Reading aloud{place}"), "playing"),
            VoiceState::Preparing => ("Preparing speech…".to_string(), "paused"),
            VoiceState::Paused => (format!("Paused{place}"), "paused"),
        };
        self.status.set_label(&text);
        self.play.set_label(match state {
            VoiceState::Playing | VoiceState::Preparing => "Pause",
            VoiceState::Paused => "Resume",
            VoiceState::NotReading => "Read aloud",
        });
        self.status.remove_css_class("playing");
        self.status.remove_css_class("paused");
        self.status.add_css_class(class);
        self.speed.set_label(speed_text);
        self.rate_bar.set_fraction(((rate + 100) as f64 / 200.0).clamp(0.0, 1.0));
    }

    pub fn move_cursor(&self, delta: i32) {
        let n = self.voices.borrow().len() as i32;
        if n > 0 {
            self.select((self.cursor.get() + delta).clamp(0, n - 1));
        }
    }

    pub fn selected(&self) -> Option<Voice> {
        self.voices.borrow().get(self.cursor.get() as usize).cloned()
    }

    fn select(&self, i: i32) {
        self.cursor.set(i);
        if let Some(row) = self.list.row_at_index(i) {
            self.list.select_row(Some(&row));
            row.grab_focus();
        }
    }

    /// Where the voice in use sits in the list, if it's among them.
    fn active_index(&self) -> Option<i32> {
        let current = self.current.borrow().clone()?;
        self.voices.borrow().iter().position(|v| v.name == current).map(|i| i as i32)
    }
}

fn row_for(voice: &Voice, active: bool, on_action: &Rc<dyn Fn(VoiceAction)>) -> gtk::ListBoxRow {
    let marker = gtk::Label::builder().label(if active { "●" } else { " " }).css_classes(["voice-marker"]).build();
    let name = gtk::Label::builder()
        .label(voice.label())
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["voice-name"])
        .build();
    let lang = gtk::Label::builder().label(annotation(voice)).css_classes(["voice-lang"]).build();
    let body = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).spacing(8).build();
    body.append(&marker);
    body.append(&name);
    body.append(&lang);

    let row = gtk::ListBoxRow::builder().child(&body).build();
    if active {
        row.add_css_class("active");
    }
    let pick = voice.name.clone();
    let on_action = on_action.clone();
    let click = gtk::GestureClick::new();
    click.connect_released(move |_, _, _, _| on_action(VoiceAction::Pick(pick.clone())));
    row.add_controller(click);
    row
}

/// A short note beside a voice: its language, and its variant when it has one.
fn annotation(voice: &Voice) -> String {
    let variant = voice.variant.trim();
    match (voice.language.trim(), variant.is_empty()) {
        ("", true) => String::new(),
        ("", false) => variant.to_lowercase(),
        (lang, true) => lang.to_string(),
        (lang, false) => format!("{lang} · {}", variant.to_lowercase()),
    }
}

#[cfg(test)]
fn rate_label(rate: i32) -> String {
    if rate == 0 { "normal".into() } else { format!("{rate:+}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_reads_as_a_nudge() {
        assert_eq!(rate_label(0), "normal");
        assert_eq!(rate_label(20), "+20");
        assert_eq!(rate_label(-10), "-10");
    }

    #[test]
    fn voices_are_annotated_with_language_and_variant() {
        let v = Voice { name: "x".into(), language: "en".into(), variant: "MALE1".into() };
        assert_eq!(annotation(&v), "en · male1");
        let v = Voice { name: "x".into(), language: String::new(), variant: String::new() };
        assert_eq!(annotation(&v), "");
    }
}

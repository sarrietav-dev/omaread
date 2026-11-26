//! Reading aloud as an MPRIS player, so media keys, `playerctl` and the
//! Omarchy bar can pause it, skip sentences and show the book.
//!
//! The player exists only while a book is being read aloud: it takes its
//! bus name when reading starts and gives it up when reading stops, so an
//! idle reader never shows up as a stopped player.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::gio;
use gtk::glib::prelude::*;
use gtk::glib::{self, Variant};

const PATH: &str = "/org/mpris/MediaPlayer2";
const ROOT: &str = "org.mpris.MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const TRACK: &str = "/io/github/sarrietav_dev/Omaread/track";

const XML: &str = r#"<node>
  <interface name="org.mpris.MediaPlayer2">
    <method name="Raise"/>
    <method name="Quit"/>
    <property name="CanQuit" type="b" access="read"/>
    <property name="CanRaise" type="b" access="read"/>
    <property name="HasTrackList" type="b" access="read"/>
    <property name="Identity" type="s" access="read"/>
    <property name="DesktopEntry" type="s" access="read"/>
    <property name="SupportedUriSchemes" type="as" access="read"/>
    <property name="SupportedMimeTypes" type="as" access="read"/>
  </interface>
  <interface name="org.mpris.MediaPlayer2.Player">
    <method name="Next"/>
    <method name="Previous"/>
    <method name="Pause"/>
    <method name="PlayPause"/>
    <method name="Stop"/>
    <method name="Play"/>
    <method name="Seek"><arg name="Offset" type="x" direction="in"/></method>
    <method name="SetPosition"><arg name="TrackId" type="o" direction="in"/><arg name="Position" type="x" direction="in"/></method>
    <method name="OpenUri"><arg name="Uri" type="s" direction="in"/></method>
    <signal name="Seeked"><arg name="Position" type="x"/></signal>
    <property name="PlaybackStatus" type="s" access="read"/>
    <property name="Rate" type="d" access="read"/>
    <property name="Metadata" type="a{sv}" access="read"/>
    <property name="Volume" type="d" access="read"/>
    <property name="Position" type="x" access="read"/>
    <property name="MinimumRate" type="d" access="read"/>
    <property name="MaximumRate" type="d" access="read"/>
    <property name="CanGoNext" type="b" access="read"/>
    <property name="CanGoPrevious" type="b" access="read"/>
    <property name="CanPlay" type="b" access="read"/>
    <property name="CanPause" type="b" access="read"/>
    <property name="CanSeek" type="b" access="read"/>
    <property name="CanControl" type="b" access="read"/>
  </interface>
</node>"#;

/// What a media key or `playerctl` asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    PlayPause,
    Play,
    Pause,
    Stop,
    /// The next sentence.
    Next,
    /// The sentence before.
    Previous,
    Raise,
}

impl Command {
    fn parse(method: &str) -> Option<Command> {
        Some(match method {
            "PlayPause" => Command::PlayPause,
            "Play" => Command::Play,
            "Pause" => Command::Pause,
            "Stop" => Command::Stop,
            "Next" => Command::Next,
            "Previous" => Command::Previous,
            "Raise" => Command::Raise,
            _ => return None,
        })
    }
}

/// What the player shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub playing: bool,
    pub book: String,
    pub authors: Vec<String>,
    /// The chapter being read, as the track's title.
    pub chapter: String,
    pub cover: Option<PathBuf>,
}

impl State {
    fn status(&self) -> &'static str {
        if self.playing { "Playing" } else { "Paused" }
    }

    fn metadata(&self) -> Variant {
        let mut map: HashMap<&str, Variant> = HashMap::new();
        if let Ok(path) = glib::variant::ObjectPath::try_from(TRACK.to_string()) {
            map.insert("mpris:trackid", path.to_variant());
        }
        let title = if self.chapter.is_empty() { &self.book } else { &self.chapter };
        map.insert("xesam:title", title.to_variant());
        map.insert("xesam:album", self.book.to_variant());
        map.insert("xesam:artist", self.authors.to_variant());
        if let Some(cover) = &self.cover
            && let Ok(uri) = glib::filename_to_uri(cover, None)
        {
            map.insert("mpris:artUrl", uri.to_string().to_variant());
        }
        map.to_variant()
    }

    fn property(&self, interface: &str, name: &str) -> Variant {
        match (interface, name) {
            (ROOT, "CanQuit") => false.to_variant(),
            (ROOT, "CanRaise") => true.to_variant(),
            (ROOT, "HasTrackList") => false.to_variant(),
            (ROOT, "Identity") => "Omaread".to_variant(),
            (ROOT, "DesktopEntry") => "io.github.sarrietav_dev.Omaread".to_variant(),
            (ROOT, "SupportedUriSchemes" | "SupportedMimeTypes") => Vec::<String>::new().to_variant(),
            (PLAYER, "PlaybackStatus") => self.status().to_variant(),
            (PLAYER, "Metadata") => self.metadata(),
            (PLAYER, "Rate" | "Volume" | "MinimumRate" | "MaximumRate") => 1.0f64.to_variant(),
            (PLAYER, "Position") => 0i64.to_variant(),
            (PLAYER, "CanSeek") => false.to_variant(),
            (PLAYER, "CanGoNext" | "CanGoPrevious" | "CanPlay" | "CanPause" | "CanControl") => true.to_variant(),
            _ => false.to_variant(),
        }
    }
}

struct Shared {
    state: State,
    connection: Option<gio::DBusConnection>,
    registrations: Vec<gio::RegistrationId>,
}

pub struct Mpris {
    owner: Option<gio::OwnerId>,
    shared: Rc<RefCell<Shared>>,
}

impl Mpris {
    /// Take the bus name and answer as a player. Commands arrive on the
    /// main loop.
    pub fn start(state: State, on_command: impl Fn(Command) + 'static) -> Mpris {
        let shared = Rc::new(RefCell::new(Shared { state, connection: None, registrations: Vec::new() }));
        let on_command: Rc<dyn Fn(Command)> = Rc::new(on_command);
        let name = bus_name();
        let owner = gio::bus_own_name(
            gio::BusType::Session,
            &name,
            gio::BusNameOwnerFlags::DO_NOT_QUEUE,
            {
                let shared = shared.clone();
                move |connection, _| register(&connection, &shared, &on_command)
            },
            |_, _| {},
            |_, name| eprintln!("omaread: couldn't take {name} for media controls"),
        );
        Mpris { owner: Some(owner), shared }
    }

    /// Show what's playing now; tells listeners what changed.
    pub fn update(&self, state: State) {
        let (connection, old) = {
            let mut shared = self.shared.borrow_mut();
            if shared.state == state {
                return;
            }
            let old = std::mem::replace(&mut shared.state, state.clone());
            (shared.connection.clone(), old)
        };
        let Some(connection) = connection else { return };
        let mut changed: HashMap<&str, Variant> = HashMap::new();
        if old.playing != state.playing {
            changed.insert("PlaybackStatus", state.status().to_variant());
        }
        if old.book != state.book || old.chapter != state.chapter || old.authors != state.authors || old.cover != state.cover {
            changed.insert("Metadata", state.metadata());
        }
        if changed.is_empty() {
            return;
        }
        let args = (PLAYER, changed, Vec::<String>::new()).to_variant();
        let _ = connection.emit_signal(None, PATH, "org.freedesktop.DBus.Properties", "PropertiesChanged", Some(&args));
    }
}

impl Drop for Mpris {
    fn drop(&mut self) {
        let mut shared = self.shared.borrow_mut();
        if let Some(connection) = shared.connection.take() {
            for id in shared.registrations.drain(..) {
                let _ = connection.unregister_object(id);
            }
        }
        if let Some(owner) = self.owner.take() {
            gio::bus_unown_name(owner);
        }
    }
}

/// `org.mpris.MediaPlayer2.omaread`, with the instance's pid for a second
/// copy (a debug build beside the installed app).
fn bus_name() -> String {
    match std::env::var("OMAREAD_APP_ID") {
        Ok(_) if cfg!(debug_assertions) => format!("org.mpris.MediaPlayer2.omaread.instance{}", std::process::id()),
        _ => "org.mpris.MediaPlayer2.omaread".into(),
    }
}

fn register(connection: &gio::DBusConnection, shared: &Rc<RefCell<Shared>>, on_command: &Rc<dyn Fn(Command)>) {
    let Ok(node) = gio::DBusNodeInfo::for_xml(XML) else { return };
    let mut ids = Vec::new();
    for interface in [ROOT, PLAYER] {
        let Some(info) = node.lookup_interface(interface) else { continue };
        let registration = connection
            .register_object(PATH, &info)
            .method_call({
                let on_command = on_command.clone();
                move |_, _, _, _, method, _, invocation| {
                    if let Some(command) = Command::parse(method) {
                        on_command(command);
                    }
                    // Seek, SetPosition, OpenUri and Quit do nothing here.
                    invocation.return_value(None);
                }
            })
            .property({
                let shared = Rc::downgrade(shared);
                move |_, _, _, interface, name| match shared.upgrade() {
                    Some(shared) => shared.borrow().state.property(interface, name),
                    None => false.to_variant(),
                }
            })
            .build();
        match registration {
            Ok(id) => ids.push(id),
            Err(e) => eprintln!("omaread: media controls: {e}"),
        }
    }
    let mut shared = shared.borrow_mut();
    shared.connection = Some(connection.clone());
    shared.registrations = ids;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_interface_description_parses() {
        let node = gio::DBusNodeInfo::for_xml(XML).unwrap();
        assert!(node.lookup_interface(ROOT).is_some());
        assert!(node.lookup_interface(PLAYER).is_some());
    }

    #[test]
    fn the_player_reports_the_book_and_whether_it_plays() {
        let state = State { playing: true, book: "Dune".into(), authors: vec!["Frank Herbert".into()], chapter: "Book One".into(), cover: None };
        assert_eq!(state.property(PLAYER, "PlaybackStatus").str(), Some("Playing"));
        let meta = state.metadata();
        let dict = glib::VariantDict::new(Some(&meta));
        assert_eq!(dict.lookup::<String>("xesam:title").unwrap().as_deref(), Some("Book One"));
        assert_eq!(dict.lookup::<String>("xesam:album").unwrap().as_deref(), Some("Dune"));
        assert_eq!(dict.lookup::<Vec<String>>("xesam:artist").unwrap(), Some(vec!["Frank Herbert".to_string()]));
        assert_eq!(Command::parse("PlayPause"), Some(Command::PlayPause));
        assert_eq!(Command::parse("Seek"), None);
    }
}

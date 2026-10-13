//! Text-to-speech through speech-dispatcher, on a worker thread.
//!
//! The book is spoken a sentence at a time: each sentence is its own SSIP
//! message, and its END event moves the reading on. That works with any
//! speech-dispatcher module, including generic ones (piper) that don't send
//! index marks.

use std::collections::HashMap;
use std::sync::mpsc;

use omaread_core::ssip::{self, Voice};

pub enum Command {
    /// Speak a sentence; `seq` comes back in its events.
    Speak { seq: u64, text: String },
    Stop,
    Rate(i32),
    Voice(String),
    Language(String),
    ListVoices,
    /// From the SSIP reader thread.
    Daemon(ssip::Event),
}

#[derive(Debug)]
pub enum Event {
    Ended(u64),
    Voices(Vec<Voice>),
    Error(String),
}

#[derive(Clone)]
pub struct Tts {
    tx: mpsc::Sender<Command>,
}

impl Tts {
    pub fn start(events: async_channel::Sender<Event>) -> Tts {
        let (tx, rx) = mpsc::channel::<Command>();
        let worker_tx = tx.clone();
        std::thread::Builder::new()
            .name("tts".into())
            .spawn(move || worker(rx, worker_tx, events))
            .expect("a thread for speech");
        Tts { tx }
    }

    pub fn send(&self, command: Command) {
        let _ = self.tx.send(command);
    }
}

fn worker(rx: mpsc::Receiver<Command>, tx: mpsc::Sender<Command>, events: async_channel::Sender<Event>) {
    let mut client: Option<ssip::Client> = None;
    // Settings to restore on a new connection.
    let mut rate = 0;
    let mut voice: Option<String> = None;
    let mut language: Option<String> = None;
    // SSIP message id → our sentence number.
    let mut sentences: HashMap<u64, u64> = HashMap::new();

    let error = |e: String| {
        let _ = events.send_blocking(Event::Error(e));
    };

    while let Ok(command) = rx.recv() {
        if let Command::Daemon(event) = command {
            match event {
                ssip::Event::End(id) => {
                    if let Some(seq) = sentences.remove(&id) {
                        let _ = events.send_blocking(Event::Ended(seq));
                    }
                }
                ssip::Event::Canceled(id) => {
                    sentences.remove(&id);
                }
                ssip::Event::Disconnected => {
                    client = None;
                    sentences.clear();
                }
                _ => {}
            }
            continue;
        }

        if client.is_none() {
            let to_worker = tx.clone();
            match ssip::Client::connect("reader", move |e| {
                let _ = to_worker.send(Command::Daemon(e));
            }) {
                Ok(mut c) => {
                    // Tests run muted.
                    if cfg!(debug_assertions) && std::env::var_os("OMAREAD_TTS_MUTE").is_some() {
                        let _ = c.command("SET self VOLUME -100");
                    }
                    let _ = c.set_rate(rate);
                    if let Some(l) = &language {
                        let _ = c.set_language(l);
                    }
                    if let Some(v) = &voice {
                        let _ = c.set_voice(v);
                    }
                    client = Some(c);
                }
                Err(e) => {
                    error(format!("Speech isn't available: {e}"));
                    continue;
                }
            }
        }
        let Some(c) = client.as_mut() else { continue };
        let result = match command {
            Command::Speak { seq, text } => c.speak(&text).map(|id| {
                sentences.insert(id, seq);
            }),
            Command::Stop => {
                sentences.clear();
                c.stop()
            }
            Command::Rate(r) => {
                rate = r;
                c.set_rate(r)
            }
            Command::Voice(v) => {
                let r = c.set_voice(&v);
                voice = Some(v);
                r
            }
            Command::Language(l) => {
                let r = c.set_language(&l);
                language = Some(l);
                r
            }
            Command::ListVoices => c.voices().map(|v| {
                let _ = events.send_blocking(Event::Voices(v));
            }),
            Command::Daemon(_) => Ok(()),
        };
        if let Err(e) = result {
            error(format!("Speech: {e}"));
        }
    }
}

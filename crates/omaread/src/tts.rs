//! Text-to-speech on a worker thread.
//!
//! Kokoro is the default: each sentence is synthesized by a local
//! OpenAI-compatible server (Kokoro-FastAPI) and played as raw PCM. Setting
//! `[tts] engine = "piper"` keeps the speech-dispatcher/SSIP path instead.
//! Either way the book is spoken one sentence at a time: our own END event
//! moves the reading on, so engines that don't send index marks still work.

use std::collections::HashMap;
use std::num::NonZero;
use std::sync::mpsc;
use std::time::Duration;

use omaread_core::config::{TtsConfig, TtsEngine};
use omaread_core::kokoro;
use omaread_core::ssip;
use omaread_core::voice::Voice;

pub enum Command {
    /// Speak a sentence; `seq` comes back in its events.
    Speak { seq: u64, text: String },
    /// Synthesize a sentence ahead of time, so the next `Speak` is instant.
    Preload { text: String },
    Stop,
    Rate(i32),
    Voice(String),
    Language(String),
    ListVoices,
    /// From the SSIP reader thread.
    Daemon(ssip::Event),
    /// From the prefetch thread: a sentence ready to cache.
    Preloaded { text: String, voice: String, rate: i32, result: Result<Vec<i16>, String> },
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
    pub fn start(events: async_channel::Sender<Event>, config: TtsConfig) -> Tts {
        let (tx, rx) = mpsc::channel::<Command>();
        let worker_tx = tx.clone();
        std::thread::Builder::new()
            .name("tts".into())
            .spawn(move || worker(rx, worker_tx, events, config))
            .expect("a thread for speech");
        Tts { tx }
    }

    pub fn send(&self, command: Command) {
        let _ = self.tx.send(command);
    }
}

fn worker(
    rx: mpsc::Receiver<Command>,
    tx: mpsc::Sender<Command>,
    events: async_channel::Sender<Event>,
    config: TtsConfig,
) {
    match config.engine {
        TtsEngine::Kokoro => kokoro_worker(rx, tx, events, config),
        TtsEngine::Piper => speechd_worker(rx, tx, events),
    }
}

// --- Kokoro -----------------------------------------------------------------

enum Playback {
    Play { seq: u64, samples: Vec<i16> },
    Stop,
}

struct Cached {
    text: String,
    voice: String,
    rate: i32,
    samples: Vec<i16>,
}

fn kokoro_worker(
    rx: mpsc::Receiver<Command>,
    tx: mpsc::Sender<Command>,
    events: async_channel::Sender<Event>,
    config: TtsConfig,
) {
    let (play_tx, play_rx) = mpsc::channel::<Playback>();
    std::thread::Builder::new()
        .name("tts-audio".into())
        .spawn({
            let events = events.clone();
            move || playback(play_rx, events)
        })
        .expect("a thread for audio");

    let mut rate = 0;
    let mut voice = if config.voice.is_empty() { "af_heart".to_string() } else { config.voice.clone() };
    // One sentence of lookahead: synthesized while the current one plays.
    let mut cache: Option<Cached> = None;
    let mut prefetching: Option<String> = None;
    let mut queued: Option<String> = None;

    while let Ok(command) = rx.recv() {
        match command {
            Command::Speak { seq, text } => {
                let samples = match cache.take().filter(|c| c.text == text && c.voice == voice && c.rate == rate) {
                    Some(cached) => cached.samples,
                    None => match kokoro::speak(&config, &text, &voice, kokoro::speed(rate)) {
                        Ok(samples) => samples,
                        Err(e) => {
                            let _ = events.send_blocking(Event::Error(e));
                            continue;
                        }
                    },
                };
                if play_tx.send(Playback::Play { seq, samples }).is_err() {
                    break;
                }
            }
            Command::Preload { text } => {
                if cache.as_ref().is_some_and(|c| c.text == text && c.voice == voice && c.rate == rate) {
                    continue;
                }
                if prefetching.as_deref() == Some(text.as_str()) {
                    continue;
                }
                if prefetching.is_some() {
                    queued = Some(text);
                    continue;
                }
                if start_prefetch(&tx, &config, text.clone(), voice.clone(), rate) {
                    prefetching = Some(text);
                }
            }
            Command::Preloaded { text, voice: got_voice, rate: got_rate, result } => {
                if prefetching.as_deref() == Some(text.as_str()) {
                    prefetching = None;
                }
                if got_voice == voice && got_rate == rate && let Ok(samples) = result {
                    cache = Some(Cached { text, voice: got_voice, rate: got_rate, samples });
                }
                if let Some(next) = queued.take()
                    && start_prefetch(&tx, &config, next.clone(), voice.clone(), rate)
                {
                    prefetching = Some(next);
                }
            }
            Command::Stop => {
                let _ = play_tx.send(Playback::Stop);
            }
            Command::Rate(r) => {
                rate = r;
                cache = None;
            }
            Command::Voice(v) => {
                voice = v;
                cache = None;
            }
            // Kokoro picks its language from the voice id.
            Command::Language(_) => {}
            Command::ListVoices => {
                let _ = events.send_blocking(Event::Voices(kokoro::voices(&config)));
            }
            Command::Daemon(_) => {}
        }
    }
}

/// Synthesize off the command thread so Stop and Rate stay responsive.
fn start_prefetch(tx: &mpsc::Sender<Command>, config: &TtsConfig, text: String, voice: String, rate: i32) -> bool {
    let tx = tx.clone();
    let config = config.clone();
    std::thread::Builder::new()
        .name("tts-prefetch".into())
        .spawn(move || {
            let result = kokoro::speak(&config, &text, &voice, kokoro::speed(rate));
            let _ = tx.send(Command::Preloaded { text, voice, rate, result });
        })
        .is_ok()
}

/// Owns the audio device. A new sentence replaces the old one, and an END
/// event goes out when the samples have actually finished playing. The device
/// is opened on the first sentence, not when the app starts.
fn playback(rx: mpsc::Receiver<Playback>, events: async_channel::Sender<Event>) {
    let mut _sink: Option<rodio::MixerDeviceSink> = None;
    let mut player: Option<rodio::Player> = None;

    let mut pending = None;
    loop {
        let command = match pending.take() {
            Some(command) => command,
            None => match rx.recv() {
                Ok(command) => command,
                Err(_) => break,
            },
        };
        match command {
            Playback::Stop => {
                if let Some(player) = &player {
                    player.stop();
                }
            }
            Playback::Play { seq, samples } => {
                if player.is_none() {
                    match rodio::DeviceSinkBuilder::open_default_sink() {
                        Ok(mut opened) => {
                            opened.log_on_drop(false);
                            let connected = rodio::Player::connect_new(opened.mixer());
                            if cfg!(debug_assertions) && std::env::var_os("OMAREAD_TTS_MUTE").is_some() {
                                connected.set_volume(0.0);
                            }
                            _sink = Some(opened);
                            player = Some(connected);
                        }
                        Err(e) => {
                            let _ = events.send_blocking(Event::Error(format!("Audio output isn't available: {e}")));
                            continue;
                        }
                    }
                }
                let Some(player) = player.as_ref() else { continue };
                player.stop();
                if !samples.is_empty() {
                    let channels = NonZero::new(1).expect("one channel");
                    let rate = NonZero::new(kokoro::SAMPLE_RATE).expect("a sample rate");
                    let samples: Vec<f32> = samples.iter().map(|&s| s as f32 / 32_768.0).collect();
                    player.append(rodio::buffer::SamplesBuffer::new(channels, rate, samples));
                }
                loop {
                    if player.empty() {
                        let _ = events.send_blocking(Event::Ended(seq));
                        break;
                    }
                    match rx.try_recv() {
                        Ok(command) => {
                            pending = Some(command);
                            break;
                        }
                        Err(mpsc::TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(20)),
                        Err(mpsc::TryRecvError::Disconnected) => return,
                    }
                }
            }
        }
    }
}

// --- speech-dispatcher (Piper and other modules) ----------------------------

fn speechd_worker(rx: mpsc::Receiver<Command>, tx: mpsc::Sender<Command>, events: async_channel::Sender<Event>) {
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
            Command::Preload { .. } | Command::Preloaded { .. } => Ok(()),
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

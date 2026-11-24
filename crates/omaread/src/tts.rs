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
    /// `order` is (block, sentence): the nearer sentence is made first, even
    /// when the next block's first sentence was asked for early.
    Preload { text: String, order: (u64, usize) },
    /// Drop sentences queued for synthesis but not started (a jump or a step).
    ForgetQueued,
    Stop,
    Rate(i32),
    Voice(String),
    Language(String),
    ListVoices,
    /// From the SSIP reader thread.
    Daemon(ssip::Event),
    /// From the prefetch thread: a sentence ready to cache.
    Preloaded { text: String, voice: String, rate: i32, result: Result<Vec<i16>, String> },
    /// From the synth thread: a piece of the sentence, played as it arrives.
    Audio { generation: u64, seq: u64, samples: Vec<i16> },
    /// From the synth thread: the whole sentence, for the cache, after the
    /// pieces have gone out.
    Streamed {
        generation: u64,
        seq: u64,
        text: String,
        voice: String,
        rate: i32,
        result: Result<Vec<i16>, String>,
    },
}

#[derive(Debug)]
pub enum Event {
    Ended(u64),
    Started(u64),
    Voices(Vec<Voice>),
    Error(String),
    SpeechError(u64, String),
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
    /// All the audio at once (a cached or prefetched sentence).
    Play { seq: u64, samples: Vec<i16> },
    /// A streamed sentence begins; chunks follow.
    Start { seq: u64 },
    Chunk { seq: u64, samples: Vec<i16> },
    /// No more chunks; end when the queue drains.
    Finish { seq: u64 },
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
    // Sentences synthesized while an earlier one plays. More than one slot
    // because two sentences are requested at a time; a single slot would
    // throw away the one that plays next.
    let mut cache: Vec<Cached> = Vec::new();
    // The prefetch in flight, with the voice and rate it was made for.
    let mut prefetching: Option<(String, String, i32)> = None;
    let mut queued: Vec<((u64, usize), String)> = Vec::new();
    // A `Speak` whose sentence is already being prefetched: play that audio
    // when it lands instead of asking the server for it a second time.
    let mut waiting: Option<(u64, String)> = None;
    // True while the sentence being spoken is still being made. Nothing for
    // later starts until its audio is ready, so the server works on it first.
    let mut speaking = false;
    let mut generation = 0u64;

    loop {
        // Prefetch only when the current sentence has its audio. Asking for
        // the next one earlier would make the current one wait on the server.
        if !speaking
            && prefetching.is_none()
            && !queued.is_empty()
            && let (_, next) = queued.remove(0)
            && start_prefetch(&tx, &config, next.clone(), voice.clone(), rate)
        {
            prefetching = Some((next, voice.clone(), rate));
        }
        let Ok(command) = rx.recv() else { break };
        match command {
            Command::Speak { seq, text } => {
                generation = generation.wrapping_add(1);
                if let Some(samples) = cached_samples(&cache, &text, &voice, rate) {
                    let _ = play_tx.send(Playback::Play { seq, samples });
                    speaking = false;
                } else if prefetching.as_ref().is_some_and(|(t, v, r)| t == &text && v == &voice && *r == rate) {
                    waiting = Some((seq, text));
                    speaking = true;
                } else {
                    speaking = true;
                    // Start playing as the server makes the audio, instead of
                    // waiting for the whole sentence.
                    let _ = play_tx.send(Playback::Start { seq });
                    let (tx, config, stream_voice, generation) = (tx.clone(), config.clone(), voice.clone(), generation);
                    let said = text.clone();
                    std::thread::spawn(move || {
                        let chunks = tx.clone();
                        let result = kokoro::speak_stream(&config, &text, &stream_voice, kokoro::speed(rate), |samples| {
                            let _ = chunks.send(Command::Audio { generation, seq, samples });
                        });
                        let _ = tx.send(Command::Streamed { generation, seq, text: said, voice: stream_voice, rate, result });
                    });
                }
            }
            Command::Audio { generation: got, seq, samples } => {
                if got == generation {
                    let _ = play_tx.send(Playback::Chunk { seq, samples });
                }
            }
            Command::Streamed { generation: got, seq, text, voice: got_voice, rate: got_rate, result } => {
                match result {
                    Ok(samples) => {
                        // Kept so pause and resume replay it instead of asking
                        // the server to say it again. A stale stream (stopped
                        // mid-sentence) still fills the cache.
                        if got_voice == voice && got_rate == rate {
                            remember(&mut cache, Cached { text, voice: got_voice, rate: got_rate, samples });
                        }
                        if got == generation {
                            speaking = false;
                            let _ = play_tx.send(Playback::Finish { seq });
                        }
                    }
                    Err(e) if got == generation => {
                        speaking = false;
                        let _ = play_tx.send(Playback::Stop);
                        let _ = events.send_blocking(Event::SpeechError(seq, e));
                    }
                    Err(_) => {}
                }
            }
            Command::Preload { text, order } => {
                if has_cached(&cache, &text, &voice, rate)
                    || prefetching.as_ref().is_some_and(|(t, _, _)| t == &text)
                    || queued.iter().any(|(_, q)| q == &text)
                {
                    continue;
                }
                queued.push((order, text));
                queued.sort_by_key(|(order, _)| *order);
                // Full: give up the sentence furthest from the reading.
                if queued.len() > QUEUED_LIMIT {
                    queued.pop();
                }
            }
            Command::ForgetQueued => queued.clear(),
            Command::Preloaded { text, voice: got_voice, rate: got_rate, result } => {
                if prefetching.as_ref().is_some_and(|(t, _, _)| t == &text) {
                    prefetching = None;
                }
                match result {
                    Ok(samples) if got_voice == voice && got_rate == rate => {
                        if let Some((seq, _)) = waiting.take_if(|(_, want)| want == &text) {
                            remember(
                                &mut cache,
                                Cached { text: text.clone(), voice: got_voice.clone(), rate: got_rate, samples: samples.clone() },
                            );
                            speaking = false;
                            let _ = play_tx.send(Playback::Play { seq, samples });
                        } else {
                            remember(&mut cache, Cached { text, voice: got_voice, rate: got_rate, samples });
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        if let Some((seq, _)) = waiting.take_if(|(_, want)| want == &text) {
                            speaking = false;
                            let _ = events.send_blocking(Event::SpeechError(seq, e));
                        }
                    }
                }
            }
            Command::Stop => {
                generation = generation.wrapping_add(1);
                waiting = None;
                speaking = false;
                let _ = play_tx.send(Playback::Stop);
            }
            Command::Rate(r) => {
                rate = r;
                generation = generation.wrapping_add(1);
                cache.clear();
                queued.clear();
                waiting = None;
            }
            Command::Voice(v) => {
                voice = v;
                generation = generation.wrapping_add(1);
                cache.clear();
                queued.clear();
                waiting = None;
            }
            // Kokoro picks its language from the voice id.
            Command::Language(_) => {}
            Command::ListVoices => {
                let (events, config) = (events.clone(), config.clone());
                std::thread::spawn(move || {
                    let _ = events.send_blocking(Event::Voices(kokoro::voices(&config)));
                });
            }
            Command::Daemon(_) => {}
        }
    }
}

/// Sentences kept ready to play, and how many may wait their turn.
const CACHE_LIMIT: usize = 6;
const QUEUED_LIMIT: usize = 3;

fn cached_samples(cache: &[Cached], text: &str, voice: &str, rate: i32) -> Option<Vec<i16>> {
    cache.iter().find(|c| c.text == text && c.voice == voice && c.rate == rate).map(|c| c.samples.clone())
}

fn has_cached(cache: &[Cached], text: &str, voice: &str, rate: i32) -> bool {
    cache.iter().any(|c| c.text == text && c.voice == voice && c.rate == rate)
}

fn remember(cache: &mut Vec<Cached>, entry: Cached) {
    cache.retain(|c| !(c.text == entry.text && c.voice == entry.voice && c.rate == entry.rate));
    cache.push(entry);
    if cache.len() > CACHE_LIMIT {
        cache.remove(0);
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

/// Owns the audio device. A sentence's audio is appended as it arrives, and
/// an END event goes out when the queue has drained. The device is opened on
/// the first sentence, not when the app starts.
fn playback(rx: mpsc::Receiver<Playback>, events: async_channel::Sender<Event>) {
    let mut _sink: Option<rodio::MixerDeviceSink> = None;
    let mut player: Option<rodio::Player> = None;
    // The sentence being played, and whether more of it is still arriving.
    let mut current: Option<(u64, bool)> = None;
    let mut started = false;

    loop {
        // While a sentence plays, poll for its next chunk; when idle, wait.
        let command = if current.is_some() {
            match rx.recv_timeout(Duration::from_millis(15)) {
                Ok(command) => Some(command),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        } else {
            match rx.recv() {
                Ok(command) => Some(command),
                Err(_) => return,
            }
        };
        if let Some(command) = command {
            match command {
                Playback::Stop => {
                    if let Some(player) = &player {
                        player.stop();
                    }
                    current = None;
                    started = false;
                }
                Playback::Play { seq, samples } => {
                    let Some(playing) = ensure_player(&mut _sink, &mut player, &events, seq) else {
                        current = None;
                        started = false;
                        continue;
                    };
                    playing.stop();
                    append(playing, samples);
                    let _ = events.send_blocking(Event::Started(seq));
                    current = Some((seq, false));
                    started = true;
                }
                Playback::Start { seq } => {
                    let Some(playing) = ensure_player(&mut _sink, &mut player, &events, seq) else {
                        current = None;
                        started = false;
                        continue;
                    };
                    playing.stop();
                    current = Some((seq, true));
                    started = false;
                }
                Playback::Chunk { seq, samples } => {
                    if current.is_some_and(|(playing, _)| playing == seq) {
                        if !started {
                            let _ = events.send_blocking(Event::Started(seq));
                            started = true;
                        }
                        if let Some(playing) = &player {
                            append(playing, samples);
                        }
                    }
                }
                Playback::Finish { seq } => {
                    if let Some((playing, streaming)) = current.as_mut()
                        && *playing == seq
                    {
                        *streaming = false;
                    }
                }
            }
        }
        // A stream that finished without making a sound still has to move the
        // reading on.
        if let Some((seq, false)) = current
            && !started
        {
            let _ = events.send_blocking(Event::Ended(seq));
            current = None;
        }
        if let Some((seq, false)) = current
            && player.as_ref().is_some_and(|playing| playing.empty())
        {
            let _ = events.send_blocking(Event::Ended(seq));
            current = None;
            started = false;
        }
    }
}

fn ensure_player<'a>(
    sink: &'a mut Option<rodio::MixerDeviceSink>,
    player: &'a mut Option<rodio::Player>,
    events: &async_channel::Sender<Event>,
    seq: u64,
) -> Option<&'a rodio::Player> {
    if player.is_none() {
        match rodio::DeviceSinkBuilder::open_default_sink() {
            Ok(mut opened) => {
                opened.log_on_drop(false);
                let connected = rodio::Player::connect_new(opened.mixer());
                if cfg!(debug_assertions) && std::env::var_os("OMAREAD_TTS_MUTE").is_some() {
                    connected.set_volume(0.0);
                }
                *sink = Some(opened);
                *player = Some(connected);
            }
            Err(e) => {
                let _ = events.send_blocking(Event::SpeechError(seq, format!("Audio output isn't available: {e}")));
                return None;
            }
        }
    }
    player.as_ref()
}

fn append(player: &rodio::Player, samples: Vec<i16>) {
    if samples.is_empty() {
        return;
    }
    let channels = NonZero::new(1).expect("one channel");
    let rate = NonZero::new(kokoro::SAMPLE_RATE).expect("a sample rate");
    let samples: Vec<f32> = samples.iter().map(|&s| s as f32 / 32_768.0).collect();
    player.append(rodio::buffer::SamplesBuffer::new(channels, rate, samples));
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
                let _ = events.send_blocking(Event::Started(seq));
            }),
            Command::Preload { .. }
            | Command::Preloaded { .. }
            | Command::Audio { .. }
            | Command::Streamed { .. }
            | Command::ForgetQueued => Ok(()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn read_request(stream: &mut std::net::TcpStream) -> String {
        let mut data = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            let n = stream.read(&mut buf).unwrap_or(0);
            if n == 0 {
                break;
            }
            data.extend_from_slice(&buf[..n]);
            if let Some(end) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&data[..end]).to_ascii_lowercase();
                let length: usize = head
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:").map(|n| n.trim().parse().unwrap_or(0)))
                    .unwrap_or(0);
                if data.len() >= end + 4 + length {
                    break;
                }
            }
        }
        String::from_utf8_lossy(&data).to_string()
    }

    fn request_input(request: &str) -> String {
        let body = request.split_once("\r\n\r\n").map(|(_, body)| body).unwrap_or_default();
        serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .and_then(|v| v.get("input").and_then(|input| input.as_str()).map(str::to_string))
            .unwrap_or_default()
    }

    #[test]
    fn resuming_a_sentence_does_not_synthesize_it_again() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let (count_tx, count_rx) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            let mut count = 0;
            while std::time::Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = read_request(&mut stream);
                        count += 1;
                        let _ = count_tx.send(count);
                        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n\0\0");
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(10)),
                    Err(_) => break,
                }
            }
            count
        });
        let (events_tx, _events_rx) = async_channel::unbounded();
        let config = TtsConfig { url: format!("http://{addr}"), ..TtsConfig::default() };
        let tts = Tts::start(events_tx, config);
        tts.send(Command::Speak { seq: 1, text: "Hello there.".into() });
        assert_eq!(count_rx.recv_timeout(Duration::from_secs(5)).unwrap(), 1);
        // Let the samples reach the cache before the second ask.
        std::thread::sleep(Duration::from_millis(200));
        tts.send(Command::Stop);
        tts.send(Command::Speak { seq: 2, text: "Hello there.".into() });
        std::thread::sleep(Duration::from_millis(300));
        assert!(count_rx.try_recv().is_err(), "the second Speak asked for the sentence again");
        drop(tts);
        assert_eq!(server.join().unwrap(), 1);
    }

    #[test]
    fn the_nearest_sentence_is_synthesized_first() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (seen_tx, seen_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let mut order = Vec::new();
            for i in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                let request = read_request(&mut stream);
                let input = request_input(&request);
                order.push(input.clone());
                seen_tx.send(input).unwrap();
                if i == 0 {
                    // Hold the first request until the others are queued.
                    release_rx.recv().unwrap();
                }
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n\0\0");
            }
            order
        });
        let (events_tx, _events_rx) = async_channel::unbounded();
        let config = TtsConfig { url: format!("http://{addr}"), ..TtsConfig::default() };
        let tts = Tts::start(events_tx, config);
        tts.send(Command::Preload { text: "Block one, first.".into(), order: (1, 0) });
        assert_eq!(seen_rx.recv_timeout(Duration::from_secs(5)).unwrap(), "Block one, first.");
        // The next block's first sentence is known early, but the rest of
        // this block is needed sooner.
        tts.send(Command::Preload { text: "Block two, first.".into(), order: (2, 0) });
        tts.send(Command::Preload { text: "Block one, second.".into(), order: (1, 1) });
        release_tx.send(()).unwrap();
        assert_eq!(seen_rx.recv_timeout(Duration::from_secs(5)).unwrap(), "Block one, second.");
        assert_eq!(seen_rx.recv_timeout(Duration::from_secs(5)).unwrap(), "Block two, first.");
        let order = server.join().unwrap();
        assert_eq!(order, ["Block one, first.", "Block one, second.", "Block two, first."]);
    }

    #[test]
    fn stop_discards_late_streamed_audio() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (arrived_tx, arrived_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut first, _) = listener.accept().unwrap();
            let mut buf = [0; 4096];
            let _ = first.read(&mut buf);
            arrived_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            first.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n\0\0").unwrap();
            let (mut second, _) = listener.accept().unwrap();
            let _ = second.read(&mut buf);
            second.write_all(b"HTTP/1.1 500 Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        });
        let (events_tx, events_rx) = async_channel::unbounded();
        let config = TtsConfig { url: format!("http://{addr}"), ..TtsConfig::default() };
        let tts = Tts::start(events_tx, config);
        tts.send(Command::Speak { seq: 1, text: "First sentence".into() });
        arrived_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        tts.send(Command::Stop);
        tts.send(Command::Speak { seq: 2, text: "Second sentence".into() });
        release_tx.send(()).unwrap();
        // The first request's audio arrives after Stop and must not speak.
        // Look for the server's own error for the second request; a machine
        // without an audio device also reports that separately.
        let mut server_error = None;
        while let Ok(event) = events_rx.recv_blocking() {
            if let Event::SpeechError(2, message) = event
                && message.starts_with("Kokoro")
            {
                server_error = Some(message);
                break;
            }
        }
        assert!(server_error.is_some(), "the second request's error never arrived");
        server.join().unwrap();
    }
}

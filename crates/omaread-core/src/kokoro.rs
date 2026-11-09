//! Kokoro TTS through an OpenAI-compatible local server (Kokoro-FastAPI by
//! default). Speech comes back as raw 24 kHz mono PCM, ready to play.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::TtsConfig;
use crate::voice::Voice;

/// Kokoro-FastAPI's `response_format = "pcm"` is 24 kHz, mono, signed 16-bit.
pub const SAMPLE_RATE: u32 = 24_000;

/// How long to wait for a sentence. GPU servers answer in well under a second;
/// a CPU server or a long sentence may take a few.
const SPEECH_TIMEOUT: Duration = Duration::from_secs(60);
/// Voices are a nicety, so a missing server shouldn't stall the panel.
const VOICES_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Serialize)]
struct SpeechRequest<'a> {
    model: &'a str,
    input: &'a str,
    voice: &'a str,
    response_format: &'a str,
    speed: f32,
}

#[derive(Deserialize)]
struct VoiceList {
    #[serde(default)]
    voices: Vec<VoiceEntry>,
}

#[derive(Deserialize)]
struct VoiceEntry {
    id: String,
}

/// The speech endpoint: the configured URL, or `/audio/speech` under it.
pub fn speech_url(url: &str) -> String {
    let base = url.trim_end_matches('/');
    if base.ends_with("/audio/speech") { base.to_string() } else { format!("{base}/audio/speech") }
}

/// The voices endpoint that belongs to the speech endpoint.
pub fn voices_url(url: &str) -> String {
    let base = url.trim_end_matches('/');
    match base.strip_suffix("/audio/speech") {
        Some(root) => format!("{root}/audio/voices"),
        None => format!("{base}/audio/voices"),
    }
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build()
        .into()
}

/// Synthesize one sentence. Returns interleaved i16 samples at [`SAMPLE_RATE`].
pub fn speak(config: &TtsConfig, text: &str, voice: &str, speed: f32) -> Result<Vec<i16>, String> {
    let body = serde_json::to_vec(&SpeechRequest {
        model: &config.model,
        input: text,
        voice,
        response_format: "pcm",
        speed: speed.clamp(0.25, 4.0),
    })
    .map_err(|e| e.to_string())?;
    let url = speech_url(&config.url);
    let mut request = agent(SPEECH_TIMEOUT).post(&url).header("Content-Type", "application/json");
    if let Some(key) = config.api_key.as_deref().filter(|k| !k.is_empty()) {
        request = request.header("Authorization", &format!("Bearer {key}"));
    }
    let mut response = request.send(body).map_err(|e| format!("Kokoro isn't answering at {url}: {e}"))?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        let detail = response.body_mut().read_to_string().unwrap_or_default();
        return Err(format!("Kokoro: {status} {}", detail.trim()));
    }
    let bytes = response.body_mut().read_to_vec().map_err(|e| format!("Kokoro: {e}"))?;
    Ok(bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect())
}

/// The server's voices, or Kokoro's built-in list when it can't be reached.
pub fn voices(config: &TtsConfig) -> Vec<Voice> {
    fetch_voices(config).filter(|voices| !voices.is_empty()).unwrap_or_else(builtin_voices)
}

fn fetch_voices(config: &TtsConfig) -> Option<Vec<Voice>> {
    let url = voices_url(&config.url);
    let mut response = agent(VOICES_TIMEOUT).get(&url).call().ok()?;
    if !response.status().is_success() {
        return None;
    }
    let text = response.body_mut().read_to_string().ok()?;
    let list: VoiceList = serde_json::from_str(&text).ok()?;
    let voices: Vec<Voice> = list.voices.into_iter().map(|entry| voice_from_id(&entry.id)).collect();
    (!voices.is_empty()).then_some(voices)
}

/// `af_bella` → an American English female voice.
pub fn voice_from_id(id: &str) -> Voice {
    let (language, variant) = voice_kind(id);
    Voice { name: id.to_string(), language: language.into(), variant: variant.into() }
}

fn voice_kind(id: &str) -> (&'static str, &'static str) {
    let Some(prefix) = id.split('_').next() else { return ("", "") };
    let language = match prefix {
        "af" | "am" => "en-US",
        "bf" | "bm" => "en-GB",
        "ef" | "em" => "es",
        "ff" => "fr",
        "hf" | "hm" => "hi",
        "if" | "im" => "it",
        "jf" | "jm" => "ja",
        "pf" | "pm" => "pt-BR",
        "zf" | "zm" => "zh",
        _ => return ("", ""),
    };
    let variant = match prefix.as_bytes().get(1) {
        Some(b'f') => "female",
        Some(b'm') => "male",
        _ => "",
    };
    (language, variant)
}

/// Kokoro v1.0's voices, so the panel works before the server is up.
pub fn builtin_voices() -> Vec<Voice> {
    const IDS: &[&str] = &[
        "af_alloy", "af_aoede", "af_bella", "af_heart", "af_jessica", "af_kore", "af_nicole", "af_nova", "af_river", "af_sarah", "af_sky",
        "am_adam", "am_echo", "am_eric", "am_fenrir", "am_liam", "am_michael", "am_onyx", "am_puck", "am_santa",
        "bf_alice", "bf_emma", "bf_isabella", "bf_lily", "bm_daniel", "bm_fable", "bm_george", "bm_lewis",
        "ef_dora", "em_alex", "em_santa", "ff_siwis", "hf_alpha", "hf_beta", "hm_omega", "hm_psi",
        "if_sara", "im_nicola", "jf_alpha", "jf_gongitsune", "jf_nezumi", "jf_tebukuro", "jm_kumo",
        "pf_dora", "pm_alex", "pm_santa", "zf_xiaobei", "zf_xiaoni", "zf_xiaoxiao", "zf_xiaoyi",
        "zm_yunjian", "zm_yunxi", "zm_yunxia", "zm_yunyang",
    ];
    IDS.iter().map(|id| voice_from_id(id)).collect()
}

/// The UI's pace nudge as a Kokoro speed multiplier. A step of 5 is 0.05×.
pub fn speed(rate: i32) -> f32 {
    (1.0 + rate as f32 / 100.0).clamp(0.5, 4.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn urls_grow_from_a_base_or_keep_a_full_endpoint() {
        assert_eq!(speech_url("http://127.0.0.1:8880/v1"), "http://127.0.0.1:8880/v1/audio/speech");
        assert_eq!(speech_url("http://127.0.0.1:8880/v1/"), "http://127.0.0.1:8880/v1/audio/speech");
        assert_eq!(speech_url("http://host/custom/speech"), "http://host/custom/speech/audio/speech");
        let full = "http://host/v1/audio/speech";
        assert_eq!(speech_url(full), full);
        assert_eq!(voices_url(full), "http://host/v1/audio/voices");
        assert_eq!(voices_url("http://host/v1"), "http://host/v1/audio/voices");
    }

    #[test]
    fn voices_know_their_language_and_gender() {
        assert_eq!(voice_kind("af_bella"), ("en-US", "female"));
        assert_eq!(voice_kind("bm_george"), ("en-GB", "male"));
        assert_eq!(voice_kind("jf_alpha"), ("ja", "female"));
        assert_eq!(voice_kind("custom"), ("", ""));
    }

    #[test]
    fn the_builtin_list_is_the_whole_kokoro_cast() {
        let voices = builtin_voices();
        assert_eq!(voices.len(), 54);
        assert!(voices.iter().any(|v| v.name == "af_heart"));
        assert!(!voices.iter().any(|v| v.language.is_empty()));
    }

    #[test]
    fn speed_maps_the_ui_nudge() {
        assert_eq!(speed(0), 1.0);
        assert_eq!(speed(5), 1.05);
        assert_eq!(speed(50), 1.5);
        assert_eq!(speed(-50), 0.5);
        assert_eq!(speed(100), 2.0);
        assert_eq!(speed(300), 4.0);
        assert_eq!(speed(500), 4.0);
    }

    /// Read one HTTP request, body included, so the test can check the JSON.
    fn read_request(stream: &mut std::net::TcpStream) -> String {
        let mut data = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            let n = stream.read(&mut chunk).unwrap_or(0);
            if n == 0 {
                break;
            }
            data.extend_from_slice(&chunk[..n]);
            if let Some(end) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&data[..end]).to_string();
                let length = head
                    .lines()
                    .find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length:").map(|n| n.trim().parse::<usize>().unwrap_or(0)))
                    .unwrap_or(0);
                if data.len() >= end + 4 + length {
                    break;
                }
            }
        }
        String::from_utf8_lossy(&data).to_string()
    }

    #[test]
    fn speak_posts_the_voice_and_plays_the_pcm_back() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request(&mut stream);
            assert!(request.starts_with("POST /v1/audio/speech "), "{request}");
            assert!(request.contains("\"input\":\"Hello.\""), "{request}");
            assert!(request.contains("\"voice\":\"af_bella\""), "{request}");
            assert!(request.contains("\"response_format\":\"pcm\""), "{request}");
            assert!(request.contains("\"model\":\"kokoro\""), "{request}");

            let pcm: Vec<u8> = [123i16, -456].iter().flat_map(|s| s.to_le_bytes()).collect();
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: audio/pcm\r\nConnection: close\r\n\r\n",
                pcm.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(&pcm).unwrap();
        });

        let config = TtsConfig { url: format!("http://{address}/v1"), ..TtsConfig::default() };
        let samples = speak(&config, "Hello.", "af_bella", 1.0).unwrap();
        assert_eq!(samples, vec![123, -456]);
        server.join().unwrap();
    }
}

//! A small client for speech-dispatcher's SSIP protocol, over its Unix socket.
//!
//! Commands are lines; replies are `NNN-data` lines ending in `NNN text`.
//! With notifications on, events (`7xx`) arrive on the same socket at any
//! time, so a reader thread splits them off: replies go back to the caller,
//! events to a callback.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Begin(u64),
    End(u64),
    Canceled(u64),
    Mark(u64, String),
    Paused(u64),
    Resumed(u64),
    /// The connection closed (the daemon quit or crashed).
    Disconnected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Voice {
    /// What `SET SELF SYNTHESIS_VOICE` takes (for piper, the model path).
    pub name: String,
    pub language: String,
    pub variant: String,
}

impl Voice {
    /// "lessac (medium)" from a piper model path; the name itself otherwise.
    pub fn label(&self) -> String {
        let file = self.name.rsplit('/').next().unwrap_or(&self.name);
        let stem = file.strip_suffix(".onnx").unwrap_or(file);
        let mut parts = stem.split('-');
        match (parts.next(), parts.next(), parts.next()) {
            (Some(_lang), Some(speaker), Some(quality)) => format!("{speaker} ({quality})"),
            _ => stem.to_string(),
        }
    }
}

/// `250-piper` → (250, false, "piper"); `250 OK DONE` → (250, true, "OK DONE").
pub fn parse_line(line: &str) -> Option<(u16, bool, &str)> {
    let code: u16 = line.get(..3)?.parse().ok()?;
    let last = match line.as_bytes().get(3) {
        Some(b' ') | None => true,
        Some(b'-') => false,
        _ => return None,
    };
    Some((code, last, line.get(4..).unwrap_or("")))
}

/// Turn a complete event block into an [`Event`].
pub fn parse_event(code: u16, data: &[String]) -> Option<Event> {
    let id: u64 = data.first()?.trim().parse().ok()?;
    Some(match code {
        700 => Event::Mark(id, data.get(2).cloned().unwrap_or_default()),
        701 => Event::Begin(id),
        702 => Event::End(id),
        703 => Event::Canceled(id),
        704 => Event::Paused(id),
        705 => Event::Resumed(id),
        _ => return None,
    })
}

/// Text as SSIP wants it: CRLF lines, and a lone "." doubled so it can't end
/// the message early.
pub fn escape_body(text: &str) -> String {
    text.replace("\r\n", "\n")
        .split('\n')
        .map(|line| if line.starts_with('.') { format!(".{line}") } else { line.to_string() })
        .collect::<Vec<_>>()
        .join("\r\n")
}

pub struct Client {
    stream: UnixStream,
    replies: mpsc::Receiver<(u16, Vec<String>)>,
}

const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

/// speech-dispatcher's socket in this user's runtime directory. Never a
/// shared location like /tmp: another user could put a socket there and
/// receive what's read aloud.
fn socket_path() -> Result<PathBuf, String> {
    if let Some(addr) = std::env::var("SPEECHD_ADDRESS").ok().and_then(|a| a.strip_prefix("unix_socket:").map(String::from)) {
        return Ok(PathBuf::from(addr));
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or("XDG_RUNTIME_DIR isn't set, so there's no private place to find speech-dispatcher")?;
    Ok(runtime.join("speech-dispatcher/speechd.sock"))
}

/// The socket must be ours: owned by the user whose runtime directory it's in.
fn owned_by_us(path: &std::path::Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let (Ok(socket), Some(runtime)) = (std::fs::metadata(path), std::env::var_os("XDG_RUNTIME_DIR")) else {
        // SPEECHD_ADDRESS without a runtime dir: the user chose it explicitly.
        return std::env::var_os("SPEECHD_ADDRESS").is_some();
    };
    std::fs::metadata(runtime).is_ok_and(|dir| dir.uid() == socket.uid())
}

impl Client {
    /// Connect, starting the daemon if it isn't running. `component` names
    /// this connection (`user:omaread:<component>`). `on_event` runs on the
    /// reader thread.
    pub fn connect(component: &str, on_event: impl Fn(Event) + Send + 'static) -> Result<Client, String> {
        let path = socket_path()?;
        let stream = match UnixStream::connect(&path) {
            Ok(s) => s,
            Err(_) => {
                let _ = std::process::Command::new(std::env::var("SPEECHD_CMD").unwrap_or_else(|_| "speech-dispatcher".into()))
                    .arg("--spawn")
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
                UnixStream::connect(&path).map_err(|e| format!("speech-dispatcher isn't answering at {}: {e}", path.display()))?
            }
        };
        if !owned_by_us(&path) {
            return Err(format!("{} isn't owned by you; not sending it anything", path.display()));
        }
        let reader = stream.try_clone().map_err(|e| e.to_string())?;
        let (tx, replies) = mpsc::channel();
        std::thread::Builder::new()
            .name("ssip-reader".into())
            .spawn(move || {
                let mut lines = BufReader::new(reader);
                let mut block: Vec<String> = Vec::new();
                let mut line = String::new();
                loop {
                    line.clear();
                    match lines.read_line(&mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    let Some((code, last, text)) = parse_line(line.trim_end_matches(['\r', '\n'])) else { continue };
                    if !last {
                        block.push(text.to_string());
                        continue;
                    }
                    let data = std::mem::take(&mut block);
                    if (700..800).contains(&code) {
                        if let Some(event) = parse_event(code, &data) {
                            on_event(event);
                        }
                    } else {
                        let mut data = data;
                        data.push(text.to_string());
                        if tx.send((code, data)).is_err() {
                            break;
                        }
                    }
                }
                on_event(Event::Disconnected);
            })
            .map_err(|e| e.to_string())?;

        let mut client = Client { stream, replies };
        let user: String = std::env::var("USER").unwrap_or_else(|_| "user".into()).chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        let component: String = component.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        client.command(&format!("SET SELF CLIENT_NAME {user}:omaread:{component}"))?;
        client.command("SET SELF NOTIFICATION ALL on")?;
        Ok(client)
    }

    /// Send a command and wait for its reply; the reply's lines on success.
    pub fn command(&mut self, command: &str) -> Result<Vec<String>, String> {
        self.stream.write_all(format!("{command}\r\n").as_bytes()).map_err(|e| e.to_string())?;
        self.reply()
    }

    fn reply(&mut self) -> Result<Vec<String>, String> {
        let (code, data) = self.replies.recv_timeout(REPLY_TIMEOUT).map_err(|_| "speech-dispatcher didn't reply".to_string())?;
        if (200..300).contains(&code) { Ok(data) } else { Err(format!("{code} {}", data.last().cloned().unwrap_or_default())) }
    }

    /// Queue `text`; returns the message id its events will carry.
    pub fn speak(&mut self, text: &str) -> Result<u64, String> {
        self.command("SPEAK")?;
        self.stream
            .write_all(format!("{}\r\n.\r\n", escape_body(text)).as_bytes())
            .map_err(|e| e.to_string())?;
        let data = self.reply()?;
        data.first().and_then(|id| id.trim().parse().ok()).ok_or_else(|| "no message id".to_string())
    }

    pub fn stop(&mut self) -> Result<(), String> {
        self.command("STOP self").map(|_| ())
    }

    /// −100…100.
    pub fn set_rate(&mut self, rate: i32) -> Result<(), String> {
        self.command(&format!("SET self RATE {}", rate.clamp(-100, 100))).map(|_| ())
    }

    pub fn set_language(&mut self, lang: &str) -> Result<(), String> {
        let lang: String = lang.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(16).collect();
        self.command(&format!("SET self LANGUAGE {lang}")).map(|_| ())
    }

    pub fn set_voice(&mut self, name: &str) -> Result<(), String> {
        if name.contains(['\r', '\n']) {
            return Err("bad voice name".into());
        }
        self.command(&format!("SET self SYNTHESIS_VOICE {name}")).map(|_| ())
    }

    pub fn voices(&mut self) -> Result<Vec<Voice>, String> {
        let data = self.command("LIST SYNTHESIS_VOICES")?;
        Ok(data
            .iter()
            .take(data.len().saturating_sub(1))
            .filter_map(|row| {
                let mut f = row.split('\t');
                Some(Voice { name: f.next()?.to_string(), language: f.next().unwrap_or("").to_string(), variant: f.next().unwrap_or("").to_string() })
            })
            .collect())
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.stream.write_all(b"STOP self\r\nQUIT\r\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_reply_lines() {
        assert_eq!(parse_line("250-piper"), Some((250, false, "piper")));
        assert_eq!(parse_line("250 OK MODULE LIST SENT"), Some((250, true, "OK MODULE LIST SENT")));
        assert_eq!(parse_line("nonsense"), None);
    }

    #[test]
    fn parses_events() {
        let data = vec!["12".to_string(), "3".to_string()];
        assert_eq!(parse_event(702, &data), Some(Event::End(12)));
        let mark = vec!["12".to_string(), "3".to_string(), "7".to_string()];
        assert_eq!(parse_event(700, &mark), Some(Event::Mark(12, "7".into())));
        assert_eq!(parse_event(799, &data), None);
    }

    #[test]
    fn a_dot_line_cannot_end_the_message() {
        assert_eq!(escape_body("one\n.\ntwo"), "one\r\n..\r\ntwo");
        assert_eq!(escape_body(".start"), "..start");
    }

    #[test]
    fn piper_voices_get_readable_names() {
        let v = Voice { name: "/usr/share/piper-voices/en/en_US/lessac/medium/en_US-lessac-medium.onnx".into(), language: "en".into(), variant: "MALE1".into() };
        assert_eq!(v.label(), "lessac (medium)");
    }
}

/// Talks to the real daemon: `cargo test -p omaread-core ssip_live -- --ignored`.
#[cfg(test)]
mod live {
    use super::*;

    #[test]
    #[ignore]
    fn ssip_live_speaks_and_reports_the_end() {
        let (tx, rx) = mpsc::channel();
        let mut c = Client::connect("test", move |e| {
            let _ = tx.send(e);
        })
        .unwrap();
        c.command("SET self VOLUME -100").unwrap();
        assert!(!c.voices().unwrap().is_empty());
        let id = c.speak("One. Two.").unwrap();
        let mut ended = false;
        while let Ok(e) = rx.recv_timeout(Duration::from_secs(15)) {
            if e == Event::End(id) {
                ended = true;
                break;
            }
        }
        assert!(ended);
    }
}

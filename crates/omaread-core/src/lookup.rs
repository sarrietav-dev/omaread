//! Looking up a selection: dictionary, encyclopedia, translation, or
//! anything you configure.
//!
//! Providers are data (`[[lookup.provider]]` in config.toml). Built-in kinds
//! talk to Wiktionary, Wikipedia and Google Translate; `command` runs a
//! program (never through a shell) and `url` opens the browser.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

const USER_AGENT: &str = "Omaread/0.1 (https://github.com/sarrietav-dev/omaread)";
const TIMEOUT: Duration = Duration::from_secs(8);
/// Longer selections are cut before they're sent anywhere.
const MAX_QUERY: usize = 2000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderSpec {
    pub name: String,
    /// `builtin.wiktionary`, `builtin.wikipedia`, `builtin.translate`, `command` or `url`.
    pub kind: String,
    /// The key that picks it, from a selection or the lookup card.
    pub key: String,
    /// `command`: the program and its arguments; `{text}` is the selection.
    #[serde(default)]
    pub command: Vec<String>,
    /// `url`: a template; `{text}` is the selection, URL-encoded.
    #[serde(default)]
    pub url: String,
    /// `builtin.translate`: the two languages to translate between.
    #[serde(default)]
    pub pair: Vec<String>,
    /// `builtin.wikipedia`: a fixed language, else the book's.
    #[serde(default)]
    pub lang: String,
}

pub fn default_providers() -> Vec<ProviderSpec> {
    let p = |name: &str, kind: &str, key: &str| ProviderSpec {
        name: name.into(),
        kind: kind.into(),
        key: key.into(),
        command: vec![],
        url: String::new(),
        pair: vec![],
        lang: String::new(),
    };
    vec![
        p("Dictionary", "builtin.wiktionary", "d"),
        p("Wikipedia", "builtin.wikipedia", "w"),
        ProviderSpec { pair: vec!["es".into(), "en".into()], ..p("Translate", "builtin.translate", "t") },
    ]
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct LookupResult {
    pub title: String,
    pub body: String,
    /// Where to read more, if anywhere.
    pub url: Option<String>,
    /// For `url` providers: nothing to show, open this instead.
    pub open: Option<String>,
}

/// Look `text` up with a provider. Blocking: call it off the UI thread.
/// `lang` is the language of the text, if the book says.
pub fn run(spec: &ProviderSpec, text: &str, lang: Option<&str>) -> Result<LookupResult, String> {
    let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let text: String = text.chars().take(MAX_QUERY).collect();
    if text.is_empty() {
        return Err("nothing selected".into());
    }
    let lang = lang.map(primary_language);
    match spec.kind.as_str() {
        "builtin.wiktionary" => wiktionary(&text, lang.as_deref()),
        "builtin.wikipedia" => {
            let lang = if spec.lang.is_empty() { lang.unwrap_or_else(|| "en".into()) } else { spec.lang.clone() };
            wikipedia(&text, &lang)
        }
        "builtin.translate" | "builtin.google-translate" => translate(&text, lang.as_deref(), &spec.pair),
        "command" => command(&spec.command, &text),
        "url" => url(&spec.url, &text),
        other => Err(format!("unknown provider kind: {other}")),
    }
}

/// `en-US` → `en`.
fn primary_language(lang: &str) -> String {
    lang.split(['-', '_']).next().unwrap_or(lang).to_ascii_lowercase()
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .https_only(true)
        .user_agent(USER_AGENT)
        .http_status_as_error(false)
        .build()
        .into()
}

/// GET a URL; `Ok(None)` on 404.
fn get(url: &str, query: &[(&str, &str)]) -> Result<Option<String>, String> {
    let mut request = agent().get(url);
    for (k, v) in query {
        request = request.query(*k, *v);
    }
    let mut response = request.call().map_err(|e| format!("network: {e}"))?;
    let status = response.status().as_u16();
    if status == 404 {
        return Ok(None);
    }
    if status >= 400 {
        return Err(format!("the service answered {status}"));
    }
    response.body_mut().read_to_string().map(Some).map_err(|e| format!("network: {e}"))
}

fn path_segment(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(*b as char),
            b' ' => out.push('_'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn wiktionary(text: &str, lang: Option<&str>) -> Result<LookupResult, String> {
    // Dictionaries want the word, not the punctuation around it.
    let word = text.trim_matches(|c: char| !c.is_alphanumeric());
    let url = format!("https://en.wiktionary.org/api/rest_v1/page/definition/{}", path_segment(word));
    let mut body = get(&url, &[("redirect", "true")])?;
    if body.is_none() && word.chars().next().is_some_and(char::is_uppercase) {
        let lower = word.to_lowercase();
        body = get(&format!("https://en.wiktionary.org/api/rest_v1/page/definition/{}", path_segment(&lower)), &[("redirect", "true")])?;
    }
    let Some(body) = body else {
        return Err(format!("No dictionary entry for “{word}”"));
    };
    let mut result = parse_wiktionary(&body, lang).ok_or_else(|| format!("No dictionary entry for “{word}”"))?;
    result.title = word.to_string();
    result.url = Some(format!("https://en.wiktionary.org/wiki/{}", path_segment(word)));
    Ok(result)
}

pub fn parse_wiktionary(json: &str, lang: Option<&str>) -> Option<LookupResult> {
    #[derive(Deserialize)]
    struct Usage {
        #[serde(rename = "partOfSpeech", default)]
        part_of_speech: String,
        #[serde(default)]
        language: String,
        #[serde(default)]
        definitions: Vec<Definition>,
    }
    #[derive(Deserialize)]
    struct Definition {
        #[serde(default)]
        definition: String,
        #[serde(default)]
        examples: Vec<String>,
    }
    let all: std::collections::BTreeMap<String, Vec<Usage>> = serde_json::from_str(json).ok()?;
    // The book's language first, then English, then whatever there is.
    let mut order: Vec<&String> = all.keys().collect();
    order.sort_by_key(|k| (Some(k.as_str()) != lang, k.as_str() != "en", k.as_str().to_string()));
    let mut out = String::new();
    for code in order.into_iter().take(2) {
        for usage in &all[code] {
            let defs: Vec<String> = usage
                .definitions
                .iter()
                .map(|d| strip_html(&d.definition))
                .filter(|d| !d.is_empty())
                .take(5)
                .collect();
            if defs.is_empty() {
                continue;
            }
            out.push_str(&format!("{} · {}\n", usage.part_of_speech.to_lowercase(), usage.language));
            for (i, d) in defs.iter().enumerate() {
                out.push_str(&format!("  {}. {d}\n", i + 1));
                if i == 0
                    && let Some(example) = usage.definitions[0].examples.first().map(|e| strip_html(e)).filter(|e| !e.is_empty())
                {
                    out.push_str(&format!("     “{example}”\n"));
                }
            }
            out.push('\n');
        }
    }
    let body = out.trim_end().to_string();
    (!body.is_empty()).then(|| LookupResult { body, ..Default::default() })
}

fn wikipedia(text: &str, lang: &str) -> Result<LookupResult, String> {
    let lang = if lang.len() <= 3 && lang.chars().all(|c| c.is_ascii_lowercase()) { lang } else { "en" };
    let summary = |title: &str| get(&format!("https://{lang}.wikipedia.org/api/rest_v1/page/summary/{}", path_segment(title)), &[("redirect", "true")]);
    let mut body = summary(text)?;
    if body.is_none() {
        // Not a title: ask the search for the closest one.
        let found = get(
            &format!("https://{lang}.wikipedia.org/w/api.php"),
            &[("action", "opensearch"), ("search", text), ("limit", "1"), ("namespace", "0"), ("format", "json")],
        )?
        .and_then(|j| parse_opensearch(&j));
        if let Some(title) = found {
            body = summary(&title)?;
        }
    }
    let body = body.ok_or_else(|| format!("Nothing on Wikipedia for “{text}”"))?;
    parse_wikipedia(&body).ok_or_else(|| format!("Nothing on Wikipedia for “{text}”"))
}

pub fn parse_opensearch(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    v.get(1)?.get(0)?.as_str().map(String::from)
}

pub fn parse_wikipedia(json: &str) -> Option<LookupResult> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let extract = v.get("extract")?.as_str()?.trim().to_string();
    if extract.is_empty() {
        return None;
    }
    Some(LookupResult {
        title: v.get("title").and_then(|t| t.as_str()).unwrap_or_default().to_string(),
        body: extract,
        url: v.pointer("/content_urls/desktop/page").and_then(|u| u.as_str()).map(String::from),
        open: None,
    })
}

fn translate(text: &str, lang: Option<&str>, pair: &[String]) -> Result<LookupResult, String> {
    let (a, b) = match pair {
        [a, b, ..] => (a.as_str(), b.as_str()),
        [a] => (a.as_str(), "en"),
        [] => ("es", "en"),
    };
    // Into the other language of the pair: Spanish text to English, anything else to Spanish.
    let to = |source: &str| if source == a { b } else { a };
    let first_target = to(lang.unwrap_or(""));
    let call = |target: &str| {
        get(
            "https://translate.googleapis.com/translate_a/single",
            &[("client", "gtx"), ("sl", "auto"), ("tl", target), ("dt", "t"), ("q", text)],
        )
    };
    let body = call(first_target)?.ok_or("Translation is unavailable")?;
    let (mut translated, mut detected) = parse_translate(&body).ok_or("Translation is unavailable")?;
    let mut target = first_target.to_string();
    // The book's language was wrong (or missing): it was already in the target.
    if detected.as_deref() == Some(first_target) {
        let second = to(first_target);
        if let Some((t, d)) = call(second)?.and_then(|b| parse_translate(&b)) {
            translated = t;
            detected = d;
            target = second.to_string();
        }
    }
    Ok(LookupResult {
        title: format!("{} → {}", detected.unwrap_or_else(|| "auto".into()), target),
        body: translated,
        url: None,
        open: None,
    })
}

/// Google's `translate_a/single` answer: the translated text and the
/// detected source language.
pub fn parse_translate(json: &str) -> Option<(String, Option<String>)> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let text: String = v.get(0)?.as_array()?.iter().filter_map(|seg| seg.get(0)?.as_str()).collect();
    let detected = v.get(2).and_then(|d| d.as_str()).map(String::from);
    (!text.trim().is_empty()).then_some((text, detected))
}

fn command(argv: &[String], text: &str) -> Result<LookupResult, String> {
    let Some((program, args)) = argv.split_first() else {
        return Err("this provider has no command".into());
    };
    let args: Vec<String> = args.iter().map(|a| a.replace("{text}", text)).collect();
    let mut child = Command::new(program)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("could not run {program}: {e}"))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() > TIMEOUT => {
                let _ = child.kill();
                return Err(format!("{program} took too long"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(30)),
            Err(e) => return Err(e.to_string()),
        }
    }
    let mut out = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.by_ref().take(256 * 1024).read_to_string(&mut out);
    }
    let out = out.trim().to_string();
    if out.is_empty() {
        return Err(format!("{program} found nothing"));
    }
    Ok(LookupResult { title: text.to_string(), body: out, url: None, open: None })
}

fn url(template: &str, text: &str) -> Result<LookupResult, String> {
    let encoded: String = text
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect();
    let target = template.replace("{text}", &encoded);
    if !(target.starts_with("https://") || target.starts_with("http://")) {
        return Err("url providers must use http or https".into());
    }
    Ok(LookupResult { title: text.to_string(), open: Some(target), ..Default::default() })
}

/// How long a cached answer is good for.
pub const CACHE_SECS: i64 = 30 * 24 * 3600;

impl crate::db::Db {
    pub fn cached_lookup(&self, provider: &str, query: &str, now: i64) -> Option<LookupResult> {
        self.conn()
            .query_row(
                "SELECT result FROM lookup_cache WHERE provider = ?1 AND query = ?2 AND created_at > ?3",
                rusqlite::params![provider, query, now - CACHE_SECS],
                |r| r.get::<_, String>(0),
            )
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
    }

    pub fn cache_lookup(&self, provider: &str, query: &str, result: &LookupResult, now: i64) {
        let Ok(json) = serde_json::to_string(result) else { return };
        let _ = self.conn().execute(
            "INSERT INTO lookup_cache (provider, query, result, created_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(provider, query) DO UPDATE SET result = excluded.result, created_at = excluded.created_at",
            rusqlite::params![provider, query, json, now],
        );
    }
}

/// Tags out, entities decoded, whitespace collapsed.
pub fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#160;", " ")
        .replace("&amp;", "&");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wiktionary_prefers_the_book_language() {
        let json = r#"{
          "en": [{"partOfSpeech": "Noun", "language": "English", "definitions": [
            {"definition": "A <a href=\"/wiki/x\">domesticated</a> animal &amp; pet.", "examples": ["The <b>dog</b> barked."]}]}],
          "es": [{"partOfSpeech": "Noun", "language": "Spanish", "definitions": [{"definition": "perro"}]}]
        }"#;
        let r = parse_wiktionary(json, Some("es")).unwrap();
        assert!(r.body.starts_with("noun · Spanish"), "{}", r.body);
        assert!(r.body.contains("A domesticated animal & pet."));
        assert!(r.body.contains("“The dog barked.”"));
        assert_eq!(parse_wiktionary("{}", None), None);
    }

    #[test]
    fn wikipedia_summary() {
        let json = r#"{"title":"Go (programming language)","extract":"Go is a language.","content_urls":{"desktop":{"page":"https://en.wikipedia.org/wiki/Go"}}}"#;
        let r = parse_wikipedia(json).unwrap();
        assert_eq!(r.title, "Go (programming language)");
        assert_eq!(r.url.as_deref(), Some("https://en.wikipedia.org/wiki/Go"));
        assert_eq!(parse_opensearch(r#"["goroutine",["Coroutine"],[""],["https://x"]]"#).as_deref(), Some("Coroutine"));
    }

    #[test]
    fn google_translate_answer() {
        let json = r#"[[["Hola mundo. ","Hello world. ",null,null,10],["Adiós","Bye",null,null,3]],null,"en",null,null,null,1]"#;
        assert_eq!(parse_translate(json), Some(("Hola mundo. Adiós".into(), Some("en".into()))));
    }

    #[test]
    fn commands_run_without_a_shell() {
        let spec = ProviderSpec {
            kind: "command".into(),
            command: vec!["echo".into(), "{text}".into()],
            ..default_providers()[0].clone()
        };
        // Shell syntax stays text: nothing is expanded or run.
        let r = run(&spec, "$(touch /tmp/omaread-should-not-exist); ok", None).unwrap();
        assert_eq!(r.body, "$(touch /tmp/omaread-should-not-exist); ok");
        assert!(!std::path::Path::new("/tmp/omaread-should-not-exist").exists());
    }

    #[test]
    fn url_providers_encode_and_refuse_other_schemes() {
        let spec = ProviderSpec { kind: "url".into(), url: "https://duckduckgo.com/?q={text}".into(), ..default_providers()[0].clone() };
        assert_eq!(run(&spec, "a b&c", None).unwrap().open.as_deref(), Some("https://duckduckgo.com/?q=a%20b%26c"));
        let bad = ProviderSpec { url: "file:///etc/{text}".into(), ..spec };
        assert!(run(&bad, "passwd", None).is_err());
    }

    #[test]
    fn answers_are_cached_for_a_while() {
        let db = crate::db::Db::in_memory().unwrap();
        let r = LookupResult { title: "t".into(), body: "b".into(), ..Default::default() };
        db.cache_lookup("Dictionary", "dog", &r, 1000);
        assert_eq!(db.cached_lookup("Dictionary", "dog", 1001), Some(r));
        assert_eq!(db.cached_lookup("Dictionary", "dog", 1000 + CACHE_SECS + 1), None);
        assert_eq!(db.cached_lookup("Wikipedia", "dog", 1001), None);
    }

    #[test]
    fn languages_reduce_to_their_primary_tag() {
        assert_eq!(primary_language("en-US"), "en");
        assert_eq!(primary_language("es_CO"), "es");
    }
}

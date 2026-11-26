//! Lookups answered by a coding agent's CLI: Claude Code, Codex or OpenCode,
//! run once per question with no tools and no project around it.
//!
//! The selection and its passage come from the book, which is untrusted
//! text: a book can carry instructions aimed at the agent. So no agent gets
//! tools that act. Claude Code runs with `--tools ""`, Codex in its
//! read-only sandbox, and OpenCode with a project config that denies every
//! permission. Agents that can't be held to that aren't built in.
//!
//! Off until the config turns it on:
//!
//! ```toml
//! [llm]
//! enabled = true
//! agent = "claude"        # claude, codex, opencode, or one of yours
//! key = "e"               # adds an "Explain" lookup on this key
//!
//! [llm.claude]
//! model = "haiku"
//!
//! [llm.codex]
//! model = "gpt-5-codex-mini"
//!
//! [llm.mine]              # any other program: the prompt goes to stdin,
//! command = ["my-llm", "--model", "{model}"]   # or where {prompt} is
//! model = "small"
//! ```
//!
//! More `kind = "llm"` providers can be listed under `[[lookup.provider]]`,
//! each with its own `agent`, `model` and `prompt`.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::lookup::{LookupContext, LookupResult, ProviderSpec};

/// Agents usually answer in seconds; one that loads many MCP servers can
/// take half a minute before it starts.
const DEFAULT_TIMEOUT: u64 = 120;
/// Characters of the chapter so far sent with `context = "chapter"`: a few
/// pages, enough to know who and what the passage is about.
const DEFAULT_MAX_CONTEXT: usize = 12_000;
/// The ceiling for `max_context`: OpenCode takes the prompt as one argument,
/// which Linux caps at 128 KiB.
const MAX_CONTEXT_CEILING: usize = 100_000;
/// Answers longer than this are cut.
const MAX_ANSWER: u64 = 256 * 1024;

pub const DEFAULT_PROMPT: &str = "You are helping someone who is reading a book. Explain the selected text in plain language, \
in at most three short paragraphs. Use the passage around it for context. If it names a person, place, work or term, say who \
or what it is and why it matters here. Do not reveal anything that happens later in the book. Answer in {language}. Reply \
with plain text only: no Markdown, no headings, no preamble.

Book: {title}{by}
Chapter: {chapter}

{earlier}Passage:
{passage}

Selected text:
{text}";

/// `[llm]` in config.toml.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    /// Nothing runs unless this is true.
    pub enabled: bool,
    /// The agent the `key` lookup (and `llm` providers without one) use.
    pub agent: String,
    /// The key for the built-in "Explain" lookup; empty for none.
    pub key: String,
    /// What that lookup is called on the card.
    pub name: String,
    /// Seconds before an agent is given up on.
    pub timeout: u64,
    /// The prompt, with {text}, {passage}, {title}, {authors}, {by},
    /// {chapter} and {language}.
    pub prompt: String,
    /// The language answers are in ("English"); else the text's own.
    pub language: String,
    /// How much of the book goes with a question: `passage` (the paragraph
    /// around the selection) or `chapter` (also the chapter so far, from
    /// its start up to the selection).
    pub context: String,
    /// The most characters of the chapter so far to send, its end kept.
    pub max_context: usize,
    /// `[llm.<agent>]`: a model, and for agents of your own, the command.
    #[serde(flatten)]
    pub agents: BTreeMap<String, AgentConfig>,
}

impl Default for LlmConfig {
    fn default() -> LlmConfig {
        LlmConfig {
            enabled: false,
            agent: "claude".into(),
            key: "e".into(),
            name: "Explain".into(),
            timeout: DEFAULT_TIMEOUT,
            prompt: String::new(),
            language: String::new(),
            context: "passage".into(),
            max_context: DEFAULT_MAX_CONTEXT,
            agents: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    /// Passed as the agent's `--model` (or `{model}` in a command).
    pub model: String,
    /// The program, when it isn't on the app's PATH (an absolute path).
    pub program: String,
    /// For agents of your own: the program and its arguments. The prompt
    /// goes to stdin unless an argument is `{prompt}`.
    pub command: Vec<String>,
}

impl LlmConfig {
    /// The provider the `key` adds, if it adds one.
    pub fn provider(&self) -> Option<ProviderSpec> {
        (self.enabled && !self.key.trim().is_empty()).then(|| ProviderSpec {
            name: if self.name.trim().is_empty() { "Explain".into() } else { self.name.clone() },
            kind: "llm".into(),
            key: self.key.trim().to_string(),
            ..ProviderSpec::default()
        })
    }
}

/// How the prompt reaches the program.
#[derive(Debug, Clone, PartialEq)]
enum Via {
    Stdin,
    /// Already in the arguments.
    Argument,
}

/// The command line for an agent. Built-in agents run once, without
/// tools, sessions or MCP servers where their CLI allows it.
fn command_line(agent: &str, config: &AgentConfig, prompt: &str) -> Result<(Vec<String>, Via), String> {
    let model = config.model.trim();
    let program = |name: &str| if config.program.trim().is_empty() { name.to_string() } else { config.program.trim().to_string() };
    let with_model = |mut argv: Vec<String>, flag: &str| {
        if !model.is_empty() {
            argv.push(flag.into());
            argv.push(model.into());
        }
        argv
    };
    let strings = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    if !config.command.is_empty() {
        let mut via = Via::Stdin;
        let argv = config
            .command
            .iter()
            .map(|a| {
                if a.contains("{prompt}") {
                    via = Via::Argument;
                }
                a.replace("{model}", model).replace("{prompt}", prompt)
            })
            .collect();
        return Ok((argv, via));
    }
    match agent {
        "claude" | "claude-code" => {
            let argv = [program("claude")].into_iter().chain(strings(&[
                "-p",
                "--tools",
                "",
                "--no-session-persistence",
                "--strict-mcp-config",
                "--disable-slash-commands",
            ]));
            Ok((with_model(argv.collect(), "--model"), Via::Stdin))
        }
        "codex" => {
            let argv = [program("codex")]
                .into_iter()
                .chain(strings(&["exec", "--skip-git-repo-check", "--ephemeral", "--sandbox", "read-only", "--color", "never"]));
            let mut argv = with_model(argv.collect(), "--model");
            // `-` reads the prompt from stdin.
            argv.push("-".into());
            Ok((argv, Via::Stdin))
        }
        "opencode" => {
            let argv = with_model(vec![program("opencode"), "run".into()], "--model");
            let mut argv = argv;
            argv.push("--".into());
            argv.push(prompt.into());
            Ok((argv, Via::Argument))
        }
        other => Err(format!("no agent called {other}: use claude, codex or opencode, or give [llm.{other}] a command")),
    }
}

/// A language tag as a name the model reads without guessing.
fn language_name(tag: &str) -> String {
    let primary = tag.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
    let name = match primary.as_str() {
        "en" => "English",
        "es" => "Spanish",
        "fr" => "French",
        "de" => "German",
        "it" => "Italian",
        "pt" => "Portuguese",
        "nl" => "Dutch",
        "ca" => "Catalan",
        "la" => "Latin",
        "el" | "grc" => "Greek",
        "ru" => "Russian",
        "ja" => "Japanese",
        "zh" => "Chinese",
        "" => return "the language of the selected text".into(),
        _ => return format!("the language with the tag {tag}"),
    };
    name.into()
}

/// The end of the chapter so far, at most `max` characters, starting at a
/// paragraph (or at least a word) so it doesn't open mid-word.
pub fn earlier_text(before: &str, max: usize) -> String {
    let before = before.trim();
    let count = before.chars().count();
    if count <= max {
        return before.to_string();
    }
    let tail: String = before.chars().skip(count - max).collect();
    let start = tail.find("\n\n").map(|i| i + 2).or_else(|| tail.find(char::is_whitespace).map(|i| i + 1)).unwrap_or(0);
    format!("…{}", tail[start..].trim_start())
}

/// Fill in the prompt's placeholders. `language` is the answer's, if the
/// config names one; `earlier` is the chapter so far, or empty.
pub fn prompt(template: &str, text: &str, ctx: &LookupContext, language: &str, earlier: &str) -> String {
    let template = if template.trim().is_empty() { DEFAULT_PROMPT } else { template };
    let passage = if ctx.passage.trim().is_empty() { text } else { ctx.passage.trim() };
    let by = if ctx.authors.is_empty() { String::new() } else { format!(" by {}", ctx.authors.join(", ")) };
    template
        .replace("{title}", if ctx.title.is_empty() { "(unknown)" } else { &ctx.title })
        .replace("{authors}", &ctx.authors.join(", "))
        .replace("{by}", &by)
        .replace("{chapter}", if ctx.chapter.is_empty() { "(unknown)" } else { &ctx.chapter })
        .replace("{language}", &if language.trim().is_empty() { language_name(&ctx.language) } else { language.trim().to_string() })
        .replace("{earlier}", &if earlier.is_empty() { String::new() } else { format!("Earlier in the chapter, up to the selection:\n{earlier}\n\n") })
        .replace("{before}", earlier)
        .replace("{passage}", passage)
        .replace("{text}", text)
}

/// Ask the agent. Blocking: call it off the UI thread.
pub fn run(spec: &ProviderSpec, text: &str, ctx: &LookupContext) -> Result<LookupResult, String> {
    let Some(config) = spec.llm.as_ref().filter(|c| c.enabled) else {
        return Err("LLM lookups are off. Set enabled = true under [llm] in ~/.config/omaread/config.toml".into());
    };
    let agent = if spec.agent.trim().is_empty() { config.agent.trim() } else { spec.agent.trim() };
    let mut agent_config = config.agents.get(agent).cloned().unwrap_or_default();
    if !spec.model.trim().is_empty() {
        agent_config.model = spec.model.trim().to_string();
    }
    let template = if spec.prompt.trim().is_empty() { &config.prompt } else { &spec.prompt };
    let earlier = match config.context.trim() {
        "chapter" => earlier_text(&ctx.before, config.max_context.min(MAX_CONTEXT_CEILING)),
        _ => String::new(),
    };
    let prompt = prompt(template, text, ctx, &config.language, &earlier);
    let (argv, via) = command_line(agent, &agent_config, &prompt)?;
    let timeout = Duration::from_secs(if config.timeout == 0 { DEFAULT_TIMEOUT } else { config.timeout });
    let answer = execute(&argv, via, &prompt, timeout)?;
    let model = if agent_config.model.is_empty() { String::new() } else { format!(" ({})", agent_config.model) };
    Ok(LookupResult { title: format!("{}{model}", agent_name(agent)), body: answer, url: None, open: None })
}

fn agent_name(agent: &str) -> &str {
    match agent {
        "claude" | "claude-code" => "Claude Code",
        "codex" => "Codex",
        "opencode" => "OpenCode",
        other => other,
    }
}

/// OpenCode's project config: every tool and permission denied, read last
/// so it overrides the user's own.
const OPENCODE_DENY_ALL: &str = r#"{"$schema":"https://opencode.ai/config.json","permissions":[{"action":"*","resource":"*","effect":"deny"}]}"#;

/// A fresh, private, empty directory to run in (mode 0700, a random name),
/// so the agent picks up no project's files or instructions and nobody else
/// can plant any; for OpenCode, it holds the config that takes its tools.
/// Removed when dropped.
fn workdir(program: &str) -> Result<tempfile::TempDir, String> {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::Builder::new()
        .prefix("omaread-llm-")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .map_err(|e| format!("no directory to run the agent in: {e}"))?;
    if Path::new(program).file_name().is_some_and(|n| n == "opencode") {
        // Without it OpenCode would run the shell for text in a book.
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.path().join("opencode.json"))
            .and_then(|mut f| f.write_all(OPENCODE_DENY_ALL.as_bytes()))
            .map_err(|e| format!("could not hold OpenCode to no tools: {e}"))?;
    }
    Ok(dir)
}

fn execute(argv: &[String], via: Via, prompt: &str, timeout: Duration) -> Result<String, String> {
    let (program, args) = argv.split_first().ok_or("the agent has no command")?;
    // Kept until the agent is done with it.
    let dir = workdir(program)?;
    let mut child = Command::new(program)
        .args(args)
        .current_dir(dir.path())
        .stdin(if via == Via::Stdin { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => format!("{program} is not installed or not on the app's PATH; set program = \"/full/path\" under its [llm.<agent>]"),
            _ => format!("could not run {program}: {e}"),
        })?;
    if via == Via::Stdin
        && let Some(mut stdin) = child.stdin.take()
    {
        let prompt = prompt.to_string();
        std::thread::spawn(move || {
            let _ = stdin.write_all(prompt.as_bytes());
        });
    }
    // Read both pipes on their own threads, so a chatty agent never blocks on a full pipe.
    let reader = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut out = String::new();
            if let Some(pipe) = pipe {
                let _ = pipe.take(MAX_ANSWER).read_to_string(&mut out);
            }
            out
        })
    };
    let stdout = reader(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let stderr = reader(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{program} gave no answer within {} seconds", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(e.to_string()),
        }
    };
    let out = stdout.join().unwrap_or_default();
    let err = stderr.join().unwrap_or_default();
    if !status.success() {
        let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
        return Err(if last.is_empty() { format!("{program} failed ({status})") } else { format!("{program} failed: {last}") });
    }
    let answer = out.trim();
    if answer.is_empty() {
        return Err(format!("{program} answered nothing"));
    }
    Ok(answer.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(model: &str) -> AgentConfig {
        AgentConfig { model: model.into(), ..AgentConfig::default() }
    }

    #[test]
    fn built_in_agents_run_once_without_tools() {
        let (argv, via) = command_line("claude", &agent("haiku"), "q").unwrap();
        assert_eq!(argv[0], "claude");
        assert!(argv.windows(2).any(|w| w == ["--tools", ""]));
        assert!(argv.windows(2).any(|w| w == ["--model", "haiku"]));
        assert_eq!(via, Via::Stdin);

        let (argv, _) = command_line("codex", &agent(""), "q").unwrap();
        assert_eq!(&argv[..2], ["codex", "exec"]);
        assert!(!argv.contains(&"--model".to_string()));
        assert_eq!(argv.last().map(String::as_str), Some("-"));

        let (argv, via) = command_line("opencode", &agent("anthropic/claude-haiku-4-5"), "-rf q").unwrap();
        assert_eq!(&argv[argv.len() - 2..], ["--", "-rf q"]);
        assert_eq!(via, Via::Argument);

        assert!(command_line("nonesuch", &agent(""), "q").is_err());
        assert!(command_line("gemini", &agent(""), "q").is_err());
    }

    #[test]
    fn a_command_of_your_own_takes_the_model_and_maybe_the_prompt() {
        let config = AgentConfig { model: "small".into(), program: String::new(), command: vec!["my-llm".into(), "-m".into(), "{model}".into()] };
        assert_eq!(command_line("mine", &config, "q").unwrap(), (vec!["my-llm".into(), "-m".into(), "small".into()], Via::Stdin));
        let config = AgentConfig { command: vec!["ask".into(), "{prompt}".into()], ..AgentConfig::default() };
        assert_eq!(command_line("mine", &config, "hello").unwrap(), (vec!["ask".into(), "hello".into()], Via::Argument));
    }

    #[test]
    fn the_prompt_carries_the_book_and_passage() {
        let ctx = LookupContext {
            title: "Dune".into(),
            authors: vec!["Frank Herbert".into()],
            chapter: "Book One".into(),
            passage: "The spice must flow.".into(),
            language: "en-US".into(),
            before: String::new(),
        };
        let p = prompt("", "spice", &ctx, "", "");
        assert!(!p.contains("Earlier in the chapter"));
        let p2 = prompt("", "spice", &ctx, "", "Paul arrives on Arrakis.");
        assert!(p2.contains("Earlier in the chapter, up to the selection:\nPaul arrives on Arrakis.\n\nPassage:"));
        assert!(p.contains("Answer in English."));
        assert!(prompt("", "spice", &ctx, "Spanish", "").contains("Answer in Spanish."));
        assert!(p.contains("Book: Dune by Frank Herbert"));
        assert!(p.contains("Chapter: Book One"));
        assert!(p.contains("Passage:\nThe spice must flow."));
        assert!(p.ends_with("Selected text:\nspice"));
        assert_eq!(prompt("{text} in {title}", "x", &LookupContext::default(), "", ""), "x in (unknown)");
    }

    #[test]
    fn opencode_runs_beside_a_config_that_denies_every_tool() {
        let dir = workdir("/usr/bin/opencode").unwrap();
        let config = std::fs::read_to_string(dir.path().join("opencode.json")).unwrap();
        let json: serde_json::Value = serde_json::from_str(&config).unwrap();
        assert_eq!(json["permissions"][0], serde_json::json!({ "action": "*", "resource": "*", "effect": "deny" }));
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777, 0o700);
        }
        let other = workdir("claude").unwrap();
        assert_ne!(dir.path(), other.path());
        assert!(!other.path().join("opencode.json").exists());
        let path = other.path().to_path_buf();
        drop(other);
        assert!(!path.exists());
    }

    #[test]
    fn the_chapter_so_far_keeps_its_end_and_starts_at_a_paragraph() {
        assert_eq!(earlier_text("  short  ", 100), "short");
        let text = "First paragraph here.\n\nSecond one.\n\nThird and last.";
        assert_eq!(earlier_text(text, 20), "…Third and last.");
        assert_eq!(earlier_text("one two three four", 9), "…four");
    }

    #[test]
    fn chapter_context_goes_in_only_when_asked_for() {
        let mut config = LlmConfig { enabled: true, agent: "cat".into(), ..LlmConfig::default() };
        config.agents.insert("cat".into(), AgentConfig { command: vec!["cat".into()], ..AgentConfig::default() });
        let ctx = LookupContext { before: "Earlier words.".into(), ..LookupContext::default() };
        let spec = ProviderSpec { kind: "llm".into(), prompt: "[{before}] {text}".into(), llm: Some(config.clone()), ..ProviderSpec::default() };
        assert_eq!(run(&spec, "x", &ctx).unwrap().body, "[] x");
        config.context = "chapter".into();
        let spec = ProviderSpec { llm: Some(config), ..spec };
        assert_eq!(run(&spec, "x", &ctx).unwrap().body, "[Earlier words.] x");
    }

    #[test]
    fn nothing_runs_unless_enabled() {
        let spec = ProviderSpec { kind: "llm".into(), ..ProviderSpec::default() };
        assert!(run(&spec, "x", &LookupContext::default()).unwrap_err().contains("off"));
        let off = LlmConfig::default();
        assert!(off.provider().is_none());
        let on = LlmConfig { enabled: true, ..LlmConfig::default() };
        assert_eq!(on.provider().map(|p| p.key), Some("e".into()));
    }

    #[test]
    fn a_program_that_answers_is_heard_and_one_that_fails_is_reported() {
        let ctx = LookupContext::default();
        let mut config = LlmConfig { enabled: true, agent: "cat".into(), ..LlmConfig::default() };
        config.agents.insert("cat".into(), AgentConfig { command: vec!["cat".into()], ..AgentConfig::default() });
        let spec = ProviderSpec { kind: "llm".into(), prompt: "echo {text}".into(), llm: Some(config.clone()), ..ProviderSpec::default() };
        assert_eq!(run(&spec, "hi", &ctx).unwrap().body, "echo hi");

        config.agents.insert("cat".into(), AgentConfig { command: vec!["false".into()], ..AgentConfig::default() });
        let spec = ProviderSpec { llm: Some(config), ..spec };
        assert!(run(&spec, "hi", &ctx).unwrap_err().contains("failed"));
    }
}

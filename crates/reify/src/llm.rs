//! Optional model assistance — the last resort, never the first.
//!
//! ## Why an external command rather than an HTTP client
//!
//! Reify's privacy claim is that it opens no network connection, and that claim is
//! enforced structurally by `tests/offline.rs`. Embedding an HTTP client would delete
//! that guarantee for every user, including the overwhelming majority who never enable
//! a model.
//!
//! So the provider is **a command the user configures**. Reify writes the prompt to its
//! stdin — or substitutes it for a `{prompt}` placeholder in the arguments, since many
//! model CLIs take the prompt as an argument — and reads the completion from stdout.
//! That buys four things an embedded client would not:
//!
//! - the offline guarantee stays literally true, and testable;
//! - a local model (`ollama run`, `llama-cli`) works with no extra code;
//! - the user can inspect, log or refuse any request by wrapping the command;
//! - no credential ever passes through Reify.
//!
//! ## What is guaranteed
//!
//! 1. Disabled unless explicitly configured. There is no default provider.
//! 2. `REIFY_OFFLINE=1` makes it unreachable regardless of configuration.
//! 3. Every call is appended to `.reify/llm.log` with the input hash and byte count.
//! 4. Every result is cached by input hash, so the same knowledge is never paid for
//!    twice.
//! 5. Output is always recorded as [`Status::Inferred`] with the retrieved facts as
//!    its evidence. A model may phrase; it may not assert.

use anyhow::{anyhow, Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::model::Status;

/// Configuration file, relative to `.reify/`.
pub const CONFIG_FILE: &str = "llm.toml";
/// Audit log, relative to `.reify/`.
pub const LOG_FILE: &str = "llm.log";

/// Environment variable that makes model use unreachable.
pub const OFFLINE_ENV: &str = "REIFY_OFFLINE";
/// Environment variable that configures a provider without a config file.
pub const COMMAND_ENV: &str = "REIFY_LLM_COMMAND";

/// Bumped whenever a prompt's wording changes, so cached output from the old wording
/// is invalidated rather than silently reused.
pub const PROMPT_VERSION: &str = "synthesis-v1";

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// A configured completion provider.
#[derive(Debug, Clone)]
pub struct Provider {
    /// Argv. The first element is the program.
    ///
    /// If any argument contains `{prompt}`, the prompt is substituted there and stdin
    /// is left closed. Otherwise the prompt is written to stdin. Both shapes are
    /// common among model CLIs and neither should require a wrapper script.
    pub command: Vec<String>,
    /// Recorded in provenance so a cached artifact can be traced to what produced it.
    pub label: String,
    pub timeout: Duration,
}

/// Marker substituted with the prompt when a provider takes it as an argument.
pub const PROMPT_PLACEHOLDER: &str = "{prompt}";

impl Provider {
    /// Does this provider take the prompt as an argument rather than on stdin?
    pub fn uses_placeholder(&self) -> bool {
        self.command
            .iter()
            .any(|arg| arg.contains(PROMPT_PLACEHOLDER))
    }

    /// The argv to execute for `prompt`.
    pub fn argv(&self, prompt: &str) -> Vec<String> {
        self.command
            .iter()
            .map(|arg| arg.replace(PROMPT_PLACEHOLDER, prompt))
            .collect()
    }
}

/// Why model assistance is unavailable, in words a user can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unavailable {
    /// `REIFY_OFFLINE=1` is set.
    Offline,
    /// No provider has been configured.
    NotConfigured,
    /// A provider is configured but its configuration is unusable.
    Misconfigured(String),
}

impl Unavailable {
    pub fn explain(&self) -> String {
        match self {
            Unavailable::Offline => format!(
                "{OFFLINE_ENV}=1 is set, so no model can be called. Unset it to enable \
                 model assistance."
            ),
            Unavailable::NotConfigured => format!(
                "No model provider is configured, and there is no default. Set \
                 {COMMAND_ENV}, or write .reify/{CONFIG_FILE}:\n\
                 \n    command = [\"ollama\", \"run\", \"llama3\"]\n\
                 \nReify writes the prompt to the command's stdin and reads the \
                 completion from its stdout."
            ),
            Unavailable::Misconfigured(why) => {
                format!("The model provider is misconfigured: {why}")
            }
        }
    }
}

/// Resolve the provider for a repository, or explain precisely why there is none.
///
/// The offline check comes first and cannot be overridden by configuration: a user who
/// has set the environment variable has stated an intent that outranks a config file
/// they may have forgotten about.
pub fn provider(root: &Path) -> std::result::Result<Provider, Unavailable> {
    if std::env::var(OFFLINE_ENV).is_ok_and(|v| v != "0" && !v.is_empty()) {
        return Err(Unavailable::Offline);
    }
    if let Ok(raw) = std::env::var(COMMAND_ENV) {
        let command = split_command(&raw);
        return if command.is_empty() {
            Err(Unavailable::Misconfigured(format!(
                "{COMMAND_ENV} is empty"
            )))
        } else {
            Ok(Provider {
                label: command.join(" "),
                command,
                timeout: DEFAULT_TIMEOUT,
            })
        };
    }

    let path = config_path(root);
    if !path.exists() {
        return Err(Unavailable::NotConfigured);
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| Unavailable::Misconfigured(format!("reading {}: {e}", path.display())))?;
    parse_config(&text).map_err(|e| Unavailable::Misconfigured(e.to_string()))
}

pub fn config_path(root: &Path) -> PathBuf {
    root.join(crate::index::REIFY_DIR).join(CONFIG_FILE)
}

pub fn log_path(root: &Path) -> PathBuf {
    root.join(crate::index::REIFY_DIR).join(LOG_FILE)
}

fn parse_config(text: &str) -> Result<Provider> {
    #[derive(serde::Deserialize)]
    struct Raw {
        command: Vec<String>,
        #[serde(default)]
        timeout_seconds: Option<u64>,
    }
    let raw: Raw = toml::from_str(text).context("parsing the provider configuration")?;
    if raw.command.is_empty() {
        return Err(anyhow!("`command` must name a program to run"));
    }
    Ok(Provider {
        label: raw.command.join(" "),
        command: raw.command,
        timeout: raw
            .timeout_seconds
            .map(Duration::from_secs)
            .unwrap_or(DEFAULT_TIMEOUT),
    })
}

/// Split a shell-ish command string into argv.
///
/// Handles quoted arguments; deliberately does not implement a shell, because Reify
/// must never introduce shell interpretation into a path a user's data flows through.
fn split_command(raw: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for ch in raw.chars() {
        match ch {
            '"' | '\'' if quote == Some(ch) => quote = None,
            '"' | '\'' if quote.is_none() => quote = Some(ch),
            c if c.is_whitespace() && quote.is_none() => {
                if !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

/// Exactly what would be sent, for `reify llm preview`.
///
/// Not an approximation: this is the same string [`complete`] writes to the provider's
/// stdin. A user must be able to read it before any byte leaves their machine.
pub fn preview(prompt: &str) -> String {
    prompt.to_string()
}

/// The hash a completion is cached and audited under.
pub fn input_hash(provider: &Provider, prompt: &str) -> String {
    blake3::hash(format!("{PROMPT_VERSION}\u{0}{}\u{0}{prompt}", provider.label).as_bytes())
        .to_hex()
        .to_string()
}

/// Run the provider, bounded, and append an audit record.
///
/// Errors are plain failures rather than panics: model assistance is the last resort,
/// and every caller must already have a deterministic answer to fall back to.
pub fn complete(provider: &Provider, root: &Path, prompt: &str) -> Result<String> {
    let hash = input_hash(provider, prompt);
    let started = Instant::now();

    let argv = provider.argv(prompt);
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| anyhow!("the provider command is empty"))?;
    let uses_placeholder = provider.uses_placeholder();

    let mut child = Command::new(program)
        .args(args)
        .stdin(if uses_placeholder {
            Stdio::null()
        } else {
            Stdio::piped()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("spawning the model provider `{program}`"))?;

    if !uses_placeholder {
        child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("the provider closed its stdin"))?
            .write_all(prompt.as_bytes())
            .context("writing the prompt to the provider")?;
    }

    loop {
        match child.try_wait()? {
            Some(status) if !status.success() => {
                audit(root, &hash, prompt.len(), 0, started, "failed");
                return Err(anyhow!("the model provider exited with {status}"));
            }
            Some(_) => {
                let output = child.wait_with_output()?;
                let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
                audit(root, &hash, prompt.len(), text.len(), started, "ok");
                return Ok(text);
            }
            None if started.elapsed() >= provider.timeout => {
                let _ = child.kill();
                let _ = child.wait();
                audit(root, &hash, prompt.len(), 0, started, "timeout");
                return Err(anyhow!(
                    "the model provider exceeded its {}s budget",
                    provider.timeout.as_secs()
                ));
            }
            None => std::thread::sleep(Duration::from_millis(25)),
        }
    }
}

/// Append one line to the audit log. Failure to log is never fatal, but it is the
/// only record a user has, so it is attempted for every call including failures.
fn audit(root: &Path, hash: &str, sent: usize, received: usize, started: Instant, outcome: &str) {
    let line = format!(
        "{hash}\t{outcome}\tsent={sent}B\treceived={received}B\telapsed={}ms\n",
        started.elapsed().as_millis()
    );
    let path = log_path(root);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(line.as_bytes()));
}

/// The status any model-derived claim must carry. There is no path to a stronger one.
pub const DERIVED_STATUS: Status = Status::Inferred;

/// Build the synthesis prompt.
///
/// Constrained on purpose: the model receives retrieved facts and is told to summarise
/// *only* those. It is not asked what it knows, because what it knows about a private
/// codebase is exactly the thing that must not enter the answer.
pub fn synthesis_prompt(task: &str, facts: &[String]) -> String {
    let mut prompt = String::with_capacity(512 + facts.iter().map(String::len).sum::<usize>());
    prompt.push_str(
        "You are summarising retrieved facts about a codebase for another engineer.\n\
         Rules:\n\
         - Use ONLY the facts listed below. Do not add anything from prior knowledge.\n\
         - If the facts do not answer the task, say exactly what is missing.\n\
         - Be concise: at most six sentences.\n\
         - Refer to files and symbols by the exact identifiers given.\n\n",
    );
    prompt.push_str("TASK: ");
    prompt.push_str(task);
    prompt.push_str("\n\nFACTS:\n");
    for fact in facts {
        prompt.push_str("- ");
        prompt.push_str(fact);
        prompt.push('\n');
    }
    prompt.push_str("\nSUMMARY:\n");
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Environment mutation is process-global, so these tests hold a lock rather than
    /// racing each other under the test harness's thread pool.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_env<T>(vars: &[(&str, Option<&str>)], f: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let saved: Vec<(String, Option<String>)> = vars
            .iter()
            .map(|(k, _)| (k.to_string(), std::env::var(k).ok()))
            .collect();
        for (key, value) in vars {
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
        let result = f();
        for (key, value) in saved {
            match value {
                Some(v) => std::env::set_var(&key, v),
                None => std::env::remove_var(&key),
            }
        }
        result
    }

    #[test]
    fn model_use_is_off_unless_explicitly_configured() {
        with_env(&[(OFFLINE_ENV, None), (COMMAND_ENV, None)], || {
            let err = provider(Path::new("/nonexistent")).unwrap_err();
            assert_eq!(err, Unavailable::NotConfigured);
            assert!(err.explain().contains("no default"));
        });
    }

    #[test]
    fn offline_mode_overrides_any_configuration() {
        // The safety property: a user who set the variable outranks a config file they
        // may have forgotten about.
        with_env(
            &[(OFFLINE_ENV, Some("1")), (COMMAND_ENV, Some("echo hi"))],
            || {
                assert_eq!(
                    provider(Path::new("/tmp")).unwrap_err(),
                    Unavailable::Offline
                );
            },
        );
    }

    #[test]
    fn offline_set_to_zero_does_not_count_as_offline() {
        with_env(
            &[(OFFLINE_ENV, Some("0")), (COMMAND_ENV, Some("echo hi"))],
            || {
                assert!(provider(Path::new("/tmp")).is_ok());
            },
        );
    }

    #[test]
    fn a_provider_can_be_configured_from_the_environment() {
        with_env(
            &[
                (OFFLINE_ENV, None),
                (COMMAND_ENV, Some("ollama run llama3")),
            ],
            || {
                let p = provider(Path::new("/tmp")).unwrap();
                assert_eq!(p.command, vec!["ollama", "run", "llama3"]);
            },
        );
    }

    #[test]
    fn command_splitting_respects_quotes_without_being_a_shell() {
        assert_eq!(
            split_command("llm -m 'gpt sized'"),
            vec!["llm", "-m", "gpt sized"]
        );
        assert_eq!(split_command("  spaced   out  "), vec!["spaced", "out"]);
        assert!(split_command("").is_empty());
    }

    #[test]
    fn configuration_is_parsed_from_toml() {
        let p = parse_config("command = [\"ollama\", \"run\", \"llama3\"]\ntimeout_seconds = 5")
            .unwrap();
        assert_eq!(p.command.len(), 3);
        assert_eq!(p.timeout, Duration::from_secs(5));
        assert!(parse_config("command = []").is_err());
    }

    #[test]
    fn the_preview_is_byte_identical_to_what_would_be_sent() {
        // If these could differ, the privacy promise would be unverifiable.
        let prompt = synthesis_prompt("do a thing", &["a fact".into()]);
        assert_eq!(preview(&prompt), prompt);
    }

    #[test]
    fn the_prompt_forbids_the_model_from_adding_knowledge() {
        let prompt = synthesis_prompt("task", &["fact one".into()]);
        assert!(prompt.contains("ONLY the facts"));
        assert!(prompt.contains("Do not add anything from prior knowledge"));
        assert!(prompt.contains("fact one"));
    }

    #[test]
    fn the_input_hash_changes_with_the_prompt_version_and_provider() {
        let a = Provider {
            command: vec!["a".into()],
            label: "a".into(),
            timeout: DEFAULT_TIMEOUT,
        };
        let b = Provider {
            command: vec!["b".into()],
            label: "b".into(),
            timeout: DEFAULT_TIMEOUT,
        };
        assert_ne!(input_hash(&a, "x"), input_hash(&b, "x"));
        assert_ne!(input_hash(&a, "x"), input_hash(&a, "y"));
        assert_eq!(input_hash(&a, "x"), input_hash(&a, "x"));
    }

    #[test]
    fn anything_a_model_produces_is_only_ever_inferred() {
        assert_eq!(DERIVED_STATUS, Status::Inferred);
        assert!(!DERIVED_STATUS.is_actionable());
    }

    #[test]
    fn a_provider_taking_the_prompt_as_an_argument_is_supported() {
        // Many model CLIs take the prompt as an argument; requiring a wrapper script
        // would push users toward pasting credentials into one.
        let p = Provider {
            command: vec!["llm".into(), "ask".into(), PROMPT_PLACEHOLDER.into()],
            label: "llm".into(),
            timeout: DEFAULT_TIMEOUT,
        };
        assert!(p.uses_placeholder());
        assert_eq!(p.argv("hello"), vec!["llm", "ask", "hello"]);

        let stdin_provider = Provider {
            command: vec!["cat".into()],
            label: "cat".into(),
            timeout: DEFAULT_TIMEOUT,
        };
        assert!(!stdin_provider.uses_placeholder());
        assert_eq!(stdin_provider.argv("hello"), vec!["cat"]);
    }

    #[test]
    fn an_argument_provider_completes_without_using_stdin() {
        let dir = std::env::temp_dir().join(format!("reify-llm-arg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(crate::index::REIFY_DIR)).unwrap();
        let p = Provider {
            command: vec!["echo".into(), PROMPT_PLACEHOLDER.into()],
            label: "echo".into(),
            timeout: Duration::from_secs(5),
        };
        assert_eq!(complete(&p, &dir, "hello facts").unwrap(), "hello facts");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_completion_runs_the_configured_command_and_is_audited() {
        let dir = std::env::temp_dir().join(format!("reify-llm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(crate::index::REIFY_DIR)).unwrap();

        // `cat` is a perfectly good stand-in provider: it echoes its stdin.
        let p = Provider {
            command: vec!["cat".into()],
            label: "cat".into(),
            timeout: Duration::from_secs(5),
        };
        let out = complete(&p, &dir, "hello facts").unwrap();
        assert_eq!(out, "hello facts");

        let log = std::fs::read_to_string(log_path(&dir)).unwrap();
        assert!(log.contains("ok"), "{log}");
        assert!(log.contains("sent=11B"), "{log}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_provider_that_hangs_is_abandoned_and_audited() {
        let dir = std::env::temp_dir().join(format!("reify-llm-hang-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(crate::index::REIFY_DIR)).unwrap();
        let p = Provider {
            command: vec!["sleep".into(), "30".into()],
            label: "sleep".into(),
            timeout: Duration::from_millis(150),
        };
        let started = Instant::now();
        assert!(complete(&p, &dir, "x").is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(std::fs::read_to_string(log_path(&dir))
            .unwrap()
            .contains("timeout"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

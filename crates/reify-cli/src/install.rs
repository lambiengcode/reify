//! `reify install`: detect the agents that are here, wire each the integration
//! `docs/integration/` recommends for it.
//!
//! # Why this installs a shell command and not an MCP server
//!
//! `docs/integration/claude-code.md` ranks the integrations cheapest first and says to
//! start at level 0, the instruction block: "an MCP server's tool schemas are re-sent on
//! every turn of every session. A CLI costs nothing until it is called. For a tool whose
//! entire purpose is reducing context, paying a per-turn tax to deliver it would be
//! self-defeating."
//!
//! That argument holds, and every agent this command can detect can run a shell command,
//! so level 0 is what gets installed by default. `--mcp` is the deliberate opt-in for
//! the client that cannot, and it says what it costs before it writes anything.
//!
//! # What it will not do
//!
//! **Nothing outside the repository.** The home directory is read as *evidence* that an
//! agent exists — `~/.claude` is Claude Code's real config location — but nothing is
//! written there. A machine-wide MCP registration cannot be undone by a per-repository
//! `reify uninit` without breaking every other repository that relies on it, and an
//! integration that cannot be reversed is one nobody should install. So the MCP entries
//! written here are the repository-scoped ones (`.mcp.json`, `.cursor/mcp.json`); for a
//! client whose only MCP config is machine-wide, the plan says so and writes the
//! instruction block instead.
//!
//! **Nothing it cannot parse.** A config that exists but does not parse is reported and
//! skipped. Overwriting it would be the one failure mode that actually costs somebody
//! their afternoon.
//!
//! **Nothing twice.** Every step checks for its own output first, so a second run is a
//! no-op and says so.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub const SCHEMA: &str = "reify.install/1";

/// The MCP server entry, written as one line so it disturbs a hand-formatted config as
/// little as possible.
const MCP_ENTRY: &str = r#""reify": { "command": "reify", "args": ["serve", "--mcp"] }"#;

/// The key under which MCP clients list their servers.
const MCP_SERVERS: &str = "mcpServers";

/// Our own key inside it.
const MCP_NAME: &str = "reify";

/// A marker that identifies our instruction block wherever it was written.
///
/// The block itself is [`crate::AGENT_INSTRUCTIONS`]; this is the substring used to
/// recognise it, and it matches what `reify init --write-agent-instructions` already
/// looks for so the two commands never double up on the same file.
const INSTRUCTION_MARKER: &str = "reify context";

/// An agent Reify knows how to wire, and the evidence it is here.
struct Known {
    name: &'static str,
    /// Paths in the repository whose presence is evidence this agent is configured here.
    repo_markers: &'static [&'static str],
    /// Paths under the user's home that are this agent's real config location.
    ///
    /// Evidence only. Nothing is ever written to any of them.
    home_markers: &'static [&'static str],
    /// A rules directory. When it exists, a dedicated file goes in it rather than
    /// appending to a shared one — a file of our own is cleanly removable.
    rules_dir: Option<(&'static str, &'static str)>,
    /// Otherwise, the instruction file the block is appended to.
    instruction_file: &'static str,
    /// This client's *repository-scoped* MCP config, if it has one.
    mcp_config: Option<&'static str>,
}

/// The agents, and what each one reads.
///
/// `AGENTS.md` is deliberately its own row rather than evidence for Codex or OpenCode:
/// it is a shared convention that a dozen tools read, and claiming a specific agent is
/// installed because a generic file exists is exactly the guess this command must not
/// make.
const KNOWN: &[Known] = &[
    Known {
        name: "Claude Code",
        repo_markers: &["CLAUDE.md", ".claude"],
        home_markers: &[".claude"],
        rules_dir: None,
        instruction_file: "CLAUDE.md",
        mcp_config: Some(".mcp.json"),
    },
    Known {
        name: "Cursor",
        repo_markers: &[".cursor", ".cursorrules"],
        home_markers: &[".cursor"],
        rules_dir: Some((".cursor/rules", "reify.mdc")),
        instruction_file: ".cursorrules",
        mcp_config: Some(".cursor/mcp.json"),
    },
    Known {
        name: "Windsurf",
        repo_markers: &[".windsurf", ".windsurfrules"],
        home_markers: &[".codeium/windsurf"],
        rules_dir: Some((".windsurf/rules", "reify.md")),
        instruction_file: ".windsurfrules",
        // Windsurf's MCP config is machine-wide only, so there is nothing repository
        // scoped to write. The instruction block is the integration here.
        mcp_config: None,
    },
    Known {
        name: "Cline",
        repo_markers: &[".clinerules"],
        home_markers: &[],
        rules_dir: Some((".clinerules", "reify.md")),
        instruction_file: ".clinerules",
        mcp_config: None,
    },
    Known {
        name: "GitHub Copilot",
        repo_markers: &[".github/copilot-instructions.md"],
        home_markers: &[],
        rules_dir: None,
        instruction_file: ".github/copilot-instructions.md",
        mcp_config: None,
    },
    Known {
        name: "Codex",
        repo_markers: &[".codex"],
        home_markers: &[".codex"],
        rules_dir: None,
        instruction_file: "AGENTS.md",
        mcp_config: None,
    },
    Known {
        name: "OpenCode",
        repo_markers: &[".opencode"],
        home_markers: &[".config/opencode"],
        rules_dir: None,
        instruction_file: "AGENTS.md",
        mcp_config: None,
    },
    Known {
        name: "Aider",
        repo_markers: &["CONVENTIONS.md", ".aider.conf.yml"],
        home_markers: &[".aider.conf.yml"],
        rules_dir: None,
        instruction_file: "CONVENTIONS.md",
        mcp_config: None,
    },
    Known {
        name: "any agent reading AGENTS.md",
        repo_markers: &["AGENTS.md"],
        home_markers: &[],
        rules_dir: None,
        instruction_file: "AGENTS.md",
        mcp_config: None,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Append the instruction block to a file the agent already reads.
    Instructions,
    /// Write a dedicated rule file into the agent's rules directory.
    RuleFile,
    /// Merge a server entry into this client's repository-scoped MCP config.
    Mcp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Not there yet; `--yes` will write it.
    Planned,
    /// Already there. A second run changes nothing.
    AlreadyPresent,
    /// The file exists and could not be parsed, so it was left alone.
    Skipped,
}

/// One thing `install` would do, and to what.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Step {
    /// Repository-relative, always with `/` separators.
    pub path: String,
    pub kind: Kind,
    /// Every agent this one write serves. More than one when two agents read the
    /// same file.
    pub agents: Vec<String>,
    /// What made Reify think those agents are here.
    pub evidence: Vec<String>,
    pub state: State,
    /// Present only when `state` is `skipped`.
    pub problem: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Plan {
    pub schema: &'static str,
    pub root: String,
    /// Whether MCP was requested, and therefore whether the per-turn cost was accepted.
    pub mcp: bool,
    /// Whether the plan was applied, or only shown.
    pub applied: bool,
    pub steps: Vec<Step>,
    /// The block to paste by hand, present when no agent was recognised.
    pub instruction_block: Option<String>,
    /// Agents installed on this machine that nothing in this repository configures.
    ///
    /// Reported rather than acted on: it explains why an agent the user knows they have
    /// is not in the plan, without pretending a home directory says anything about this
    /// repository.
    pub detected_elsewhere: Vec<String>,
}

impl Plan {
    pub fn has_work(&self) -> bool {
        self.steps.iter().any(|s| s.state == State::Planned)
    }
}

/// Build the plan without writing anything.
pub fn plan(root: &Path, mcp: bool) -> Result<Plan> {
    plan_with_home(root, mcp, home_dir().as_deref())
}

/// The same, with the home directory supplied.
///
/// Injected rather than read from the environment so the rule that home evidence never
/// triggers a write can be tested without mutating a process-wide variable that every
/// other test in this binary shares.
pub fn plan_with_home(root: &Path, mcp: bool, home: Option<&Path>) -> Result<Plan> {
    let mut steps: Vec<Step> = Vec::new();

    let mut elsewhere: Vec<String> = Vec::new();

    for agent in KNOWN {
        let mut evidence: Vec<String> = agent
            .repo_markers
            .iter()
            .filter(|m| root.join(m).exists())
            .map(|m| format!("{m} is here"))
            .collect();
        let at_home: Vec<String> = home
            .into_iter()
            .flat_map(|home| {
                agent
                    .home_markers
                    .iter()
                    .filter(move |m| home.join(m).exists())
                    .map(|m| format!("~/{m} exists"))
            })
            .collect();

        // Repository evidence is required before anything is written. `~/.cursor`
        // means this user has Cursor installed, not that this repository is worked on
        // with it — creating a `.cursorrules` on that basis is the guess this command
        // exists to avoid. Home evidence corroborates; it never triggers.
        if evidence.is_empty() {
            if !at_home.is_empty() {
                elsewhere.push(format!("{} ({})", agent.name, at_home.join(", ")));
            }
            continue;
        }
        evidence.extend(at_home);

        // MCP is registered *instead of* the instruction block where the client has a
        // repository-scoped config: an agent given both pays for the schemas every turn
        // and reads instructions telling it to use the CLI anyway.
        let target = match (mcp, agent.mcp_config) {
            (true, Some(config)) => (config.to_string(), Kind::Mcp),
            _ => match agent.rules_dir {
                Some((dir, file)) if root.join(dir).is_dir() => {
                    (format!("{dir}/{file}"), Kind::RuleFile)
                }
                _ => (agent.instruction_file.to_string(), Kind::Instructions),
            },
        };

        // Two agents reading one file is one write, credited to both.
        match steps.iter_mut().find(|s| s.path == target.0) {
            Some(existing) => {
                existing.agents.push(agent.name.to_string());
                existing.evidence.extend(evidence);
            }
            None => {
                let (state, problem) = inspect(root, &target.0, target.1);
                steps.push(Step {
                    path: target.0,
                    kind: target.1,
                    agents: vec![agent.name.to_string()],
                    evidence,
                    state,
                    problem,
                });
            }
        }
    }
    steps.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(Plan {
        schema: SCHEMA,
        root: root.display().to_string(),
        mcp,
        applied: false,
        instruction_block: steps
            .is_empty()
            .then(|| crate::AGENT_INSTRUCTIONS.trim().to_string()),
        steps,
        detected_elsewhere: elsewhere,
    })
}

/// Is this step already done, or is its target unusable?
fn inspect(root: &Path, rel: &str, kind: Kind) -> (State, Option<String>) {
    let path = root.join(rel);
    let Ok(text) = std::fs::read_to_string(&path) else {
        // Absent is the normal case: it will be created.
        return (State::Planned, None);
    };
    match kind {
        Kind::Instructions | Kind::RuleFile => {
            if text.contains(INSTRUCTION_MARKER) {
                (State::AlreadyPresent, None)
            } else {
                (State::Planned, None)
            }
        }
        Kind::Mcp => match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(value) => {
                if value
                    .get(MCP_SERVERS)
                    .and_then(|s| s.get(MCP_NAME))
                    .is_some()
                {
                    (State::AlreadyPresent, None)
                } else {
                    (State::Planned, None)
                }
            }
            // Never overwritten. A config that exists but does not parse is somebody's
            // work in progress, and clobbering it is the one outcome worth avoiding
            // above all others.
            Err(e) => (
                State::Skipped,
                Some(format!("{rel} is not valid JSON ({e}); left untouched")),
            ),
        },
    }
}

/// Apply every planned step.
pub fn apply(root: &Path, plan: &mut Plan) -> Result<()> {
    for step in &mut plan.steps {
        if step.state != State::Planned {
            continue;
        }
        let path = root.join(&step.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        match step.kind {
            Kind::Instructions => {
                let mut text = std::fs::read_to_string(&path).unwrap_or_default();
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(crate::AGENT_INSTRUCTIONS);
                std::fs::write(&path, text)
                    .with_context(|| format!("writing {}", path.display()))?;
            }
            Kind::RuleFile => {
                std::fs::write(&path, rule_file_body(&step.path))
                    .with_context(|| format!("writing {}", path.display()))?;
            }
            Kind::Mcp => {
                let before = std::fs::read_to_string(&path).unwrap_or_default();
                let after = with_mcp_entry(&before)?;
                std::fs::write(&path, after)
                    .with_context(|| format!("writing {}", path.display()))?;
            }
        }
        step.state = State::AlreadyPresent;
    }
    plan.applied = true;
    Ok(())
}

/// The contents of a dedicated rule file.
///
/// Cursor's `.mdc` rules need frontmatter to apply to every request; without it the
/// file is written and silently never read, which is worse than not writing it.
fn rule_file_body(rel: &str) -> String {
    let block = crate::AGENT_INSTRUCTIONS.trim_start();
    if rel.ends_with(".mdc") {
        format!("---\ndescription: Reify\nalwaysApply: true\n---\n\n{block}")
    } else {
        block.to_string()
    }
}

/// Splice our server entry into an MCP config, preserving everything else byte for byte.
///
/// Textual rather than a `serde_json` round-trip on purpose. Re-serialising sorts the
/// user's keys, collapses their indentation and drops the shape of a file they wrote by
/// hand — this config is theirs, and the only part of it that should change is the part
/// being added. The result is parsed before it is returned, so a splice that would
/// produce broken JSON fails loudly instead of being written.
pub fn with_mcp_entry(text: &str) -> Result<String> {
    if text.trim().is_empty() {
        return Ok(format!(
            "{{\n  \"{MCP_SERVERS}\": {{\n    {MCP_ENTRY}\n  }}\n}}\n"
        ));
    }
    let parsed: serde_json::Value =
        serde_json::from_str(text).context("the existing MCP config is not valid JSON")?;
    if parsed
        .get(MCP_SERVERS)
        .and_then(|s| s.get(MCP_NAME))
        .is_some()
    {
        return Ok(text.to_string());
    }

    let out = match body_start(text, Some(MCP_SERVERS)) {
        // `mcpServers` is there: add one member to it.
        Some(at) => splice(text, at, MCP_ENTRY, 4),
        // It is not: add the whole key to the root object.
        None => {
            let at = body_start(text, None)
                .context("the existing MCP config has no top-level object")?;
            splice(
                text,
                at,
                &format!("\"{MCP_SERVERS}\": {{ {MCP_ENTRY} }}"),
                2,
            )
        }
    };
    serde_json::from_str::<serde_json::Value>(&out)
        .context("adding the server entry would have produced invalid JSON; nothing written")?;
    Ok(out)
}

/// Remove our server entry, leaving everything else byte for byte.
///
/// Returns `None` when there was nothing to remove.
pub fn without_mcp_entry(text: &str) -> Result<Option<String>> {
    let parsed: serde_json::Value =
        serde_json::from_str(text).context("the MCP config is not valid JSON")?;
    if parsed
        .get(MCP_SERVERS)
        .and_then(|s| s.get(MCP_NAME))
        .is_none()
    {
        return Ok(None);
    }
    let Some(span) = member_span(text, MCP_SERVERS, MCP_NAME) else {
        return Ok(None);
    };
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..span.0]);
    out.push_str(&text[span.1..]);
    serde_json::from_str::<serde_json::Value>(&out)
        .context("removing the server entry would have produced invalid JSON; nothing written")?;
    Ok(Some(out))
}

/// Insert `member` just inside an object whose body starts at `at`.
fn splice(text: &str, at: usize, member: &str, indent: usize) -> String {
    let rest = &text[at..];
    let empty = rest.trim_start().starts_with('}');
    let pad = " ".repeat(indent);
    let mut out = String::with_capacity(text.len() + member.len() + 8);
    out.push_str(&text[..at]);
    out.push('\n');
    out.push_str(&pad);
    out.push_str(member);
    if !empty {
        out.push(',');
    }
    // An object that was `{}` gets its closing brace put on its own line; one that
    // already had members keeps whatever the author wrote after the brace.
    if empty {
        out.push('\n');
        out.push_str(&" ".repeat(indent.saturating_sub(2)));
        out.push_str(rest.trim_start());
    } else {
        out.push_str(rest);
    }
    out
}

/// Byte offset just past the `{` opening the root object, or the object at top-level
/// `key`.
///
/// A small scanner rather than a JSON library: the caller has already parsed the text
/// for validity, and what is needed here is a *position in the original bytes*, which no
/// parse tree carries.
fn body_start(text: &str, key: Option<&str>) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut opened_at = 0usize;
    let mut awaiting = false;

    for (i, &c) in bytes.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                in_string = false;
                // A key sits at depth 1 — inside the root object — and is followed by
                // a colon. Anything else and this was a value that happened to match.
                if depth == 1 && key.is_some_and(|k| &text[opened_at + 1..i] == k) {
                    awaiting = true;
                }
            }
            continue;
        }
        if awaiting && !c.is_ascii_whitespace() && c != b':' && c != b'{' {
            awaiting = false;
        }
        match c {
            b'"' => {
                in_string = true;
                opened_at = i;
            }
            b'{' => {
                depth += 1;
                if awaiting {
                    return Some(i + 1);
                }
                if key.is_none() && depth == 1 {
                    return Some(i + 1);
                }
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            b'[' => depth += 1,
            _ => {}
        }
    }
    None
}

/// The byte range covering `parent.member` and the comma that separates it from its
/// neighbours, so cutting it leaves valid JSON.
fn member_span(text: &str, parent: &str, member: &str) -> Option<(usize, usize)> {
    let body = body_start(text, Some(parent))?;
    let bytes = text.as_bytes();
    let mut i = body;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut key_at: Option<(usize, usize)> = None;
    let mut quote_at = 0usize;

    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                in_string = false;
                if depth == 0 && &text[quote_at + 1..i] == member {
                    key_at = Some((quote_at, i));
                }
            }
            i += 1;
            continue;
        }
        match c {
            b'"' => {
                in_string = true;
                quote_at = i;
            }
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                if depth == 0 {
                    return None; // end of the parent object, member not found
                }
                depth -= 1;
                // A value that has just closed at the parent's own level ends the
                // member we were tracking.
                if depth == 0 {
                    if let Some((start, _)) = key_at {
                        return Some(widen(text, start, i + 1));
                    }
                }
            }
            b',' if depth == 0 => {
                if let Some((start, _)) = key_at {
                    return Some(widen(text, start, i + 1));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Widen a member's range to swallow one separating comma and the whitespace around it.
///
/// Without this, removing the last member of an object leaves a trailing comma, which is
/// not valid JSON.
fn widen(text: &str, start: usize, end: usize) -> (usize, usize) {
    let bytes = text.as_bytes();
    let mut end = end;
    // A comma after the member: take it, plus the newline it sat on.
    let mut probe = end;
    while probe < bytes.len() && bytes[probe].is_ascii_whitespace() && bytes[probe] != b'\n' {
        probe += 1;
    }
    if probe < bytes.len() && bytes[probe] == b',' {
        end = probe + 1;
    } else {
        // No comma after, so this was the last member: take the one before it instead.
        let mut back = start;
        while back > 0 && bytes[back - 1].is_ascii_whitespace() {
            back -= 1;
        }
        if back > 0 && bytes[back - 1] == b',' {
            return (back - 1, end);
        }
    }
    // Leading whitespace on the member's own line goes with it.
    let mut begin = start;
    while begin > 0 && (bytes[begin - 1] == b' ' || bytes[begin - 1] == b'\t') {
        begin -= 1;
    }
    if begin > 0 && bytes[begin - 1] == b'\n' {
        begin -= 1;
    }
    (begin, end)
}

/// Every repository path `install` could ever write to, for `uninit` to undo.
///
/// Derived from the same table `plan` walks, so a new agent cannot be added to one
/// without appearing in the other.
pub fn removable_targets() -> Vec<(String, Kind)> {
    let mut out: Vec<(String, Kind)> = Vec::new();
    for agent in KNOWN {
        out.push((agent.instruction_file.to_string(), Kind::Instructions));
        if let Some((dir, file)) = agent.rules_dir {
            out.push((format!("{dir}/{file}"), Kind::RuleFile));
        }
        if let Some(config) = agent.mcp_config {
            out.push((config.to_string(), Kind::Mcp));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out.dedup_by(|a, b| a.0 == b.0);
    out
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("reify-install-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// A repository with a `CLAUDE.md` and nothing else Reify recognises.
    fn claude_repo(name: &str) -> PathBuf {
        let d = tmp(name);
        fs::write(d.join("CLAUDE.md"), "# My project\n\nSome house rules.\n").unwrap();
        d
    }

    #[test]
    fn a_claude_repository_gets_the_shell_integration_not_mcp() {
        // The documented position: level 0 first, because MCP schemas cost tokens on
        // every turn of every session.
        let d = claude_repo("level0");
        let plan = plan_with_home(&d, false, None).unwrap();
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].path, "CLAUDE.md");
        assert_eq!(plan.steps[0].kind, Kind::Instructions);
        assert_eq!(plan.steps[0].agents, vec!["Claude Code"]);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn mcp_is_the_deliberate_opt_in_and_replaces_the_block_rather_than_joining_it() {
        let d = claude_repo("mcpoptin");
        let plan = plan_with_home(&d, true, None).unwrap();
        assert_eq!(plan.steps.len(), 1, "one integration per agent, not two");
        assert_eq!(plan.steps[0].path, ".mcp.json");
        assert_eq!(plan.steps[0].kind, Kind::Mcp);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn planning_writes_nothing_and_applying_is_idempotent() {
        let d = claude_repo("idempotent");
        let before = fs::read_to_string(d.join("CLAUDE.md")).unwrap();

        let mut p = plan_with_home(&d, false, None).unwrap();
        assert!(p.has_work());
        assert_eq!(
            fs::read_to_string(d.join("CLAUDE.md")).unwrap(),
            before,
            "a plan must not write"
        );

        apply(&d, &mut p).unwrap();
        let after = fs::read_to_string(d.join("CLAUDE.md")).unwrap();
        assert!(after.starts_with(&before), "the user's content is kept");
        assert!(after.contains("reify context"));

        let again = plan_with_home(&d, false, None).unwrap();
        assert!(!again.has_work(), "a second run has nothing to do");
        let mut again = again;
        apply(&d, &mut again).unwrap();
        assert_eq!(
            fs::read_to_string(d.join("CLAUDE.md")).unwrap(),
            after,
            "applying a no-op plan changes nothing"
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_rules_directory_gets_its_own_file_rather_than_a_shared_one() {
        let d = tmp("rulesdir");
        fs::create_dir_all(d.join(".cursor/rules")).unwrap();
        let p = plan_with_home(&d, false, None).unwrap();
        let step = p.steps.iter().find(|s| s.kind == Kind::RuleFile).unwrap();
        assert_eq!(step.path, ".cursor/rules/reify.mdc");
        let mut p = p;
        apply(&d, &mut p).unwrap();
        let body = fs::read_to_string(d.join(".cursor/rules/reify.mdc")).unwrap();
        assert!(
            body.starts_with("---\n") && body.contains("alwaysApply: true"),
            "a Cursor rule without frontmatter is written and never read"
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn two_agents_reading_one_file_are_one_write_credited_to_both() {
        let d = tmp("shared");
        fs::write(d.join("AGENTS.md"), "# rules\n").unwrap();
        fs::create_dir_all(d.join(".codex")).unwrap();
        let p = plan_with_home(&d, false, None).unwrap();
        let step = p.steps.iter().find(|s| s.path == "AGENTS.md").unwrap();
        assert!(step.agents.len() >= 2, "{:?}", step.agents);
        assert_eq!(
            p.steps.iter().filter(|s| s.path == "AGENTS.md").count(),
            1,
            "one file, one write"
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn an_agent_installed_on_this_machine_but_not_in_this_repository_is_never_written_for() {
        // `~/.cursor` means the user has Cursor, not that this repository is worked on
        // with it. Creating a `.cursorrules` on that evidence is the guess this command
        // exists to avoid — so it is reported instead, which also explains the silence.
        let d = tmp("homeonly");
        fs::write(d.join("main.rs"), "fn main() {}").unwrap();
        let home = tmp("fakehome");
        fs::create_dir_all(home.join(".cursor")).unwrap();

        let p = plan_with_home(&d, false, Some(&home)).unwrap();
        assert!(p.steps.is_empty(), "{:?}", p.steps);
        assert!(p.detected_elsewhere.iter().any(|a| a.starts_with("Cursor")));
        assert!(p.instruction_block.is_some(), "hand over the block instead");
        assert!(!d.join(".cursorrules").exists());
        let _ = fs::remove_dir_all(&d);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn home_evidence_corroborates_repository_evidence_rather_than_replacing_it() {
        let d = claude_repo("corroborate");
        let home = tmp("fakehome2");
        fs::create_dir_all(home.join(".claude")).unwrap();
        let p = plan_with_home(&d, false, Some(&home)).unwrap();
        assert_eq!(p.steps.len(), 1);
        assert_eq!(
            p.steps[0].evidence,
            vec!["CLAUDE.md is here", "~/.claude exists"],
            "both are stated, so the detection can be checked"
        );
        let _ = fs::remove_dir_all(&d);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn nothing_recognised_offers_the_block_to_paste_rather_than_guessing() {
        let d = tmp("unknown");
        fs::write(d.join("main.rs"), "fn main() {}").unwrap();
        let p = plan_with_home(&d, false, None).unwrap();
        assert!(p.steps.is_empty());
        assert!(p.instruction_block.unwrap().contains("reify context"));
        let _ = fs::remove_dir_all(&d);
    }

    // The requirement most likely to break somebody's setup. Everything the user wrote
    // must survive, byte for byte, apart from the entry being added.
    #[test]
    fn an_existing_mcp_config_survives_byte_identical_apart_from_the_added_entry() {
        let original = r#"{
  "mcpServers": {
    "postgres": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-postgres", "postgres://localhost/db"],
      "env": { "PGPASSWORD": "hunter2" }
    }
  },
  "somethingElse": [1, 2, 3]
}
"#;
        let updated = with_mcp_entry(original).unwrap();

        // The added line, and nothing else.
        let removed: Vec<&str> = original
            .lines()
            .filter(|l| !updated.lines().any(|u| u == *l))
            .collect();
        assert!(removed.is_empty(), "lines disappeared: {removed:?}");
        let added: Vec<&str> = updated
            .lines()
            .filter(|l| !original.lines().any(|o| o == *l))
            .collect();
        assert_eq!(added.len(), 1, "expected exactly one added line: {added:?}");
        assert!(added[0].contains("\"reify\""));

        // And the user's own content is intact when read back.
        let after: serde_json::Value = serde_json::from_str(&updated).unwrap();
        let before: serde_json::Value = serde_json::from_str(original).unwrap();
        assert_eq!(after["somethingElse"], before["somethingElse"]);
        assert_eq!(
            after[MCP_SERVERS]["postgres"], before[MCP_SERVERS]["postgres"],
            "an unrelated server must survive exactly"
        );
        assert!(after[MCP_SERVERS][MCP_NAME]["command"] == "reify");

        // Removing it puts the file back the way it was.
        let restored = without_mcp_entry(&updated).unwrap().unwrap();
        assert_eq!(restored, original, "uninstalling must be a clean reversal");
    }

    #[test]
    fn an_empty_or_absent_config_is_created_whole() {
        let fresh = with_mcp_entry("").unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&fresh).unwrap();
        assert_eq!(parsed[MCP_SERVERS][MCP_NAME]["command"], "reify");

        let empty_object = with_mcp_entry("{}\n").unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&empty_object).unwrap();
        assert_eq!(parsed[MCP_SERVERS][MCP_NAME]["args"][0], "serve");

        let no_servers_key = with_mcp_entry("{\n  \"other\": true\n}\n").unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&no_servers_key).unwrap();
        assert_eq!(parsed["other"], true);
        assert_eq!(parsed[MCP_SERVERS][MCP_NAME]["command"], "reify");

        let empty_servers = with_mcp_entry("{\n  \"mcpServers\": {}\n}\n").unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&empty_servers).unwrap();
        assert_eq!(parsed[MCP_SERVERS][MCP_NAME]["command"], "reify");
    }

    #[test]
    fn adding_an_entry_twice_changes_nothing() {
        let once = with_mcp_entry("{\n  \"mcpServers\": {}\n}\n").unwrap();
        assert_eq!(with_mcp_entry(&once).unwrap(), once);
    }

    #[test]
    fn a_config_that_does_not_parse_is_reported_and_left_alone() {
        let d = tmp("broken");
        fs::write(d.join("CLAUDE.md"), "# x\n").unwrap();
        let broken = "{ \"mcpServers\": { oops }";
        fs::write(d.join(".mcp.json"), broken).unwrap();

        let mut p = plan_with_home(&d, true, None).unwrap();
        let step = &p.steps[0];
        assert_eq!(step.state, State::Skipped);
        assert!(step.problem.as_ref().unwrap().contains("not valid JSON"));

        apply(&d, &mut p).unwrap();
        assert_eq!(
            fs::read_to_string(d.join(".mcp.json")).unwrap(),
            broken,
            "an unparsable config is never overwritten"
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_string_that_merely_looks_like_the_servers_key_is_not_mistaken_for_it() {
        // `body_start` scans bytes, so it has to tell a key from a value that happens
        // to spell the same thing.
        let text = "{\n  \"note\": \"mcpServers\",\n  \"mcpServers\": {\n    \"a\": {}\n  }\n}\n";
        let updated = with_mcp_entry(text).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&updated).unwrap();
        assert_eq!(parsed["note"], "mcpServers");
        assert!(parsed[MCP_SERVERS]["a"].is_object());
        assert_eq!(parsed[MCP_SERVERS][MCP_NAME]["command"], "reify");
    }

    #[test]
    fn removing_the_only_entry_leaves_valid_json() {
        let text = with_mcp_entry("{}").unwrap();
        let stripped = without_mcp_entry(&text).unwrap().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&stripped).unwrap();
        assert!(parsed[MCP_SERVERS].get(MCP_NAME).is_none());
        assert!(without_mcp_entry(&stripped).unwrap().is_none());
    }

    #[test]
    fn every_agent_in_the_table_is_reachable_by_uninit() {
        // A new agent added to KNOWN without a removal path would leave orphans.
        let targets = removable_targets();
        for agent in KNOWN {
            assert!(
                targets.iter().any(|(p, _)| p == agent.instruction_file
                    || agent
                        .rules_dir
                        .is_some_and(|(d, f)| *p == format!("{d}/{f}"))),
                "{} has no removal path",
                agent.name
            );
        }
    }
}

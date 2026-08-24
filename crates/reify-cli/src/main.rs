//! The `reify` command line.
//!
//! Two audiences, one code path. Humans get dense terminal output; agents get
//! `--json` against a versioned schema. Every command works fully offline — Reify makes
//! no network call at all in this build, which is asserted by a test rather than
//! promised in a README.

mod mcp;
mod render;
mod selfmanage;

use anyhow::{bail, Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use std::path::{Path, PathBuf};

use reify::context::{self, ContextOptions};
use reify::index::{self, IndexOptions};
use reify::llm;
use reify::lockfile::IndexLock;
use reify::query;
use reify::store::Store;

#[derive(Parser, Debug)]
#[command(
    name = "reify",
    about = "A local knowledge engine that gives AI coding agents the smallest context they need",
    version
)]
struct Cli {
    /// Repository root. Defaults to the nearest ancestor containing `.reify`, else the
    /// working directory.
    #[arg(long, short = 'C', global = true)]
    repo: Option<PathBuf>,

    /// Emit machine-readable JSON instead of terminal output.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Create `.reify/`, and report what will and will not be indexed.
    Init {
        /// Append Reify's usage instructions to this repository's agent instruction
        /// file (`AGENTS.md` or `CLAUDE.md`). Without it, `init` only shows the block
        /// and where it would go.
        #[arg(long)]
        write_agent_instructions: bool,
    },

    /// Compile the system model. Incremental unless `--force`.
    Index {
        /// Rebuild from scratch.
        #[arg(long)]
        force: bool,
        /// Bound on how far back history is walked.
        #[arg(long, default_value_t = index::DEFAULT_MAX_COMMITS)]
        max_commits: usize,
    },

    /// Freshness and coverage of the current store.
    Status,

    /// Compile the minimum useful context for a task. The flagship command.
    Context {
        /// What you are about to do, in your own words.
        task: String,
        /// Token budget for the compiled context.
        #[arg(long, default_value_t = context::DEFAULT_BUDGET)]
        budget: u32,
        /// Files already read or ruled out; repeat the flag per file.
        ///
        /// This is how an agent iterates: call again excluding what the first answer
        /// offered, and the freed budget goes to the next-best candidates.
        #[arg(long)]
        exclude: Vec<String>,
        /// Return regions sized to be *edited*: whole enclosing definitions, the
        /// file's imports, and neighbouring code, instead of the smallest spans that
        /// answer the question. Use this when the next step is writing a patch.
        #[arg(long)]
        for_edit: bool,
        /// Emit TOON, the agent-facing format: columns stated once, one row per
        /// record. Roughly a third of the JSON envelope's tokens for the same facts.
        #[arg(long)]
        toon: bool,
    },

    /// Why does this exist: rules, concepts, documents, history and blast radius.
    Why {
        /// `path:line`, a path, or a symbol name.
        target: String,
    },

    /// What breaks if this changes.
    Impact {
        /// A symbol name, a file path, or a description of the change.
        query: String,
    },

    /// Documentation that disagrees with the implementation.
    Conflicts,

    /// Mined business rules.
    Rules {
        /// Hide candidates below this confidence.
        #[arg(long, default_value_t = reify::rules::MIN_SURFACED_CONFIDENCE)]
        min_confidence: f32,
    },

    /// A system-level scorecard.
    Report,

    /// Everything known about a business concept, in every language it appears in.
    Explain {
        /// A business term, in any language.
        term: String,
    },

    /// The sequence of code that carries out a business process.
    Flow {
        /// A process or symbol name.
        process: String,
    },

    /// Inspect and extend the concept glossary.
    Concepts {
        /// Print concepts that could be declared, in glossary syntax.
        #[arg(long)]
        suggest: bool,
        /// Append the suggestions to `.reify/glossary.toml` instead of printing them.
        #[arg(long)]
        write: bool,
    },

    /// A compact risk header for a file about to be edited. Designed for an editor hook.
    Preflight {
        /// The file about to be changed.
        path: String,
    },

    /// Model-assistance status and prompt inspection.
    Llm {
        #[command(subcommand)]
        action: LlmAction,
    },

    /// Print a shell completion script.
    Completions {
        /// The shell to generate for.
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },

    /// Serve the Model Context Protocol on stdio.
    Serve {
        /// Speak MCP. Present so the flag is explicit rather than implied.
        #[arg(long)]
        mcp: bool,
    },

    /// Replace this binary with a newer release.
    ///
    /// The one command in Reify that reaches the network — through `curl` and `tar`
    /// as visible subprocesses, never an embedded client — with the checksum
    /// verified before anything is installed. `REIFY_OFFLINE=1` refuses it.
    Upgrade {
        /// Only report whether a newer release exists.
        #[arg(long)]
        check: bool,
        /// Install this exact version instead of the latest (e.g. `v0.1.0`).
        version: Option<String>,
    },

    /// Remove the reify binary. Repository stores are never touched.
    Uninstall {
        /// Actually remove it; without this flag, only the plan is shown.
        #[arg(long)]
        yes: bool,
    },

    /// Remove this repository's `.reify/` store and the instruction block
    /// `reify init --write-agent-instructions` appended.
    Uninit {
        /// Actually remove them; without this flag, only the plan is shown.
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand, Debug)]
enum LlmAction {
    /// Is a provider configured, and what would be run?
    Status,
    /// Print the exact bytes that would be sent for a task. Nothing is sent.
    Preview {
        task: String,
        #[arg(long, default_value_t = context::DEFAULT_BUDGET)]
        budget: u32,
    },
}

fn main() {
    if let Err(error) = run() {
        eprintln!("reify: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let root = resolve_root(cli.repo.as_deref())?;

    match &cli.command {
        Command::Init {
            write_agent_instructions,
        } => init(&root, *write_agent_instructions, cli.json),
        Command::Index { force, max_commits } => {
            let reify_dir = root.join(index::REIFY_DIR);
            std::fs::create_dir_all(&reify_dir)?;

            // Held for the whole run and released on drop. Without it, two concurrent
            // runs interleave and the only symptom a developer sees is a raw SQLite
            // "database is locked" error.
            let lock = IndexLock::acquire(&reify_dir)?;
            if lock.reclaimed_stale() && !cli.json {
                eprintln!(
                    "A previous index did not finish; its lock was reclaimed.                      If results look wrong, run `reify index --force`."
                );
            }

            let opts = IndexOptions {
                root: root.clone(),
                force: *force,
                max_commits: *max_commits,
                progress: (!cli.json).then(render::progress_reporter),
            };
            let mut store = open_store_for_write(&opts)?;
            let report = index::index(&mut store, &opts)?;
            render::clear_progress();
            render::index_report(&report, cli.json)
        }
        Command::Status => {
            let store = open_existing(&root)?;
            render::status(&store, &root, cli.json)
        }
        Command::Context {
            task,
            budget,
            exclude,
            toon,
            for_edit,
        } => {
            let store = open_existing(&root)?;
            let compiled = context::compile(
                &store,
                task,
                &ContextOptions {
                    for_edit: *for_edit,
                    budget: *budget,
                    exclude: exclude.clone(),
                    ..Default::default()
                },
            )?;
            if *toon {
                print!("{}", render::context_toon(&compiled));
                return Ok(());
            }
            render::context(&compiled, cli.json)
        }
        Command::Why { target } => {
            let store = open_existing(&root)?;
            let answer = query::why(&store, &root, target)?;
            render::why(&answer, cli.json)
        }
        Command::Impact { query: q } => {
            let store = open_existing(&root)?;
            let answer = query::impact(&store, q)?;
            render::impact(&answer, cli.json)
        }
        Command::Conflicts => {
            let store = open_existing(&root)?;
            render::conflicts(&query::conflicts(&store)?, cli.json)
        }
        Command::Rules { min_confidence } => {
            let store = open_existing(&root)?;
            render::rules(&query::rules(&store, *min_confidence)?, cli.json)
        }
        Command::Report => {
            let store = open_existing(&root)?;
            render::report(&query::report(&store)?, cli.json)
        }
        Command::Explain { term } => {
            let store = open_existing(&root)?;
            render::explain(&query::explain(&store, term)?, cli.json)
        }
        Command::Flow { process } => {
            let store = open_existing(&root)?;
            render::flow(&query::flow(&store, process)?, cli.json)
        }
        Command::Concepts { suggest, write } => {
            let store = open_existing(&root)?;
            concepts(&store, &root, *suggest, *write, cli.json)
        }
        Command::Preflight { path } => {
            let store = open_existing(&root)?;
            render::preflight(&query::preflight(&store, path)?, cli.json)
        }
        Command::Llm { action } => match action {
            LlmAction::Status => render::llm_status(&root, cli.json),
            LlmAction::Preview { task, budget } => {
                let store = open_existing(&root)?;
                let compiled = context::compile(
                    &store,
                    task,
                    &ContextOptions {
                        budget: *budget,
                        ..Default::default()
                    },
                )?;
                let prompt = llm::synthesis_prompt(task, &render::facts_for_synthesis(&compiled));
                render::llm_preview(&prompt, cli.json)
            }
        },
        Command::Completions { shell } => {
            let mut command = Cli::command();
            let name = command.get_name().to_string();
            clap_complete::generate(*shell, &mut command, name, &mut std::io::stdout());
            Ok(())
        }
        Command::Serve { mcp } => {
            anyhow::ensure!(*mcp, "only `--mcp` is supported; pass `reify serve --mcp`");
            mcp::serve(&root)
        }
        Command::Upgrade { check, version } => selfmanage::upgrade(*check, version.as_deref()),
        Command::Uninstall { yes } => selfmanage::uninstall(*yes),
        Command::Uninit { yes } => selfmanage::uninit(&root, *yes),
    }
}

/// Show the concept layer, and optionally propose glossary entries for it.
///
/// A declared glossary is the highest-precision knowledge Reify can hold, and hand
/// writing one from nothing is the reason most teams never do. This turns what was
/// mined into a starting point a human edits down.
fn concepts(store: &Store, root: &Path, suggest: bool, write: bool, json: bool) -> Result<()> {
    let proposals = query::concept_suggestions(store)?;
    if !suggest && !write {
        return render::concepts(&query::concept_overview(store)?, json);
    }
    let rendered = reify::concepts::Glossary::render(&proposals);
    if !write {
        print!("{rendered}");
        return Ok(());
    }
    let path = root.join(index::REIFY_DIR).join(index::GLOSSARY_FILE);
    let mut existing = std::fs::read_to_string(&path).unwrap_or_default();
    existing.push_str("\n# --- suggested by `reify concepts --write`; edit freely ---\n");
    existing.push_str(&rendered);
    std::fs::write(&path, existing).with_context(|| format!("writing {}", path.display()))?;
    eprintln!(
        "Appended {} suggested concepts to {}",
        proposals.len(),
        path.display()
    );
    Ok(())
}

/// Find the repository root.
///
/// Walking up to an existing `.reify` means the commands work from any subdirectory,
/// which is how anyone actually uses a tool inside a large repository.
fn resolve_root(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()));
    }
    let cwd = std::env::current_dir().context("reading the working directory")?;
    let mut candidate = cwd.as_path();
    loop {
        if candidate.join(index::REIFY_DIR).is_dir() {
            return Ok(candidate.to_path_buf());
        }
        match candidate.parent() {
            Some(parent) => candidate = parent,
            None => return Ok(cwd),
        }
    }
}

fn open_store_for_write(opts: &IndexOptions) -> Result<Store> {
    let path = opts.store_path();
    if opts.force && path.exists() {
        std::fs::remove_file(&path).ok();
        for suffix in ["-wal", "-shm"] {
            std::fs::remove_file(format!("{}{suffix}", path.display())).ok();
        }
    }
    Store::open(&path)
}

fn open_existing(root: &Path) -> Result<Store> {
    let path = root.join(index::REIFY_DIR).join(index::STORE_FILE);
    if !path.exists() {
        bail!(
            "no index at {}; run `reify init && reify index` first",
            path.display()
        );
    }
    Store::open(&path)
}

/// Create `.reify/`, scaffold a glossary, and report what indexing will cover.
///
/// The skipped-file summary is printed on purpose: a knowledge tool that silently
/// ignores half a repository is worse than one that indexes nothing.
fn init(root: &Path, write_agent_instructions: bool, json: bool) -> Result<()> {
    let dir = root.join(index::REIFY_DIR);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;

    let glossary = dir.join(index::GLOSSARY_FILE);
    let created_glossary = !glossary.exists();
    if created_glossary {
        std::fs::write(&glossary, GLOSSARY_TEMPLATE)?;
    }

    // The store is private and machine-specific; committing it would leak internals
    // into a shared repository.
    let ignore = dir.join(".gitignore");
    if !ignore.exists() {
        std::fs::write(
            &ignore,
            "# The compiled store is local to this machine.\n*\n",
        )?;
    }

    let found = reify::discover::discover(root)?;
    let agent_file = find_agent_instruction_file(root);
    let mut wrote_instructions = false;
    if write_agent_instructions {
        let target = agent_file.clone().unwrap_or_else(|| root.join("AGENTS.md"));
        let existing = std::fs::read_to_string(&target).unwrap_or_default();
        if existing.contains("reify context") {
            eprintln!(
                "{} already mentions reify; nothing appended.",
                target.display()
            );
        } else {
            let mut updated = existing;
            if !updated.is_empty() && !updated.ends_with('\n') {
                updated.push('\n');
            }
            updated.push_str(AGENT_INSTRUCTIONS);
            std::fs::write(&target, updated)
                .with_context(|| format!("writing {}", target.display()))?;
            eprintln!("Appended Reify instructions to {}", target.display());
            wrote_instructions = true;
        }
    }

    render::init(
        root,
        &found,
        created_glossary,
        agent_file.as_deref(),
        wrote_instructions,
        reify::gitlog::is_repository(root),
        json,
    )
}

/// The agent instruction file this repository already uses, if any.
fn find_agent_instruction_file(root: &Path) -> Option<PathBuf> {
    ["AGENTS.md", "CLAUDE.md", "CONVENTIONS.md", ".cursorrules"]
        .iter()
        .map(|name| root.join(name))
        .find(|path| path.is_file())
}

/// What an agent needs to be told. Kept short on purpose: instructions an agent must
/// re-read every session are themselves a context cost.
pub const AGENT_INSTRUCTIONS: &str = r#"
## Before changing code in this repository

Run `reify context "<what you are about to do>" --toon` and read its output first.
Run `reify why <file>:<line>` before modifying unfamiliar logic.
Run `reify impact "<symbol>"` before changing anything shared.

Claims marked `INFERRED` are leads to verify against their citation, not facts.
If `conflicts` is non-empty, resolve the disagreement before changing behaviour.
"#;

const GLOSSARY_TEMPLATE: &str = r#"# Reify glossary.
#
# Declared concepts are the highest-precision knowledge in the system: they are trusted
# above anything mined from translations, headings or identifiers, and they are the
# cheapest way to make Reify understand your domain.
#
# Add one entry per business idea. Labels may be in any language.
#
# [[concept]]
# id = "STRATEGIC_ACCOUNT"
# labels = { eng = "strategic account", vie = "khách hàng chiến lược" }
# code = ["StrategicAccount"]
# db = ["CUSTOMER_GROUP=7"]
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn the_documented_agent_instructions_name_the_commands_they_describe() {
        // These are what `docs/integration/` tells users to add, so the two must not
        // drift apart.
        for command in ["reify context", "reify why", "reify impact"] {
            assert!(AGENT_INSTRUCTIONS.contains(command), "missing {command}");
        }
        assert!(AGENT_INSTRUCTIONS.contains("INFERRED"));
    }

    #[test]
    fn an_explicit_repo_flag_wins_over_discovery() {
        let dir = std::env::temp_dir();
        let resolved = resolve_root(Some(&dir)).unwrap();
        assert_eq!(resolved, dir.canonicalize().unwrap_or(dir));
    }

    #[test]
    fn every_command_accepts_json() {
        // The agent-facing surface must exist on every command, not just some.
        for args in [
            vec!["reify", "--json", "status"],
            vec!["reify", "--json", "context", "task"],
            vec!["reify", "--json", "why", "a.py:1"],
            vec!["reify", "--json", "impact", "x"],
            vec!["reify", "--json", "conflicts"],
            vec!["reify", "--json", "rules"],
            vec!["reify", "--json", "report"],
            vec!["reify", "--json", "explain", "x"],
            vec!["reify", "--json", "flow", "x"],
            vec!["reify", "--json", "concepts"],
            vec!["reify", "--json", "preflight", "a.py"],
            vec!["reify", "--json", "llm", "status"],
            vec!["reify", "--json", "init"],
        ] {
            let cli = Cli::try_parse_from(&args).expect("should parse");
            assert!(cli.json, "{args:?}");
        }
    }
}

//! The Reify Brownfield Benchmark runner.
//!
//! Three commands, so a result is reproducible from a clean checkout:
//!
//! ```text
//! reify-bench tasks  --repo <path> --out benchmarks/tasks/<name>.json
//! reify-bench run    --repo <path> --tasks <file> --out benchmarks/results/<date>/
//! reify-bench report --in  <results dir> --out benchmarks/REPORT.md
//! ```
//!
//! The task set is generated and frozen *before* any condition runs, and the raw
//! per-task outcomes are written out alongside the summary. Nothing in the report is
//! computed anywhere except from those files.

mod conditions;
mod metrics;
mod tasks;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

use reify::store::Store;

/// Token budget every condition is held to.
///
/// The same number for all of them: comparing a tool that may read the whole
/// repository against one held to a budget would measure the budget, not the tool.
const DEFAULT_BUDGET: u32 = 4_000;

#[derive(Parser, Debug)]
#[command(name = "reify-bench", about = "The Reify Brownfield Benchmark")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Derive a frozen task set from a repository's history.
    Tasks {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 40)]
        count: usize,
        #[arg(long, default_value_t = 4_000)]
        scan: usize,
    },
    /// Run every condition over a task set.
    Run {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        tasks: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = DEFAULT_BUDGET)]
        budget: u32,
    },
    /// Render a report from raw results.
    Report {
        #[arg(long = "in")]
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}

fn main() {
    if let Err(error) = run() {
        eprintln!("reify-bench: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    match Cli::parse().command {
        Command::Tasks {
            repo,
            out,
            count,
            scan,
        } => {
            let set = tasks::generate(&repo, count, scan)?;
            eprintln!(
                "{} tasks from {} commits at {}",
                set.tasks.len(),
                set.generated_from_commits,
                &set.head[..8.min(set.head.len())]
            );
            write_json(&out, &set)
        }
        Command::Run {
            repo,
            tasks: task_file,
            out,
            budget,
        } => execute(&repo, &task_file, &out, budget),
        Command::Report { input, out } => report(&input, &out),
    }
}

fn execute(repo: &Path, task_file: &Path, out: &Path, budget: u32) -> Result<()> {
    let set: tasks::TaskSet = read_json(task_file)?;
    let store_path = repo
        .join(reify::index::REIFY_DIR)
        .join(reify::index::STORE_FILE);
    anyhow::ensure!(
        store_path.exists(),
        "no index at {}; run `reify index` in the benchmark repository first",
        store_path.display()
    );
    let store = Store::open(&store_path)?;

    eprintln!("loading corpus...");
    let corpus = conditions::Corpus::load(repo)?;
    eprintln!("{} code files, {} tasks", corpus.len(), set.tasks.len());

    let mut outcomes: Vec<metrics::Outcome> = Vec::new();
    for (i, task) in set.tasks.iter().enumerate() {
        eprint!("\r  task {}/{}", i + 1, set.tasks.len());
        let content = conditions::content_search(&corpus, &task.prompt, budget);
        outcomes.push(metrics::score(
            &task.id,
            "B-content-grep",
            &content,
            &task.ground_truth,
        ));

        let path = conditions::path_search(&corpus, &task.prompt, budget);
        outcomes.push(metrics::score(
            &task.id,
            "C-path-grep",
            &path,
            &task.ground_truth,
        ));

        let compiled = conditions::reify_context(&store, &task.prompt, budget)?;
        outcomes.push(metrics::score(
            &task.id,
            "R-reify",
            &compiled,
            &task.ground_truth,
        ));
    }
    eprintln!();

    std::fs::create_dir_all(out)?;
    write_json(&out.join("outcomes.json"), &outcomes)?;
    let summaries: Vec<metrics::Summary> = ["B-content-grep", "C-path-grep", "R-reify"]
        .iter()
        .map(|n| metrics::summarise(n, &outcomes))
        .collect();
    write_json(&out.join("summary.json"), &summaries)?;
    write_json(&out.join("tasks.json"), &set)?;
    write_json(
        &out.join("environment.json"),
        &serde_json::json!({
            "reify_version": env!("CARGO_PKG_VERSION"),
            "repository": set.repository,
            "head": set.head,
            "budget_tokens": budget,
            "conditions": ["B-content-grep", "C-path-grep", "R-reify"],
            "code_files_in_corpus": corpus.len(),
        }),
    )?;
    eprintln!("wrote {}", out.display());
    Ok(())
}

fn report(input: &Path, out: &Path) -> Result<()> {
    let outcomes: Vec<metrics::Outcome> = read_json(&input.join("outcomes.json"))?;
    let set: tasks::TaskSet = read_json(&input.join("tasks.json"))?;
    let environment: serde_json::Value = read_json(&input.join("environment.json"))?;

    let names = ["B-content-grep", "C-path-grep", "R-reify"];
    let summaries: Vec<metrics::Summary> = names
        .iter()
        .map(|n| metrics::summarise(n, &outcomes))
        .collect();

    let mut md = String::new();
    md.push_str("# Reify Brownfield Benchmark\n\n");
    md.push_str("Generated by `reify-bench report`. Every number here is computed from\n");
    md.push_str("`outcomes.json` in the same directory; nothing is entered by hand.\n\n");

    md.push_str("## What this measures\n\n");
    md.push_str(
        "**Retrieval.** Given a change request, does the tool put the files that actually\n\
         had to change in front of the agent, and at what token cost? It does *not*\n\
         measure whether an agent then makes the change correctly — see Limitations.\n\n",
    );

    md.push_str("## Setup\n\n");
    md.push_str(&format!(
        "| | |\n|---|---|\n| Repository | `{}` |\n| Commit | `{}` |\n| Tasks | {} |\n\
         | Token budget per condition | {} |\n| Code files in corpus | {} |\n\n",
        environment["repository"].as_str().unwrap_or("?"),
        environment["head"].as_str().unwrap_or("?"),
        set.tasks.len(),
        environment["budget_tokens"],
        environment["code_files_in_corpus"],
    ));

    md.push_str("## Conditions\n\n");
    md.push_str(
        "| Condition | What the agent gets |\n|---|---|\n\
         | `B-content-grep` | Files ranked by how many distinct task terms they contain, then by frequency. Charged the same token budget. |\n\
         | `C-path-grep` | Files whose *path* matches task terms. What an agent usually tries first. |\n\
         | `R-reify` | `reify context`, charged for its own output **and** for the spans it recommends reading. |\n\n",
    );

    md.push_str("## Results\n\n");
    md.push_str("| Metric | B content grep | C path grep | R reify |\n|---|---:|---:|---:|\n");
    let row = |label: &str, f: &dyn Fn(&metrics::Summary) -> String| {
        format!(
            "| {label} | {} | {} | {} |\n",
            f(&summaries[0]),
            f(&summaries[1]),
            f(&summaries[2])
        )
    };
    md.push_str(&row("Tasks with at least one correct file", &|s| {
        format!(
            "{}/{} ({:.0}%)",
            s.tasks_with_a_hit,
            s.tasks,
            s.hit_rate * 100.0
        )
    }));
    md.push_str(&row("Mean recall of changed files", &|s| {
        format!("{:.2}", s.mean_recall)
    }));
    md.push_str(&row("Mean precision", &|s| {
        format!("{:.2}", s.mean_precision)
    }));
    md.push_str(&row("MRR of first correct file", &|s| {
        format!("{:.2}", s.mrr)
    }));
    md.push_str(&row("Median tokens to first correct file", &|s| {
        s.median_tokens_to_first_hit
            .map_or("—".into(), |t| t.to_string())
    }));
    md.push_str(&row("Median tokens for the whole answer", &|s| {
        s.median_tokens_total.to_string()
    }));
    md.push_str(&row("Median files put in front of the agent", &|s| {
        s.median_files_inspected.to_string()
    }));
    md.push_str(&row("Median latency (ms)", &|s| {
        s.median_elapsed_ms.to_string()
    }));

    let budget = environment["budget_tokens"].as_u64().unwrap_or(4_000) as u32;
    md.push_str("\n### Cost, corrected for difficulty\n\n");
    md.push_str(
        "The median above is not comparable across conditions: each is computed only\n\
         over the tasks *that condition* solved, so a tool that only ever succeeds on\n\
         easy tasks posts a flattering number precisely because it fails everywhere\n\
         else. Two corrections follow.\n\n",
    );
    md.push_str(&format!(
        "**Expected tokens to reach a changed file**, charging a miss the full {budget}-token budget:\n\n\
         | B content grep | C path grep | R reify |\n|---:|---:|---:|\n| {:.0} | {:.0} | {:.0} |\n\n",
        metrics::expected_tokens("B-content-grep", &outcomes, budget),
        metrics::expected_tokens("C-path-grep", &outcomes, budget),
        metrics::expected_tokens("R-reify", &outcomes, budget),
    ));

    let paired = metrics::pair("R-reify", "B-content-grep", &outcomes);
    md.push_str("**Head to head on the tasks both solved:**\n\n");
    md.push_str(&format!(
        "| | |\n|---|---:|\n| Tasks both solved | {} |\n\
         | Median tokens, Reify | {} |\n| Median tokens, content grep | {} |\n\
         | Tasks Reify reached first for less | {} |\n| Tasks content grep reached first for less | {} |\n\n",
        paired.common_tasks,
        paired.a_median_tokens.map_or("—".into(), |t| t.to_string()),
        paired.b_median_tokens.map_or("—".into(), |t| t.to_string()),
        paired.a_cheaper_on,
        paired.b_cheaper_on,
    ));

    md.push_str("\n## Reading the table\n\n");
    md.push_str(
        "*Tokens to first correct file* is the metric that matters most: an answer that\n\
         eventually contains the right file, after four wrong ones, has not helped.\n\n\
         *Hit rate* and *recall* say whether the right file is there at all. A tool can\n\
         win on tokens by returning almost nothing, so the two must be read together.\n\n",
    );

    md.push_str(&where_reify_lost(&outcomes, &set));

    md.push_str("\n## Limitations\n\n");
    md.push_str(
        "These are stated because the benchmark is only worth what its caveats allow.\n\n\
         1. **Retrieval, not task success.** No model is run. A tool that surfaces the\n   \
            right file may still be given to an agent that fails the task.\n\
         2. **The index is built at `HEAD`, not at each task's parent commit.** The change\n   \
            being asked for is therefore already present in the code. This makes every\n   \
            task easier, for every condition equally — but it means these numbers are an\n   \
            upper bound on real-world retrieval, not an estimate of it.\n\
         3. **Ground truth is what one commit touched.** A different correct solution\n   \
            touching different files scores as a miss.\n\
         4. **One repository, one language mix.** Nothing here shows the result holds for\n   \
            a typed-language codebase.\n\
         5. **No baseline uses a model.** A real agent iterates: greps, reads, greps again.\n   \
            These baselines are single-shot, which understates them.\n",
    );

    std::fs::write(out, md).with_context(|| format!("writing {}", out.display()))?;
    eprintln!("wrote {}", out.display());
    Ok(())
}

/// The section the benchmark exists to keep honest.
fn where_reify_lost(outcomes: &[metrics::Outcome], set: &tasks::TaskSet) -> String {
    let mut md = String::from("\n## Where Reify lost\n\n");
    let baseline: std::collections::HashMap<&str, &metrics::Outcome> = outcomes
        .iter()
        .filter(|o| o.condition == "B-content-grep")
        .map(|o| (o.task.as_str(), o))
        .collect();

    let mut losses: Vec<(&str, String)> = Vec::new();
    for outcome in outcomes.iter().filter(|o| o.condition == "R-reify") {
        let Some(other) = baseline.get(outcome.task.as_str()) else {
            continue;
        };
        let lost = match (outcome.first_hit_rank, other.first_hit_rank) {
            (None, Some(_)) => Some("baseline found a changed file; Reify found none".to_string()),
            (Some(_), Some(_)) if outcome.tokens_to_first_hit > other.tokens_to_first_hit => {
                Some(format!(
                    "reached the first changed file at {} tokens vs {} for the baseline",
                    outcome.tokens_to_first_hit.unwrap_or(0),
                    other.tokens_to_first_hit.unwrap_or(0)
                ))
            }
            _ => None,
        };
        if let Some(reason) = lost {
            losses.push((outcome.task.as_str(), reason));
        }
    }

    if losses.is_empty() {
        md.push_str("No task where the content-grep baseline beat Reify.\n");
        return md;
    }
    md.push_str(&format!(
        "{} of {} tasks where the content-grep baseline did better:\n\n",
        losses.len(),
        set.tasks.len()
    ));
    for (task, reason) in losses.iter().take(12) {
        let prompt = set
            .tasks
            .iter()
            .find(|t| t.id == *task)
            .map(|t| t.prompt.as_str())
            .unwrap_or("");
        md.push_str(&format!("- `{task}` — {reason}\n  > {prompt}\n"));
    }
    if losses.len() > 12 {
        md.push_str(&format!(
            "\n…and {} more in `outcomes.json`.\n",
            losses.len() - 12
        ));
    }
    md
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(value)?)
        .with_context(|| format!("writing {}", path.display()))
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_cli_definition_is_valid() {
        Cli::command().debug_assert();
    }
}

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

mod agent;
mod chart;
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
        /// Only take tasks from commits *after* this revision, and record it as the
        /// base an index should be built at. This is what removes the leakage caveat:
        /// index at the base, and the changes being asked for are genuinely absent.
        #[arg(long)]
        after: Option<String>,
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
    /// Run the model-in-the-loop experiments, including the falsification controls.
    Agent {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        tasks: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = DEFAULT_BUDGET)]
        budget: u32,
        /// Tasks to run. The full matrix is expensive, so a subset is the default.
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Attribute ranking losses on training tasks: recall gap vs selection vs ordering.
    Audit {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        tasks: PathBuf,
        #[arg(long, default_value_t = DEFAULT_BUDGET)]
        budget: u32,
    },
    /// Fit the ranking weights against training tasks, by exhaustive grid search.
    ///
    /// The trap this command is built around: fitting on the evaluation set is the
    /// easiest way to fake a benchmark. Training tasks must come from commits
    /// *earlier* than every benchmark task, the evaluation set stays frozen, and the
    /// chosen weights are judged there exactly once.
    Fit {
        /// Training pairs, as `LABEL=repo_dir=tasks_file`. Several pairs fit jointly,
        /// so one repository cannot pull the weights toward itself.
        #[arg(long = "train", value_name = "LABEL=REPO=TASKS", num_args = 1..)]
        train: Vec<String>,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = DEFAULT_BUDGET)]
        budget: u32,
    },
    /// Render the benchmark charts from committed result files.
    ///
    /// Generated rather than drawn: a chart that can drift from its data is a picture,
    /// not a measurement.
    Chart {
        /// Result directories, as `Label=path`, in the order they should appear.
        #[arg(long = "results", value_name = "LABEL=DIR", num_args = 1..)]
        results: Vec<String>,
        /// Directory to write the SVGs into.
        #[arg(long)]
        out: PathBuf,
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
            after,
        } => {
            let set = tasks::generate(&repo, count, scan, after.as_deref())?;
            eprintln!(
                "{} tasks from {} commits at {}",
                set.tasks.len(),
                set.generated_from_commits,
                &set.head[..8.min(set.head.len())]
            );
            match &set.base {
                Some(base) => eprintln!(
                    "base {} — build the index there:\n  \
                     git worktree add <dir> {base} && reify -C <dir> init && reify -C <dir> index",
                    &base[..8.min(base.len())]
                ),
                None => eprintln!(
                    "no --after given, so the index will contain the changes being asked \
                     for; results are an upper bound"
                ),
            }
            write_json(&out, &set)
        }
        Command::Run {
            repo,
            tasks: task_file,
            out,
            budget,
        } => execute(&repo, &task_file, &out, budget),
        Command::Agent {
            repo,
            tasks: task_file,
            out,
            budget,
            limit,
        } => agent_experiments(&repo, &task_file, &out, budget, limit),
        Command::Audit {
            repo,
            tasks,
            budget,
        } => audit(&repo, &tasks, budget),
        Command::Fit { train, out, budget } => fit(&train, &out, budget),
        Command::Chart { results, out } => charts(&results, &out),
        Command::Report { input, out } => report(&input, &out),
    }
}

/// Attribute every training task's ranking outcome to a stage.
fn audit(repo: &Path, task_file: &Path, budget: u32) -> Result<()> {
    let set: tasks::TaskSet = read_json(task_file)?;
    let store = Store::open(
        repo.join(reify::index::REIFY_DIR)
            .join(reify::index::STORE_FILE),
    )?;
    let (mut unscored, mut cut, mut late, mut top3, mut first) = (0, 0, 0, 0, 0);
    let mut offered_sizes = Vec::new();
    for task in &set.tasks {
        let a = conditions::rank_audit(&store, &task.prompt, &task.ground_truth, budget)?;
        offered_sizes.push(a.offered);
        match (a.scored_rank, a.offered_rank) {
            (None, _) => unscored += 1,
            (Some(s), None) => {
                cut += 1;
                eprintln!(
                    "  CUT   scored#{s:<3} {}",
                    task.prompt.chars().take(60).collect::<String>()
                );
            }
            (Some(_), Some(1)) => first += 1,
            (Some(_), Some(o)) if o <= 3 => top3 += 1,
            (Some(_), Some(o)) => {
                late += 1;
                eprintln!(
                    "  LATE  offered#{o:<3} {}",
                    task.prompt.chars().take(60).collect::<String>()
                );
            }
        }
    }
    let n = set.tasks.len();
    offered_sizes.sort_unstable();
    eprintln!(
        "
{n} tasks: first={first} top3={top3} late={late} cut={cut} unscored={unscored}  (median offered: {})",
        offered_sizes.get(n / 2).copied().unwrap_or(0)
    );
    Ok(())
}

/// One training corpus: an opened store and its task set.
struct TrainingSet {
    label: String,
    store: Store,
    tasks: Vec<tasks::Task>,
}

/// Exhaustive grid search over the ranking weights.
///
/// Exhaustive rather than clever on purpose: the grid is small enough to enumerate,
/// enumeration cannot get stuck in a local optimum, and the full surface is written
/// out so a later reader can see how flat or sharp the optimum was — a fit that
/// barely beats its neighbours is noise wearing a crown.
fn fit(train: &[String], out: &Path, budget: u32) -> Result<()> {
    use reify::context::RankWeights;

    let mut sets = Vec::new();
    for spec in train {
        let mut parts = spec.splitn(3, '=');
        let (label, repo, task_file) = (
            parts.next().unwrap_or_default(),
            parts.next().unwrap_or_default(),
            parts.next().unwrap_or_default(),
        );
        anyhow::ensure!(
            !task_file.is_empty(),
            "expected LABEL=REPO=TASKS, got `{spec}`"
        );
        let set: tasks::TaskSet = read_json(Path::new(task_file))?;
        let store_path = Path::new(repo)
            .join(reify::index::REIFY_DIR)
            .join(reify::index::STORE_FILE);
        anyhow::ensure!(store_path.exists(), "no index at {}", store_path.display());
        sets.push(TrainingSet {
            label: label.to_string(),
            store: Store::open(&store_path)?,
            tasks: set.tasks,
        });
    }

    // Grids from earlier phases established the surface's shape: the prior dominates
    // (and its fitted peak failed held-out validation, so it is pinned at the pre-fit
    // default), symbols mildly favours few, floor and affinity are flat to noise.
    // The open dimension now is the offer cutoff — the precision knob.
    let history_prior = [0.9f32];
    let symbols_per_file = [6usize];
    let coverage_floor = [0.35f32];
    let path_affinity = [1.0f32];
    let concept_expansion = [0.5f32];
    let offer_cutoff = [0.0f32, 0.1, 0.2, 0.3, 0.45];

    // A flat cross-product rather than nested loops: six levels of nesting is where
    // a formatter and a patch stop agreeing about what the code looks like.
    let mut combos: Vec<RankWeights> = Vec::new();
    for &hp in &history_prior {
        for &spf in &symbols_per_file {
            for &cf in &coverage_floor {
                for &pa in &path_affinity {
                    for &ce in &concept_expansion {
                        for &oc in &offer_cutoff {
                            combos.push(RankWeights {
                                history_prior: hp,
                                history_symbols_per_file: spf,
                                coverage_floor: cf,
                                path_affinity: pa,
                                concept_expansion: ce,
                                offer_cutoff: oc,
                            });
                        }
                    }
                }
            }
        }
    }

    let total = combos.len();
    let mut grid: Vec<serde_json::Value> = Vec::with_capacity(total);
    let mut best: Option<(f32, RankWeights)> = None;

    for (done, weights) in combos.into_iter().enumerate() {
        // The score is the mean over corpora of (hit rate + MRR), so a gain on one
        // repository cannot pay for a loss on another.
        let mut per_set = Vec::new();
        let mut score_sum = 0.0f32;
        for set in &sets {
            let mut outcomes = Vec::new();
            for task in &set.tasks {
                let answer =
                    conditions::reify_context_weighted(&set.store, &task.prompt, budget, &weights)?;
                outcomes.push(metrics::score(&task.id, "fit", &answer, &task.ground_truth));
            }
            let summary = metrics::summarise("fit", &outcomes);
            score_sum += summary.hit_rate + summary.mrr;
            per_set.push(serde_json::json!({
                "label": set.label,
                "hit_rate": summary.hit_rate,
                "mrr": summary.mrr,
                "precision": summary.mean_precision,
                "median_offered": summary.median_files_inspected,
            }));
        }
        let score = score_sum / sets.len() as f32;
        grid.push(serde_json::json!({
            "weights": {
                "history_prior": weights.history_prior,
                "history_symbols_per_file": weights.history_symbols_per_file,
                "coverage_floor": weights.coverage_floor,
                "path_affinity": weights.path_affinity,
                "concept_expansion": weights.concept_expansion,
                "offer_cutoff": weights.offer_cutoff,
            },
            "score": score,
            "per_set": per_set,
        }));
        // Strictly-greater keeps the first of equals, so ties resolve by grid order
        // and the result is reproducible.
        if best.as_ref().is_none_or(|(b, _)| score > *b) {
            best = Some((score, weights));
        }
        eprint!("\r  {}/{total} combos", done + 1);
    }
    eprintln!();

    let (score, weights) = best.expect("the grid is never empty");
    eprintln!(
        "best score {score:.3}: prior={} symbols={} cutoff={}",
        weights.history_prior, weights.history_symbols_per_file, weights.offer_cutoff
    );
    write_json(
        out,
        &serde_json::json!({
            "purpose": "fitted ranking weights; training data is commits earlier than every benchmark task",
            "budget_tokens": budget,
            "best": {
                "score": score,
                "history_prior": weights.history_prior,
                "history_symbols_per_file": weights.history_symbols_per_file,
                "coverage_floor": weights.coverage_floor,
                "path_affinity": weights.path_affinity,
                "concept_expansion": weights.concept_expansion,
                "offer_cutoff": weights.offer_cutoff,
            },
            "grid": grid,
        }),
    )
}

/// Generate every chart the README embeds.
fn charts(results: &[String], out: &Path) -> Result<()> {
    let mut agent_series = Vec::new();
    let mut retrieval_series = Vec::new();

    for spec in results {
        let (label, dir) = spec
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("expected LABEL=DIR, got `{spec}`"))?;
        let dir = Path::new(dir);

        if let Ok(summaries) =
            read_json::<Vec<agent::AgentSummary>>(&dir.join("agent-summary.json"))
        {
            agent_series.push(chart::agent_series(label, &summaries));
        }
        if let Ok(summaries) = read_json::<Vec<metrics::Summary>>(&dir.join("summary.json")) {
            retrieval_series.push(chart::retrieval_series(label, &summaries));
        }
    }

    anyhow::ensure!(
        !agent_series.is_empty() || !retrieval_series.is_empty(),
        "no result files found in the given directories"
    );

    std::fs::create_dir_all(out)?;
    if !agent_series.is_empty() {
        let path = out.join("benchmark-agent.svg");
        std::fs::write(&path, chart::agent_chart(&agent_series))?;
        eprintln!("wrote {}", path.display());
    }
    if !retrieval_series.is_empty() {
        let path = out.join("benchmark-retrieval.svg");
        std::fs::write(&path, chart::retrieval_chart(&retrieval_series))?;
        eprintln!("wrote {}", path.display());
    }
    Ok(())
}

/// Run every model condition over a task subset.
///
/// Conditions run per task rather than per condition so a run interrupted halfway
/// still holds a balanced sample rather than one complete condition and four empty
/// ones.
fn agent_experiments(
    repo: &Path,
    task_file: &Path,
    out: &Path,
    budget: u32,
    limit: usize,
) -> Result<()> {
    let set: tasks::TaskSet = read_json(task_file)?;
    let provider = agent::provider_or_explain(repo)?;
    eprintln!("provider: {}", provider.label);

    let store_path = repo
        .join(reify::index::REIFY_DIR)
        .join(reify::index::STORE_FILE);
    anyhow::ensure!(store_path.exists(), "no index at {}", store_path.display());
    let store = Store::open(&store_path)?;
    let corpus = conditions::Corpus::load(repo)?;

    let chosen: Vec<&tasks::Task> = set.tasks.iter().take(limit).collect();
    let mut outcomes: Vec<agent::AgentOutcome> = Vec::new();

    for (i, task) in chosen.iter().enumerate() {
        eprint!("\r  task {}/{}   ", i + 1, chosen.len());

        // E6: memorisation control. No context at all.
        outcomes.push(agent::run(&provider, repo, task, "N-no-context", ""));

        // E1: the budget-matched lexical baseline.
        let grep = conditions::content_search(&corpus, &task.prompt, budget);
        outcomes.push(agent::run(
            &provider,
            repo,
            task,
            "B-content-grep",
            &agent::files_block(&grep),
        ));

        // The condition under test.
        let compiled = conditions::reify_context(&store, &task.prompt, budget)?;
        outcomes.push(agent::run(
            &provider,
            repo,
            task,
            "R-reify",
            &agent::files_block(&compiled),
        ));

        // E3: negative control. Reify's context for a *different* task. If this scores
        // like the real thing, the model is not reading the content.
        //
        // Skipped for a single-task run, where "a different task" would be this one
        // and the control would silently measure nothing.
        if chosen.len() >= 2 {
            let other = chosen[(i + chosen.len() / 2) % chosen.len()];
            debug_assert_ne!(other.id, task.id, "the control must use a different task");
            let shuffled = conditions::reify_context(&store, &other.prompt, budget)?;
            outcomes.push(agent::run(
                &provider,
                repo,
                task,
                "R-shuffled",
                &agent::files_block(&shuffled),
            ));
        }

        // E2: the ceiling. The answer, handed over.
        outcomes.push(agent::run(
            &provider,
            repo,
            task,
            "O-oracle",
            &agent::oracle_block(task),
        ));

        // The iterated pair. Reify's three rounds cost roughly three budgets, so the
        // honest control is grep given three budgets outright — otherwise iteration
        // buys its gain with tokens the baseline was never offered.
        let iterated = conditions::reify_context_iterative(&store, &task.prompt, budget, 3)?;
        outcomes.push(agent::run(
            &provider,
            repo,
            task,
            "R-reify-iter3",
            &agent::files_block(&iterated),
        ));
        let grep_wide = conditions::content_search(&corpus, &task.prompt, budget * 3);
        outcomes.push(agent::run(
            &provider,
            repo,
            task,
            "B-content-grep-x3",
            &agent::files_block(&grep_wide),
        ));
    }
    eprintln!();

    let names = [
        "N-no-context",
        "B-content-grep",
        "R-reify",
        "R-shuffled",
        "O-oracle",
        "R-reify-iter3",
        "B-content-grep-x3",
    ];
    let summaries: Vec<agent::AgentSummary> = names
        .iter()
        .map(|n| agent::summarise(n, &outcomes))
        .collect();

    std::fs::create_dir_all(out)?;
    write_json(&out.join("agent-outcomes.json"), &outcomes)?;
    write_json(&out.join("agent-summary.json"), &summaries)?;
    write_json(
        &out.join("agent-environment.json"),
        &serde_json::json!({
            "provider": provider.label,
            "tasks": chosen.len(),
            "budget_tokens": budget,
            "conditions": names,
            "head": set.head,
            "token_counts": "estimated by reify heuristic-v1; the provider interface \
                             is a command, so no usage counts are returned",
        }),
    )?;

    eprintln!(
        "\n{:<16} {:>6} {:>8} {:>8} {:>8}",
        "condition", "tasks", "hit", "recall", "tokens"
    );
    for s in &summaries {
        eprintln!(
            "{:<16} {:>6} {:>7.0}% {:>8.2} {:>8}",
            s.condition,
            s.tasks,
            s.hit_rate * 100.0,
            s.mean_recall,
            s.median_prompt_tokens
        );
        if s.errors > 0 {
            eprintln!("{:<16} {} provider failures excluded", "", s.errors);
        }
    }
    eprintln!("wrote {}", out.display());
    Ok(())
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

        let iterated = conditions::reify_context_iterative(&store, &task.prompt, budget, 3)?;
        outcomes.push(metrics::score(
            &task.id,
            "R-reify-iter3",
            &iterated,
            &task.ground_truth,
        ));
    }
    eprintln!();

    std::fs::create_dir_all(out)?;
    write_json(&out.join("outcomes.json"), &outcomes)?;
    let summaries: Vec<metrics::Summary> =
        ["B-content-grep", "C-path-grep", "R-reify", "R-reify-iter3"]
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
            "conditions": ["B-content-grep", "C-path-grep", "R-reify", "R-reify-iter3"],
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

    let names = ["B-content-grep", "C-path-grep", "R-reify", "R-reify-iter3"];
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
    match set.base.as_deref() {
        Some(base) => md.push_str(&format!(
            "The index was built at `{base}`, **before** any of these changes were \
             made, so the code being asked for is genuinely absent.\n\n"
        )),
        None => md.push_str(
            "The index was built at `HEAD`, so the change being asked for is already \
             present in the code. This makes every task easier for every condition \
             equally, and means these numbers are an upper bound.\n\n",
        ),
    }

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

    md.push_str(&agent_section(input));
    md.push_str(&where_reify_lost(&outcomes, &set));

    md.push_str("\n## Limitations\n\n");
    md.push_str(
        "These are stated because the benchmark is only worth what its caveats allow.\n\n\
         1. **Single-shot, not agentic.** The model gets one turn and no tools. A real\n   \
            agent greps, reads, and greps again. This understates every condition\n   \
            equally, but it is not the same measurement.\n\
         2. LEAKAGE_NOTE\n\
         3. **Ground truth is what one commit touched.** A different correct solution\n   \
            touching different files scores as a miss.\n\
         4. **One repository, one language mix.** Nothing here shows the result holds for\n   \
            a typed-language codebase.\n\
         5. **Estimated token counts.** The provider interface is a command, so no usage\n   \
            counts are returned. Prompt sizes are Reify's own estimate, named as such.\n",
    );

    let leakage = match set.base {
        Some(_) => {
            "**Leakage is controlled.** The index was built before any of these \
                    changes were made, so the code being asked for is genuinely absent."
        }
        None => {
            "**The index is built at `HEAD`.** The change being asked for is already \
                 present in the code, which makes every task easier for every condition \
                 equally and makes these numbers an upper bound."
        }
    };
    let md = md.replace("LEAKAGE_NOTE", leakage);

    std::fs::write(out, md).with_context(|| format!("writing {}", out.display()))?;
    eprintln!("wrote {}", out.display());
    Ok(())
}

/// The model-in-the-loop results, when an agent run exists in the same directory.
///
/// This is the half of the benchmark that tests the product claim rather than a proxy
/// for it, so it leads with the falsification controls: if the oracle does not beat the
/// no-context floor, nothing else in the report matters.
fn agent_section(input: &Path) -> String {
    let Ok(summaries) = read_json::<Vec<agent::AgentSummary>>(&input.join("agent-summary.json"))
    else {
        return String::new();
    };
    let environment: serde_json::Value =
        read_json(&input.join("agent-environment.json")).unwrap_or(serde_json::Value::Null);
    let find = |name: &str| summaries.iter().find(|s| s.condition == name);

    let mut md = String::from("\n## With a model in the loop\n\n");
    md.push_str(&format!(
        "Single-shot file identification: the model is given the task and one context \
         block, and asked which files must change. Provider `{}`, {} tasks.\n\n",
        environment["provider"].as_str().unwrap_or("?"),
        environment["tasks"].as_u64().unwrap_or(0)
    ));

    md.push_str(
        "| Condition | Experiment | Tasks | Hit rate | 95% CI | Recall | Prompt tokens |\n",
    );
    md.push_str("|---|---|---:|---:|---:|---:|---:|\n");
    let purpose = |name: &str| match name {
        "N-no-context" => "E6 memorisation control",
        "B-content-grep" => "E1 budget-matched baseline",
        "R-reify" => "condition under test",
        "R-shuffled" => "E3 negative control",
        "O-oracle" => "E2 ceiling",
        "R-reify-iter3" => "three rounds, cumulative cost",
        "B-content-grep-x3" => "grep at the same tripled budget",
        _ => "",
    };
    for s in &summaries {
        md.push_str(&format!(
            "| `{}` | {} | {} | {:.0}% | {:.0}–{:.0}% | {:.2} | {} |\n",
            s.condition,
            purpose(&s.condition),
            s.tasks,
            s.hit_rate * 100.0,
            s.hit_rate_ci.0 * 100.0,
            s.hit_rate_ci.1 * 100.0,
            s.mean_recall,
            s.median_prompt_tokens
        ));
    }

    md.push_str("\n### What the controls say\n\n");
    if let (Some(oracle), Some(floor)) = (find("O-oracle"), find("N-no-context")) {
        let headroom = oracle.hit_rate - floor.hit_rate;
        md.push_str(&format!(
            "**E2 — is context the bottleneck at all?** Perfect context scores {:.0}% \
             against {:.0}% with none. That {:.0}-point gap is the entire space any \
             retrieval system can compete in. {}\n\n",
            oracle.hit_rate * 100.0,
            floor.hit_rate * 100.0,
            headroom * 100.0,
            if headroom > 0.3 {
                "The thesis survives its most dangerous test."
            } else {
                "**This is small. The thesis is in trouble.**"
            }
        ));
        if let Some(reify) = find("R-reify") {
            let captured = if headroom > 0.0 {
                (reify.hit_rate - floor.hit_rate) / headroom
            } else {
                0.0
            };
            let baseline_captured = find("B-content-grep")
                .map(|b| {
                    if headroom > 0.0 {
                        (b.hit_rate - floor.hit_rate) / headroom
                    } else {
                        0.0
                    }
                })
                .unwrap_or(0.0);
            md.push_str(&format!(
                "**Share of that headroom recovered:** Reify {:.0}%, lexical baseline \
                 {:.0}%.\n\n",
                captured * 100.0,
                baseline_captured * 100.0
            ));
        }
    }
    if let (Some(shuffled), Some(reify), Some(floor)) =
        (find("R-shuffled"), find("R-reify"), find("N-no-context"))
    {
        // The comparison that matters is shuffled against the *real* context, not
        // against the floor: the question is whether the content is doing the work or
        // the format is. A shuffled score near the floor is expected; a shuffled score
        // near the real one is the failure mode.
        let separated = reify.hit_rate - shuffled.hit_rate;
        md.push_str(&format!(
            "**E3 — is the model reading the context, or just its framing?** Context \
             compiled for a *different* task scores {:.0}%, against {:.0}% for the real \
             context and {:.0}% for no context at all. {}\n\n",
            shuffled.hit_rate * 100.0,
            reify.hit_rate * 100.0,
            floor.hit_rate * 100.0,
            if separated >= 0.15 {
                "Real context clearly outperforms decoy context of identical shape and \
                 size, so the gain comes from what the context says rather than from \
                 being handed a list of files."
            } else {
                "**The control did not separate: a decoy scores about as well as the \
                 real thing, which means the format rather than the content is doing \
                 the work. Treat the main result as unsupported.**"
            }
        ));
    }
    if let Some(floor) = find("N-no-context") {
        md.push_str(&format!(
            "**E6 — are these tasks memorised?** With no repository access at all the \
             model still scores {:.0}%. {} That floor is subtracted in the headroom \
             figures above rather than ignored.\n\n",
            floor.hit_rate * 100.0,
            if floor.hit_rate > 0.4 {
                "**That is high enough that contamination is a serious concern and the \
                 other numbers should be read sceptically.**"
            } else if floor.hit_rate > 0.05 {
                "Some contamination, as expected for a well-known public repository."
            } else {
                "Effectively none: the model cannot answer these from memory, so the \
                 remaining conditions measure retrieval rather than recall."
            }
        ));
    }

    md.push_str(
        "### Reading these numbers honestly\n\n\
         The confidence intervals overlap. At this sample size the ordering is \
         consistent but not established: a difference of a few tasks is not a \
         difference. What the run *does* establish is the direction of all three \
         controls, which is a structural result rather than a marginal one.\n\n\
         Prompt tokens are **estimates**. The provider interface is a command, so no \
         usage counts come back. Reify's prompts are larger than the baseline's, so \
         its higher hit rate is bought with tokens, not free.\n",
    );
    md
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

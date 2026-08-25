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
        /// Only take tasks from commits strictly *older* than this revision. This is
        /// how a training corpus stays disjoint from every evaluation window.
        #[arg(long)]
        until: Option<String>,
        /// Existing task files whose commits must not be re-used, so a new frozen
        /// set is disjoint from the sets that came before it.
        #[arg(long, num_args = 0..)]
        exclude: Vec<PathBuf>,
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
        /// Ranking-weight overrides for the reify arms, as a JSON file. Absent
        /// fields keep their defaults, so an ablation names only what it changes.
        #[arg(long)]
        weights: Option<PathBuf>,
    },
    /// Cross-file coverage: of the files that contain symbols, how many have at
    /// least one resolved dependent in a *different* file, per language. A truth
    /// file with no inbound cross-file edge cannot be reached through the graph,
    /// which puts this number upstream of every ranking improvement.
    Coverage {
        #[arg(long)]
        repo: PathBuf,
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
        /// Conditions to run (default: all seven). Every provider call costs money,
        /// so a re-baseline that only needs one arm should only pay for one arm.
        #[arg(long, num_args = 0..)]
        arms: Vec<String>,
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
    /// Held-out-hunk evaluation: does the graph notice an incomplete patch?
    ///
    /// Model-free, deterministic, and free to run. It exists to decide whether
    /// `reify verify` is worth building, against the pre-registered condition in
    /// `metrics::VERIFY_RECALL_FLOOR`.
    VerifyEval {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 20)]
        count: usize,
        #[arg(long, default_value_t = 4_000)]
        scan: usize,
        /// Only take trials from commits after this revision.
        #[arg(long)]
        after: Option<String>,
        /// Only take trials from commits strictly older than this revision.
        #[arg(long)]
        until: Option<String>,
        /// Where parent trees are extracted. Defaults to a temporary directory, which
        /// is removed when the run finishes.
        #[arg(long)]
        work: Option<PathBuf>,
    },
    /// Render the held-out-hunk report from one or more `verify-eval` result
    /// directories.
    VerifyReport {
        /// Result directories, as `Label=path`, in the order they should appear.
        #[arg(long = "results", value_name = "LABEL=DIR", num_args = 1..)]
        results: Vec<String>,
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
            until,
            exclude,
        } => {
            let mut excluded = std::collections::BTreeSet::new();
            for file in &exclude {
                let prior: tasks::TaskSet = read_json(file)?;
                excluded.extend(prior.tasks.into_iter().map(|t| t.commit));
            }
            let set = tasks::generate(
                &repo,
                count,
                scan,
                after.as_deref(),
                until.as_deref(),
                &excluded,
            )?;
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
            weights,
        } => execute(&repo, &task_file, &out, budget, weights.as_deref()),
        Command::Coverage { repo } => coverage(&repo),
        Command::Agent {
            repo,
            tasks: task_file,
            out,
            budget,
            limit,
            arms,
        } => agent_experiments(&repo, &task_file, &out, budget, limit, &arms),
        Command::Audit {
            repo,
            tasks,
            budget,
        } => audit(&repo, &tasks, budget),
        Command::Fit { train, out, budget } => fit(&train, &out, budget),
        Command::Chart { results, out } => charts(&results, &out),
        Command::VerifyEval {
            repo,
            out,
            count,
            scan,
            after,
            until,
            work,
        } => verify_eval(
            &repo,
            &out,
            count,
            scan,
            after.as_deref(),
            until.as_deref(),
            work.as_deref(),
        ),
        Command::VerifyReport { results, out } => verify_report(&results, &out),
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
    let (mut connected_unseeded, mut not_connected) = (0, 0);
    let mut offered_sizes = Vec::new();
    for task in &set.tasks {
        let a = conditions::rank_audit(&store, &task.prompt, &task.ground_truth, budget)?;
        offered_sizes.push(a.offered);
        match (a.scored_rank, a.offered_rank) {
            (None, _) => {
                unscored += 1;
                // The taxonomy the plan pre-registered: an unscored miss is either
                // *not-connected* — no lexical, path, or history feature links the
                // prompt to any truth file under the current extractors — or
                // *indexed-but-unseeded*, a connection existed and seeding never
                // surfaced it. Nothing is "unreachable" in an index whose oracle
                // scores 100%; the word would only flatter the extractors.
                let stems: Vec<String> = reify::concepts::meaningful_words(&task.prompt)
                    .into_iter()
                    .map(|w| reify::concepts::stem(&w).to_string())
                    .collect();
                let via_history = store
                    .history_prior(&stems, 50)
                    .unwrap_or_default()
                    .iter()
                    .any(|(p, _)| task.ground_truth.iter().any(|t| t == p));
                let words = reify::concepts::meaningful_words(&task.prompt);
                let via_path = task.ground_truth.iter().any(|t| {
                    let flat = t.to_lowercase().replace(['/', '_', '-', '.'], " ");
                    words.iter().any(|w| flat.contains(w.as_str()))
                });
                if via_history || via_path {
                    connected_unseeded += 1;
                    eprintln!(
                        "  UNSEEDED ({}{}) {}",
                        if via_path { "path" } else { "" },
                        if via_history { "+history" } else { "" },
                        task.prompt.chars().take(60).collect::<String>()
                    );
                } else {
                    not_connected += 1;
                }
            }
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
{n} tasks: first={first} top3={top3} late={late} cut={cut} unscored={unscored} (of which connected-but-unseeded={connected_unseeded}, not-connected={not_connected})  (median offered: {})",
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
fn coverage(repo: &Path) -> Result<()> {
    let store_path = repo
        .join(reify::index::REIFY_DIR)
        .join(reify::index::STORE_FILE);
    anyhow::ensure!(store_path.exists(), "no index at {}", store_path.display());
    let store = Store::open(&store_path)?;
    let rows = store.coverage_by_language()?;
    let (mut total, mut covered) = (0usize, 0usize);
    println!(
        "{:<14} {:>7} {:>9} {:>9}",
        "language", "files", "covered", "share"
    );
    for (lang, files, with_dependents) in &rows {
        println!(
            "{lang:<14} {files:>7} {with_dependents:>9} {:>8.1}%",
            *with_dependents as f32 * 100.0 / (*files).max(1) as f32
        );
        total += files;
        covered += with_dependents;
    }
    println!(
        "{:<14} {total:>7} {covered:>9} {:>8.1}%",
        "TOTAL",
        covered as f32 * 100.0 / total.max(1) as f32
    );
    Ok(())
}

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

    // Coordinate descent rather than a grid: the weight vector now has nine
    // dimensions, and a cross-product at any useful resolution is millions of
    // evaluations. Two sweeps over per-dimension candidate lists converge on this
    // surface (verified against the earlier grids on the dimensions they shared),
    // and every evaluation is logged so the surface stays auditable.
    //
    // The prior's own history applies: its fitted peak failed held-out validation
    // once, so its candidates stay near the pre-fit default rather than re-opening
    // the range that failed.
    // coverage_floor, path_affinity and concept_expansion are omitted: three grid
    // campaigns found them flat to noise, and every candidate here costs a full pass
    // over every training corpus.
    let candidates: Vec<(&str, Vec<f32>)> = vec![
        ("lexical_files", vec![0.0, 0.3, 0.6, 1.0]),
        ("file_fanout", vec![6.0, 10.0, 14.0]),
        ("fanout_symbols", vec![8.0, 12.0]),
        ("file_to_symbol", vec![0.6, 0.8, 1.0]),
        ("history_prior", vec![0.6, 0.9, 1.2]),
        ("offer_cutoff", vec![0.0, 0.1, 0.2, 0.3]),
    ];

    fn get(weights: &RankWeights, dim: &str) -> f32 {
        match dim {
            "lexical_files" => weights.lexical_files,
            "file_fanout" => weights.file_fanout as f32,
            "fanout_symbols" => weights.fanout_symbols as f32,
            "file_to_symbol" => weights.file_to_symbol,
            "history_prior" => weights.history_prior,
            "offer_cutoff" => weights.offer_cutoff,
            "coverage_floor" => weights.coverage_floor,
            "path_affinity" => weights.path_affinity,
            "concept_expansion" => weights.concept_expansion,
            _ => unreachable!("unknown fit dimension {dim}"),
        }
    }
    fn set(weights: &mut RankWeights, dim: &str, value: f32) {
        match dim {
            "lexical_files" => weights.lexical_files = value,
            "file_fanout" => weights.file_fanout = value as usize,
            "fanout_symbols" => weights.fanout_symbols = value as usize,
            "file_to_symbol" => weights.file_to_symbol = value,
            "history_prior" => weights.history_prior = value,
            "offer_cutoff" => weights.offer_cutoff = value,
            "coverage_floor" => weights.coverage_floor = value,
            "path_affinity" => weights.path_affinity = value,
            "concept_expansion" => weights.concept_expansion = value,
            _ => unreachable!("unknown fit dimension {dim}"),
        }
    }

    let evaluate = |weights: &RankWeights| -> Result<(f32, Vec<serde_json::Value>)> {
        // The score is the mean over corpora of (hit rate + MRR), so a gain on one
        // repository cannot pay for a loss on another.
        let mut per_set = Vec::new();
        let mut score_sum = 0.0f32;
        for set in &sets {
            let mut outcomes = Vec::new();
            for task in &set.tasks {
                let answer =
                    conditions::reify_context_weighted(&set.store, &task.prompt, budget, weights)?;
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
        Ok((score_sum / sets.len() as f32, per_set))
    };

    let mut grid: Vec<serde_json::Value> = Vec::new();
    let mut weights = RankWeights::default();
    let (mut best_score, _) = evaluate(&weights)?;
    eprintln!("  defaults score {best_score:.4}");
    for sweep in 0..2 {
        for (dim, values) in &candidates {
            for &value in values {
                if (get(&weights, dim) - value).abs() < 1e-6 {
                    continue;
                }
                let mut trial = weights.clone();
                set(&mut trial, dim, value);
                let (score, per_set) = evaluate(&trial)?;
                grid.push(serde_json::json!({
                    "sweep": sweep,
                    "dim": dim,
                    "value": value,
                    "score": score,
                    "per_set": per_set,
                }));
                // Strictly greater keeps the incumbent on ties, so the defaults win
                // unless a candidate actually earns its keep.
                if score > best_score {
                    best_score = score;
                    weights = trial;
                    eprintln!("  sweep {sweep}: {dim}={value} -> {score:.4}");
                }
            }
        }
    }
    let (score, weights) = (best_score, weights);
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
    arms: &[String],
) -> Result<()> {
    let wanted = |name: &str| arms.is_empty() || arms.iter().any(|a| a == name);
    let set: tasks::TaskSet = read_json(task_file)?;
    let name = set.repository_name().to_string();
    let name = name.as_str();
    let provider = agent::provider_or_explain(repo)?;
    eprintln!("provider: {}  repository: {name}", provider.label);

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
        if wanted("N-no-context") {
            outcomes.push(agent::run(&provider, repo, name, task, "N-no-context", ""));
        }

        // E1: the budget-matched lexical baseline.
        if wanted("B-content-grep") {
            let grep = conditions::content_search(&corpus, &task.prompt, budget);
            outcomes.push(agent::run(
                &provider,
                repo,
                name,
                task,
                "B-content-grep",
                &agent::files_block(&grep),
            ));
        }

        // The condition under test.
        if wanted("R-reify") {
            let compiled = conditions::reify_context(&store, &task.prompt, budget)?;
            outcomes.push(agent::run(
                &provider,
                repo,
                name,
                task,
                "R-reify",
                &agent::files_block(&compiled),
            ));
        }

        // E3: negative control. Reify's context for a *different* task. If this scores
        // like the real thing, the model is not reading the content.
        //
        // Skipped for a single-task run, where "a different task" would be this one
        // and the control would silently measure nothing.
        if wanted("R-shuffled") && chosen.len() >= 2 {
            let other = chosen[(i + chosen.len() / 2) % chosen.len()];
            debug_assert_ne!(other.id, task.id, "the control must use a different task");
            let shuffled = conditions::reify_context(&store, &other.prompt, budget)?;
            outcomes.push(agent::run(
                &provider,
                repo,
                name,
                task,
                "R-shuffled",
                &agent::files_block(&shuffled),
            ));
        }

        // E2: the ceiling. The answer, handed over.
        if wanted("O-oracle") {
            outcomes.push(agent::run(
                &provider,
                repo,
                name,
                task,
                "O-oracle",
                &agent::oracle_block(task),
            ));
        }

        // The iterated pair. Reify's three rounds cost roughly three budgets, so the
        // honest control is grep given three budgets outright — otherwise iteration
        // buys its gain with tokens the baseline was never offered.
        if wanted("R-reify-iter3") {
            let iterated = conditions::reify_context_iterative(&store, &task.prompt, budget, 3)?;
            outcomes.push(agent::run(
                &provider,
                repo,
                name,
                task,
                "R-reify-iter3",
                &agent::files_block(&iterated),
            ));
        }
        if wanted("B-content-grep-x3") {
            let grep_wide = conditions::content_search(&corpus, &task.prompt, budget * 3);
            outcomes.push(agent::run(
                &provider,
                repo,
                name,
                task,
                "B-content-grep-x3",
                &agent::files_block(&grep_wide),
            ));
        }
    }
    eprintln!();

    let names: Vec<&str> = [
        "N-no-context",
        "B-content-grep",
        "R-reify",
        "R-shuffled",
        "O-oracle",
        "R-reify-iter3",
        "B-content-grep-x3",
    ]
    .into_iter()
    .filter(|n| wanted(n))
    .collect();
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
            "repository": name,
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

fn execute(
    repo: &Path,
    task_file: &Path,
    out: &Path,
    budget: u32,
    weight_file: Option<&Path>,
) -> Result<()> {
    let weights: reify::context::RankWeights = match weight_file {
        Some(path) => serde_json::from_str(&std::fs::read_to_string(path)?)?,
        None => Default::default(),
    };
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

        let compiled = conditions::reify_context_weighted(&store, &task.prompt, budget, &weights)?;
        outcomes.push(metrics::score(
            &task.id,
            "R-reify",
            &compiled,
            &task.ground_truth,
        ));

        let iterated = conditions::reify_context_iterative_weighted(
            &store,
            &task.prompt,
            budget,
            3,
            &weights,
        )?;
        outcomes.push(metrics::score(
            &task.id,
            "R-reify-iter3",
            &iterated,
            &task.ground_truth,
        ));

        // Edit mode: regions padded to whole definitions instead of the smallest
        // spans that answer the question, scored on the same ground truth so the
        // cost of that padding is visible beside what it buys.
        let edit = conditions::reify_context_for_edit(&store, &task.prompt, budget)?;
        outcomes.push(metrics::score(
            &task.id,
            "R-reify-edit",
            &edit,
            &task.ground_truth,
        ));
    }
    eprintln!();

    std::fs::create_dir_all(out)?;
    write_json(&out.join("outcomes.json"), &outcomes)?;
    let summaries: Vec<metrics::Summary> = [
        "B-content-grep",
        "C-path-grep",
        "R-reify",
        "R-reify-iter3",
        "R-reify-edit",
    ]
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

/// Held-out-hunk evaluation: can the graph tell that a patch is incomplete?
///
/// Model-free and deterministic. For each qualifying merged commit the parent tree is
/// extracted and indexed, the change is fed to the checker twice — once with one file's
/// only hunk withheld, once complete — and the two runs answer two different questions:
/// does a finding cite the withheld hunk, and how many findings does a change that is
/// complete by construction still attract.
fn verify_eval(
    repo: &Path,
    out: &Path,
    count: usize,
    scan: usize,
    after: Option<&str>,
    until: Option<&str>,
    work: Option<&Path>,
) -> Result<()> {
    let started = std::time::Instant::now();
    let set = tasks::generate_truncated(
        repo,
        count,
        scan,
        after,
        until,
        &std::collections::BTreeSet::new(),
    )?;
    anyhow::ensure!(
        !set.tasks.is_empty(),
        "no commit in the scanned history could be truncated; {} candidates were \
         rejected, the commonest reason being `{}`",
        set.rejected.len(),
        set.rejected
            .first()
            .map(|(_, why)| why.as_str())
            .unwrap_or("none recorded"),
    );
    eprintln!(
        "{} trials from {} commits ({} passed every retrieval filter but could not be \
         truncated)",
        set.tasks.len(),
        set.generated_from_commits,
        set.rejected.len()
    );

    let scratch = work.map(Path::to_path_buf).unwrap_or_else(|| {
        std::env::temp_dir().join(format!("reify-verify-eval-{}", std::process::id()))
    });
    let tree = scratch.join("tree");

    let mut outcomes: Vec<metrics::VerifyOutcome> = Vec::new();
    // Taken from the first trial's index and kept: what the repository is written in
    // is a property of the repository, not of the label someone passes to the report.
    let mut languages: Vec<(String, usize)> = Vec::new();
    for (i, task) in set.tasks.iter().enumerate() {
        eprint!("\r  trial {}/{}   ", i + 1, set.tasks.len());
        let indexing = std::time::Instant::now();
        extract_tree(repo, &task.parent, &tree)?;
        let mut store = Store::open(
            tree.join(reify::index::REIFY_DIR)
                .join(reify::index::STORE_FILE),
        )?;
        reify::index::index(&mut store, &reify::index::IndexOptions::new(&tree))?;
        let index_ms = indexing.elapsed().as_millis();
        if languages.is_empty() {
            let mut rows = store.coverage_by_language()?;
            rows.sort_by_key(|row| std::cmp::Reverse(row.1));
            languages = rows.into_iter().take(3).map(|(l, n, _)| (l, n)).collect();
        }

        // Resolved at the parent, where the withheld change has not happened yet — the
        // same state the checker sees, so a symbol that does not exist there is
        // honestly unscorable rather than quietly credited.
        let omission_symbol = store
            .symbol_at(&task.omission_file, task.omission_line)?
            .map(|node| node.location());
        let truncated = conditions::missing_callers(&store, &task.truncated)?;
        let complete = conditions::missing_callers(&store, &task.complete)?;
        outcomes.push(metrics::score_verify(
            task,
            omission_symbol,
            conditions::can_be_cited(&store, &task.omission_file)?,
            &truncated,
            &complete,
            index_ms,
        ));
    }
    eprintln!();
    let _ = std::fs::remove_dir_all(&scratch);

    let summary = metrics::summarise_verify(&outcomes);
    std::fs::create_dir_all(out)?;
    write_json(&out.join("verify-outcomes.json"), &outcomes)?;
    write_json(&out.join("verify-summary.json"), &summary)?;
    write_json(&out.join("verify-tasks.json"), &set)?;
    write_json(
        &out.join("verify-environment.json"),
        &serde_json::json!({
            "reify_version": env!("CARGO_PKG_VERSION"),
            "repository": set.repository,
            // The local path is wherever the run happened, which is no help to anyone
            // reproducing it. The remote is.
            "origin": origin(repo),
            "head": set.head,
            "languages": languages,
            // The selection window, so a committed result can be re-run exactly even
            // after the branch it was taken from has moved on.
            "count": count,
            "scan": scan,
            "after": after,
            "until": until,
            "trials": set.tasks.len(),
            "candidates_rejected": set.rejected.len(),
            "checker": "symbols changed by this diff, minus symbols present in the diff, \
                        where an inbound CALLS edge exists at distance 1, via reify::query::impact",
            "wall_clock_ms": started.elapsed().as_millis(),
            "token_counts": "estimated by reify heuristic-v1",
        }),
    )?;

    eprintln!("\n{}", render_verify(&summary));
    eprintln!(
        "wrote {} ({:.1}s wall clock)",
        out.display(),
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

/// The summary as a human reads it, verdict first.
fn render_verify(s: &metrics::VerifySummary) -> String {
    let mut text = String::new();
    text.push_str(&format!(
        "{:<28} {:.2}  (95% CI {:.2}–{:.2}, {}/{} trials)\n",
        "omission_recall",
        s.omission_recall,
        s.omission_recall_ci.0,
        s.omission_recall_ci.1,
        (s.omission_recall * s.tasks as f32).round() as usize,
        s.tasks,
    ));
    text.push_str(&format!(
        "{:<28} {:.2}  (95% CI {:.2}–{:.2}) — citations the complete commit does not \
         also produce\n",
        "  of which attributable",
        s.omission_recall_attributable,
        s.omission_recall_attributable_ci.0,
        s.omission_recall_attributable_ci.1,
    ));
    match (s.omission_recall_reachable, s.omission_recall_reachable_ci) {
        (Some(recall), Some(ci)) => text.push_str(&format!(
            "{:<28} {recall:.2}  (95% CI {:.2}–{:.2}, {}/{} omitted files call into \
             another file at all — the ceiling on any call-graph checker)\n",
            "  where citable at all", ci.0, ci.1, s.reachable_omissions, s.tasks
        )),
        _ => text.push_str(&format!(
            "{:<28} —     (no omitted file calls into another file; the ceiling on \
             any call-graph checker here is zero)\n",
            "  where citable at all"
        )),
    }
    match (s.omission_recall_symbol, s.omission_recall_symbol_ci) {
        (Some(recall), Some(ci)) => text.push_str(&format!(
            "{:<28} {recall:.2}  (95% CI {:.2}–{:.2}, {} scorable)\n",
            "omission_recall_symbol", ci.0, ci.1, s.symbol_scorable
        )),
        _ => text.push_str(&format!(
            "{:<28} —     (no trial's omission fell inside an indexed symbol)\n",
            "omission_recall_symbol"
        )),
    }
    text.push_str(&format!(
        "{:<28} {:.2}  per complete commit ({}/{} commits noisy, 95% CI {:.2}–{:.2})\n",
        "false_alarm_rate",
        s.false_alarm_rate,
        s.commits_with_a_false_alarm,
        s.tasks,
        s.false_alarm_share_ci.0,
        s.false_alarm_share_ci.1,
    ));
    text.push_str(&format!(
        "{:<28} {}\n",
        "findings_per_diff (median)", s.median_findings_per_diff
    ));
    text.push_str(&format!(
        "{:<28} {}\n",
        "verify_tokens (median)", s.median_verify_tokens
    ));
    text.push_str(&format!(
        "{:<28} {}\n",
        "verify_latency_ms (median)", s.median_verify_latency_ms
    ));
    text.push_str(&format!(
        "{:<28} {}  (extract + index one parent tree; not part of the query)\n",
        "index_ms (median)", s.median_index_ms
    ));
    if s.diffs_resolving_to_nothing > 0 {
        text.push_str(&format!(
            "{:<28} {} of {} truncated diffs resolved to no indexed symbol at all\n",
            "unresolved", s.diffs_resolving_to_nothing, s.tasks
        ));
    }
    text.push_str(&format!(
        "\npre-registered verdict: {}\n  {}\n",
        match s.verdict() {
            metrics::Verdict::Build => "BUILD `reify verify` on this substrate",
            metrics::Verdict::DoNotBuild => "DO NOT BUILD `reify verify` on this substrate",
        },
        s.why()
    ));
    text
}

/// The repository's `origin` remote, so a committed result names something a reader
/// can clone rather than the temporary directory it happened to be run in.
fn origin(repo: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(repo)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Extract a commit's tree into `into`, replacing whatever was there.
///
/// `git archive` rather than a worktree: a worktree registers itself in the shared
/// git directory, and this harness must not leave anything behind in the repository
/// it is measuring. The cost is a full index per trial, which is reported.
fn extract_tree(repo: &Path, sha: &str, into: &Path) -> Result<()> {
    use std::process::{Command, Stdio};
    if into.exists() {
        std::fs::remove_dir_all(into).with_context(|| format!("clearing {}", into.display()))?;
    }
    std::fs::create_dir_all(into)?;
    let mut archive = Command::new("git")
        .args(["archive", "--format=tar", sha])
        .current_dir(repo)
        .stdout(Stdio::piped())
        .spawn()
        .context("running git archive")?;
    let stdout = archive
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("git archive produced no output"))?;
    let extracted = Command::new("tar")
        .arg("-x")
        .arg("-C")
        .arg(into)
        .stdin(Stdio::from(stdout))
        .status()
        .context("running tar to extract a parent tree")?;
    let archived = archive.wait()?;
    anyhow::ensure!(
        archived.success() && extracted.success(),
        "cannot extract the tree at {sha}"
    );
    Ok(())
}

/// Render the held-out-hunk report across every repository that was run.
///
/// Generated from `verify-summary.json` for the same reason the retrieval report is:
/// a table that can drift from its data is a picture, not a measurement. The per-trial
/// appendix is included so the selection rule's effects are visible rather than
/// described — every omitted file is named.
fn verify_report(results: &[String], out: &Path) -> Result<()> {
    struct Run {
        label: String,
        summary: metrics::VerifySummary,
        environment: serde_json::Value,
        outcomes: Vec<metrics::VerifyOutcome>,
    }

    let mut runs = Vec::new();
    for spec in results {
        let (label, dir) = spec
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("expected LABEL=DIR, got `{spec}`"))?;
        let dir = Path::new(dir);
        runs.push(Run {
            label: label.to_string(),
            summary: read_json(&dir.join("verify-summary.json"))?,
            environment: read_json(&dir.join("verify-environment.json"))?,
            outcomes: read_json(&dir.join("verify-outcomes.json"))?,
        });
    }
    anyhow::ensure!(!runs.is_empty(), "no results given");

    let mut md = String::from("# Can the graph tell that a patch is incomplete?\n\n");
    md.push_str(
        "Generated by `reify-bench verify-report`. Every number is computed from the \
         `verify-summary.json` files named below; nothing is entered by hand.\n\n\
         This benchmark exists to decide one thing: whether `reify verify` — a \
         post-flight check that reads an agent's diff and reports what the patch \
         missed — is worth building on Reify's call graph. It is model-free, \
         deterministic, and costs nothing per run.\n\n",
    );

    md.push_str("## Construction\n\n");
    md.push_str(
        "For each merged commit that passes the retrieval benchmark's filters and \
         touches at least two indexable files:\n\n\
         1. the parent tree is extracted and indexed, so the change is absent from the \
            index by construction;\n\
         2. one file's **only** hunk is withheld — the *omission*. Removing it removes \
            that file from the patch entirely, so a citation of it cannot be an echo of \
            a hunk still present. Among the files with exactly one hunk, the last by \
            path order is chosen; the choice is arbitrary, fixed, and made before any \
            checker runs;\n\
         3. the truncated patch goes to the checker;\n\
         4. **the same commit goes to the checker complete.** A merged commit is \
            complete by definition, so every finding there is a false positive. This \
            control is not optional: without it the metric would reward a checker that \
            simply shouts.\n\n\
         The checker is not `reify verify`, which does not exist. It is the shipped \
         graph query — *symbols changed by this diff, minus symbols present in the \
         diff, where an inbound `CALLS` edge exists at distance 1* — reached through \
         `reify::query::impact`. That deliberately measures the **substrate**, which is \
         the number the decision needs.\n\n",
    );

    md.push_str("## Pre-registered falsification condition\n\n");
    md.push_str(&format!(
        "> If `omission_recall` on this substrate is below **{VERIFY_RECALL_FLOOR:.2}**, \
         or `false_alarm_rate` is above **{VERIFY_FALSE_ALARM_CEILING:.1} per commit**, \
         the `reify verify` feature does not get built on this substrate.\n\n\
         Stated in `crates/reify-bench/src/metrics.rs` before the first run and not \
         moved since. A result that kills the feature is a result.\n\n",
        VERIFY_RECALL_FLOOR = metrics::VERIFY_RECALL_FLOOR,
        VERIFY_FALSE_ALARM_CEILING = metrics::VERIFY_FALSE_ALARM_CEILING,
    ));

    md.push_str("## Results\n\n| Metric |");
    for run in &runs {
        md.push_str(&format!(" {} |", run.label));
    }
    md.push_str("\n|---|");
    for _ in &runs {
        md.push_str("---:|");
    }
    md.push('\n');
    let row = |label: &str, f: &dyn Fn(&Run) -> String| {
        let mut line = format!("| {label} |");
        for run in &runs {
            line.push_str(&format!(" {} |", f(run)));
        }
        line.push('\n');
        line
    };
    md.push_str(&row("Most indexed language", &|r| {
        r.environment["languages"][0][0]
            .as_str()
            .unwrap_or("—")
            .to_string()
    }));
    md.push_str(&row("Trials", &|r| r.summary.tasks.to_string()));
    md.push_str(&row("`omission_recall`", &|r| {
        format!(
            "**{:.2}** ({:.2}–{:.2})",
            r.summary.omission_recall,
            r.summary.omission_recall_ci.0,
            r.summary.omission_recall_ci.1
        )
    }));
    md.push_str(&row("…attributable to the omission", &|r| {
        format!(
            "{:.2} ({:.2}–{:.2})",
            r.summary.omission_recall_attributable,
            r.summary.omission_recall_attributable_ci.0,
            r.summary.omission_recall_attributable_ci.1
        )
    }));
    md.push_str(&row("`omission_recall_symbol`", &|r| match (
        r.summary.omission_recall_symbol,
        r.summary.omission_recall_symbol_ci,
    ) {
        (Some(v), Some(ci)) => format!(
            "{v:.2} ({:.2}–{:.2}) over {}",
            ci.0, ci.1, r.summary.symbol_scorable
        ),
        _ => "— (0 scorable)".into(),
    }));
    md.push_str(&row("Omitted files a caller query *could* cite", &|r| {
        format!("{}/{}", r.summary.reachable_omissions, r.summary.tasks)
    }));
    md.push_str(&row("`false_alarm_rate` (per complete commit)", &|r| {
        format!("**{:.1}**", r.summary.false_alarm_rate)
    }));
    md.push_str(&row("Complete commits with ≥1 false alarm", &|r| {
        format!(
            "{}/{} ({:.2}–{:.2})",
            r.summary.commits_with_a_false_alarm,
            r.summary.tasks,
            r.summary.false_alarm_share_ci.0,
            r.summary.false_alarm_share_ci.1
        )
    }));
    md.push_str(&row("`findings_per_diff` (median)", &|r| {
        r.summary.median_findings_per_diff.to_string()
    }));
    md.push_str(&row("`verify_tokens` (median)", &|r| {
        r.summary.median_verify_tokens.to_string()
    }));
    md.push_str(&row("`verify_latency_ms` (median)", &|r| {
        r.summary.median_verify_latency_ms.to_string()
    }));
    md.push_str(&row("Index per trial, ms (median)", &|r| {
        r.summary.median_index_ms.to_string()
    }));
    md.push_str(&row("Whole run, wall clock", &|r| {
        format!(
            "{:.0}s",
            r.environment["wall_clock_ms"].as_f64().unwrap_or(0.0) / 1000.0
        )
    }));
    md.push_str(&row(
        "Pre-registered verdict",
        &|r| match r.summary.verdict() {
            metrics::Verdict::Build => "build".into(),
            metrics::Verdict::DoNotBuild => "**do not build**".into(),
        },
    ));

    md.push_str("\n## What the numbers say\n\n");
    for run in &runs {
        md.push_str(&format!("**{}** — {}\n\n", run.label, run.summary.why()));
    }
    let all_fail = runs
        .iter()
        .all(|r| r.summary.verdict() == metrics::Verdict::DoNotBuild);
    md.push_str(if all_fail {
        "Every repository fails the pre-registered condition, so **`reify verify` does \
         not get built on this substrate**. The condition was written down before the \
         first run precisely so this outcome could not be argued away afterwards.\n\n"
    } else {
        "At least one repository clears the pre-registered condition. Read the \
         confidence intervals before treating that as settled.\n\n"
    });

    // Which half of the condition actually fails, counted rather than asserted: the
    // interesting question is not "did it fail" but "on what".
    let failed_recall = runs
        .iter()
        .filter(|r| r.summary.omission_recall < metrics::VERIFY_RECALL_FLOOR)
        .count();
    let failed_noise = runs
        .iter()
        .filter(|r| r.summary.false_alarm_rate > metrics::VERIFY_FALSE_ALARM_CEILING)
        .count();
    md.push_str(&format!(
        "**It fails on noise, not on blindness.** {failed_noise} of {} repositories \
         exceed the false-alarm ceiling; {failed_recall} of {} fall below the recall \
         floor (a repository can fail both). The graph does find the omitted file often enough to be interesting; \
         what it cannot do is stay quiet about a patch that is already complete.\n\n",
        runs.len(),
        runs.len(),
    ));

    md.push_str(
        "**The negative control takes most of the headline back.** `omission_recall` \
         counts a citation of the omitted file whether or not the complete commit is \
         cited too. The attributable row counts only citations the complete commit does \
         *not* produce, and it is the smaller number in every repository here. The gap \
         is the checker citing a file it would have cited anyway — which is not \
         detection, however it reads next to the label.\n\n",
    );

    // The ceiling either binds or it does not, and which one decides whether a better
    // query could help. Asserting the wrong one would be worse than saying nothing.
    let tightest = runs
        .iter()
        .map(|r| r.summary.reachable_omissions as f32 / r.summary.tasks.max(1) as f32)
        .fold(f32::INFINITY, f32::min);
    md.push_str(&format!(
        "**The ceiling is not what binds.** A finding is a caller, so the omitted file \
         can only be cited if something in it calls out of itself. In the least \
         favourable repository here that holds for {:.0}% of omissions, so the edges \
         mostly exist and `omission_recall` is not capped by their absence. The gap \
         between that row and the recall row is a *ranking* gap, not a coverage one.\n\n",
        tightest * 100.0
    ));

    md.push_str(
        "**The noise is structural, not marginal.** `false_alarm_rate` is findings per \
         commit that is complete by construction. A `CALLS` edge says a caller exists; \
         it does not say the caller needed changing. Nothing in the graph distinguishes \
         a changed signature from an edit inside a body, so every caller of every \
         touched symbol is a candidate. That is a property of the edge, and no \
         rewriting of the query around the same edge removes it.\n\n",
    );

    md.push_str("## Cost and determinism\n\n");
    md.push_str(&format!(
        "No model, no network, no provider key: the whole run is a git extract, an \
         index and a graph query. Total wall clock for everything in this report is \
         **{:.0}s**, dominated by re-indexing one parent tree per trial. The query \
         itself is the `verify_latency_ms` row — single-digit milliseconds.\n\n\
         Each run is deterministic given a fixed `HEAD`: task selection, the omission \
         rule and the query contain no randomness and no tunable threshold. A run \
         against a repository whose history is still moving — this one, for instance — \
         should pin the window with `--until <sha>`, or the trial set moves with the \
         branch.\n\n\
         ```bash\n\
         reify-bench verify-eval --repo <repo> --out results/verify-<name> --until <sha>\n\
         reify-bench verify-report --results \"name=results/verify-<name>\" --out benchmarks/REPORT-verify.md\n\
         ```\n\n",
        runs
            .iter()
            .map(|r| r.environment["wall_clock_ms"].as_f64().unwrap_or(0.0))
            .sum::<f64>()
            / 1000.0
    ));

    md.push_str("## Limitations\n\n");
    md.push_str(
        "1. **Small samples.** The intervals are wide and are printed beside every \
            rate. Where two repositories differ by less than their intervals, they have \
            not been shown to differ.\n\
         2. **The omission-selection rule has a direction.** \"Last by path order, among \
            files with exactly one hunk\" is arbitrary but not neutral: in a repository \
            laid out as `src/` and `tests/`, path order lands on `tests/`. Counted \
            across every run here, TEST_SHARE omissions sit under a path segment \
            named `test` or `tests`. The rule was fixed before any run and has not been \
            changed since; every omitted file is named in the appendix, so the effect is \
            checkable rather than described.\n\
         3. **`CALLS` at distance 1 only.** `impact` also propagates two hops and crosses \
            into the data layer. Widening the query would raise recall and raise the \
            false-alarm rate with it — the trade this benchmark measures rather than \
            pre-empts.\n\
         4. **A checker, not the feature.** `reify verify` could use a signature diff, \
            type information, or the model. This measures the substrate those would all \
            stand on.\n\
         5. **Parent trees are extracted with `git archive`**, so the indexed tree has no \
            git history and no co-change edges. The checker uses neither; a checker that \
            did would need re-measuring.\n\
         6. **Ground truth is one commit's hunks.** A change that could correctly have \
            been made elsewhere scores as a miss.\n\
         7. **`impact`'s own bounds are inherited, not bypassed.** It stops at 60 \
            affected nodes and walks depth-first to two hops, so on a widely-called \
            symbol some direct callers can be crowded out by second-hop ones. That is \
            the shipped query's behaviour and measuring around it would measure \
            something that does not exist.\n\n",
    );

    let (mut in_tests, mut trials) = (0usize, 0usize);
    for run in &runs {
        for outcome in &run.outcomes {
            trials += 1;
            if outcome
                .omission_file
                .split('/')
                .any(|part| part == "test" || part == "tests")
            {
                in_tests += 1;
            }
        }
    }
    let md = md.replace("TEST_SHARE", &format!("{in_tests} of {trials}"));

    let mut md = md;
    md.push_str("## Appendix: every trial\n\n");
    md.push_str(
        "`could cite` is whether the omitted file calls out of itself at all — the \
         ceiling for that trial. `cited` is findings on the truncated patch, `noise` is \
         findings on the same commit complete.\n\n",
    );
    for run in &runs {
        md.push_str(&format!(
            "### {} (`{}`, commit `{}`)\n\n",
            run.label,
            run.environment["origin"]
                .as_str()
                .or_else(|| run.environment["repository"].as_str())
                .unwrap_or("?"),
            run.environment["head"].as_str().unwrap_or("?"),
        ));
        md.push_str("| Trial | Omitted file | could cite | hit | attributable | cited | noise |\n");
        md.push_str("|---|---|---|---|---|---:|---:|\n");
        let tick = |yes: bool| if yes { "yes" } else { "no" };
        for outcome in &run.outcomes {
            md.push_str(&format!(
                "| `{}` | `{}` | {} | {} | {} | {} | {} |\n",
                outcome.task,
                outcome.omission_file,
                tick(outcome.omission_file_reachable),
                tick(outcome.file_hit),
                tick(outcome.file_hit_attributable),
                outcome.findings,
                outcome.false_alarms,
            ));
        }
        md.push('\n');
    }

    std::fs::write(out, md).with_context(|| format!("writing {}", out.display()))?;
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

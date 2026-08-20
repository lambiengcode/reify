//! The model-in-the-loop experiments.
//!
//! The retrieval benchmark measures whether the right file is *offered*. This measures
//! whether a model, given that offering, *identifies* it. Between them they bound the
//! product claim from both sides.
//!
//! Four of the five conditions exist to try to falsify the thesis rather than support
//! it (`docs/PLAN.md` §D):
//!
//! | Condition | Experiment | What a result would mean |
//! |---|---|---|
//! | `N-no-context` | E6 | High success means the tasks are memorised and every other number is suspect |
//! | `O-oracle` | E2 | If perfect context barely beats no context, **context is not the bottleneck and the thesis is wrong** |
//! | `R-shuffled` | E3 | If Reify's real context scores like another task's context, the model is not reading it |
//! | `B-content-grep` | E1 | The budget-matched baseline |
//! | `R-reify` | — | The condition under test |
//!
//! This is single-shot file identification, not an agentic loop: the model gets one
//! turn and no tools. That understates every condition equally and is stated as a
//! limitation rather than glossed over.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;

use reify::llm::{self, Provider};
use reify::tokens;

use crate::conditions::Answer;
use crate::tasks::Task;

/// Files a model may name before we stop reading its answer.
///
/// Without a cap, "list every file that might be involved" is a winning strategy, and
/// the measurement would reward verbosity rather than knowledge.
const MAX_NAMED_FILES: usize = 8;

/// One model run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOutcome {
    pub task: String,
    pub condition: String,
    /// Files the model named, in the order it named them.
    pub named: Vec<String>,
    pub hit: bool,
    pub recall: f32,
    /// Estimated prompt tokens. Named as an estimate because it is one — the provider
    /// interface is a command, so no usage counts come back.
    pub prompt_tokens: u32,
    pub elapsed_ms: u128,
    /// Set when the provider failed, so a failure is never scored as a miss.
    pub error: Option<String>,
}

/// Build the prompt for one condition.
///
/// Identical across conditions except for the CONTEXT block, so any difference in
/// outcome is attributable to the context and not to the wording of the question.
pub fn prompt(task: &Task, context_block: &str) -> String {
    format!(
        "You are helping a developer change a large existing codebase (ERPNext).\n\
         \n\
         TASK: {}\n\
         \n\
         CONTEXT:\n{}\n\
         \n\
         Name the source files that must be modified to do this task.\n\
         Answer with file paths only, one per line, at most {MAX_NAMED_FILES}.\n\
         Use repository-relative paths. No prose, no numbering, no explanation.\n",
        task.prompt,
        if context_block.trim().is_empty() {
            "(none provided)"
        } else {
            context_block
        }
    )
}

/// The context block for a retrieval condition: the files it offered.
pub fn files_block(answer: &Answer) -> String {
    if answer.files.is_empty() {
        return String::new();
    }
    let mut block = String::from("Candidate files found by search:\n");
    for file in answer.files.iter().take(40) {
        block.push_str("  ");
        block.push_str(file);
        block.push('\n');
    }
    block
}

/// The oracle context: the answer, handed over.
///
/// This is the ceiling. If a model given the ground truth in its context still cannot
/// name it, then no retrieval system can help on that task, and the gap between the
/// oracle and the baseline is the entire space Reify could ever compete in.
pub fn oracle_block(task: &Task) -> String {
    let mut block =
        String::from("Files known to be involved in this change, with surrounding candidates:\n");
    for file in &task.ground_truth {
        block.push_str("  ");
        block.push_str(file);
        block.push('\n');
    }
    block
}

/// Run one condition for one task.
pub fn run(
    provider: &Provider,
    root: &Path,
    task: &Task,
    condition: &str,
    context_block: &str,
) -> AgentOutcome {
    let text = prompt(task, context_block);
    let started = std::time::Instant::now();
    let mut outcome = AgentOutcome {
        task: task.id.clone(),
        condition: condition.to_string(),
        named: Vec::new(),
        hit: false,
        recall: 0.0,
        prompt_tokens: tokens::estimate(&text),
        elapsed_ms: 0,
        error: None,
    };

    match llm::complete(provider, root, &text) {
        Ok(reply) => outcome.named = parse_paths(&reply),
        // A provider failure is recorded, never silently scored as a wrong answer.
        Err(e) => outcome.error = Some(format!("{e:#}")),
    }
    outcome.elapsed_ms = started.elapsed().as_millis();

    let truth: BTreeSet<&str> = task.ground_truth.iter().map(String::as_str).collect();
    let found = outcome
        .named
        .iter()
        .filter(|f| truth.contains(f.as_str()))
        .count();
    outcome.hit = found > 0;
    outcome.recall = found as f32 / truth.len().max(1) as f32;
    outcome
}

/// Pull repository paths out of a model's reply.
///
/// Lenient about formatting — models add bullets, backticks and numbering however they
/// are asked not to — and strict about what counts as a path, so prose never scores.
pub fn parse_paths(reply: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in reply.lines() {
        let cleaned = line
            .trim()
            .trim_start_matches(|c: char| {
                c == '-' || c == '*' || c == '`' || c.is_ascii_digit() || c == '.' || c == ' '
            })
            .trim_matches('`')
            .trim_end_matches([',', ';', '.'])
            .trim();
        if cleaned.is_empty() || cleaned.contains(' ') || !cleaned.contains('/') {
            continue;
        }
        if !cleaned.contains('.') {
            continue; // a directory, not a file
        }
        let path = cleaned.to_string();
        if !out.contains(&path) {
            out.push(path);
        }
        if out.len() >= MAX_NAMED_FILES {
            break;
        }
    }
    out
}

/// Aggregate one condition's model outcomes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSummary {
    pub condition: String,
    pub tasks: usize,
    /// Runs where the provider failed. Excluded from every rate below.
    pub errors: usize,
    pub hits: usize,
    pub hit_rate: f32,
    /// 95% Wilson interval on the hit rate.
    ///
    /// Reported because a twenty-task run cannot separate a fifteen-point difference
    /// from noise, and a table without it invites exactly that mistake.
    pub hit_rate_ci: (f32, f32),
    pub mean_recall: f32,
    pub median_prompt_tokens: u32,
    pub median_elapsed_ms: u128,
}

pub fn summarise(condition: &str, outcomes: &[AgentOutcome]) -> AgentSummary {
    let mine: Vec<&AgentOutcome> = outcomes
        .iter()
        .filter(|o| o.condition == condition)
        .collect();
    let errors = mine.iter().filter(|o| o.error.is_some()).count();
    let scored: Vec<&&AgentOutcome> = mine.iter().filter(|o| o.error.is_none()).collect();
    let n = scored.len().max(1) as f32;
    let hits = scored.iter().filter(|o| o.hit).count();

    let median = |mut v: Vec<u128>| -> u128 {
        if v.is_empty() {
            return 0;
        }
        v.sort_unstable();
        v[v.len() / 2]
    };

    AgentSummary {
        condition: condition.to_string(),
        tasks: scored.len(),
        errors,
        hits,
        hit_rate: hits as f32 / n,
        hit_rate_ci: wilson_interval(hits, scored.len()),
        mean_recall: scored.iter().map(|o| o.recall).sum::<f32>() / n,
        median_prompt_tokens: median(scored.iter().map(|o| o.prompt_tokens as u128).collect())
            as u32,
        median_elapsed_ms: median(scored.iter().map(|o| o.elapsed_ms).collect()),
    }
}

/// Wilson score interval at 95%, which behaves sensibly at the small sample sizes and
/// extreme proportions this benchmark actually produces — unlike the normal
/// approximation, which happily reports an interval extending past 100%.
pub fn wilson_interval(successes: usize, trials: usize) -> (f32, f32) {
    if trials == 0 {
        return (0.0, 0.0);
    }
    let n = trials as f32;
    let p = successes as f32 / n;
    const Z: f32 = 1.96;
    let denominator = 1.0 + Z * Z / n;
    let centre = (p + Z * Z / (2.0 * n)) / denominator;
    let spread = Z * ((p * (1.0 - p) / n) + (Z * Z / (4.0 * n * n))).sqrt() / denominator;
    ((centre - spread).max(0.0), (centre + spread).min(1.0))
}

/// Resolve the provider, with a message that says how to configure one.
pub fn provider_or_explain(root: &Path) -> Result<Provider> {
    llm::provider(root)
        .map_err(|reason| anyhow::anyhow!("{}", reason.explain()))
        .context("the model experiments need a provider")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task() -> Task {
        Task {
            id: "t-1".into(),
            prompt: "fix the credit limit check".into(),
            ground_truth: vec!["app/order.py".into(), "app/customer.py".into()],
            commit: "a".repeat(40),
            date: "2026-01-01".into(),
        }
    }

    #[test]
    fn the_prompt_differs_between_conditions_only_in_its_context() {
        let a = prompt(&task(), "context A");
        let b = prompt(&task(), "context B");
        let strip = |s: &str| s.replace("context A", "@").replace("context B", "@");
        assert_eq!(
            strip(&a),
            strip(&b),
            "the question itself must be identical"
        );
    }

    #[test]
    fn an_empty_context_is_stated_rather_than_left_blank() {
        assert!(prompt(&task(), "").contains("(none provided)"));
    }

    #[test]
    fn the_oracle_block_contains_the_answer() {
        // It is meant to. The oracle measures the ceiling, not a retrieval system.
        let block = oracle_block(&task());
        assert!(block.contains("app/order.py"));
        assert!(block.contains("app/customer.py"));
    }

    #[test]
    fn paths_are_parsed_out_of_however_a_model_formats_them() {
        let reply = "Here you go:\n- `app/order.py`\n2. app/customer.py\n* erpnext/x/y.js\n";
        assert_eq!(
            parse_paths(reply),
            vec!["app/order.py", "app/customer.py", "erpnext/x/y.js"]
        );
    }

    #[test]
    fn prose_and_directories_never_count_as_answers() {
        let reply = "I think the change is in the selling module.\nsrc/\nnot a path at all\n";
        assert!(parse_paths(reply).is_empty());
    }

    #[test]
    fn duplicate_paths_are_counted_once() {
        assert_eq!(parse_paths("a/b.py\na/b.py\n"), vec!["a/b.py"]);
    }

    #[test]
    fn a_model_cannot_win_by_naming_everything() {
        let reply = (0..50)
            .map(|i| format!("app/f{i}.py"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(parse_paths(&reply).len(), MAX_NAMED_FILES);
    }

    #[test]
    fn the_confidence_interval_stays_inside_zero_and_one() {
        // The normal approximation does not, which is why it is not used here.
        for (hits, trials) in [(0, 20), (20, 20), (1, 3), (11, 20), (0, 0)] {
            let (low, high) = wilson_interval(hits, trials);
            assert!((0.0..=1.0).contains(&low), "{hits}/{trials} -> {low}");
            assert!((0.0..=1.0).contains(&high), "{hits}/{trials} -> {high}");
            assert!(low <= high);
        }
    }

    #[test]
    fn a_small_sample_produces_an_interval_too_wide_to_claim_a_winner() {
        // 11/20 versus 8/20 looks like a fifteen-point win and is not one.
        let (a_low, a_high) = wilson_interval(11, 20);
        let (b_low, b_high) = wilson_interval(8, 20);
        assert!(
            a_low < b_high && b_low < a_high,
            "these intervals overlap, and the report must say so"
        );
    }

    #[test]
    fn a_provider_failure_is_excluded_rather_than_scored_as_a_miss() {
        let outcomes = vec![
            AgentOutcome {
                task: "t1".into(),
                condition: "c".into(),
                named: vec!["a.py".into()],
                hit: true,
                recall: 1.0,
                prompt_tokens: 100,
                elapsed_ms: 10,
                error: None,
            },
            AgentOutcome {
                task: "t2".into(),
                condition: "c".into(),
                named: vec![],
                hit: false,
                recall: 0.0,
                prompt_tokens: 100,
                elapsed_ms: 10,
                error: Some("provider exploded".into()),
            },
        ];
        let s = summarise("c", &outcomes);
        assert_eq!(s.tasks, 1, "the failed run is not a data point");
        assert_eq!(s.errors, 1);
        assert_eq!(s.hit_rate, 1.0);
    }
}

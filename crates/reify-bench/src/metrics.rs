//! Benchmark metrics.
//!
//! One rule governs everything here: a metric that cannot be defined precisely does
//! not get reported. Each field below states what it counts and what it does not.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::agent::wilson_interval;
use crate::conditions::{Answer, Finding, Findings};

/// How one condition performed on one task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Outcome {
    pub task: String,
    pub condition: String,
    /// Ground-truth files the answer names, over all ground-truth files.
    pub recall: f32,
    /// Ground-truth files the answer names, over all files it names.
    pub precision: f32,
    /// 1-based position of the first ground-truth file, if any was found.
    pub first_hit_rank: Option<usize>,
    /// Tokens spent reading the answer and everything before the first hit.
    ///
    /// The metric that matters most: an answer that eventually contains the right file
    /// on page four has not helped.
    pub tokens_to_first_hit: Option<u32>,
    /// Tokens to consume the whole answer and everything it recommends.
    pub tokens_total: u32,
    /// Files the agent is asked to open.
    pub files_inspected: usize,
    /// Files it opens that are in no ground-truth set.
    pub wrong_files: usize,
    pub elapsed_ms: u128,
}

/// Score one answer against a task's ground truth.
pub fn score(task: &str, condition: &str, answer: &Answer, truth: &[String]) -> Outcome {
    let truth_set: BTreeSet<&str> = truth.iter().map(String::as_str).collect();
    let found: BTreeSet<&str> = answer
        .files
        .iter()
        .map(String::as_str)
        .filter(|f| truth_set.contains(f))
        .collect();

    let mut first_hit_rank = None;
    let mut tokens_to_first_hit = None;
    let mut running = answer.answer_tokens;
    for (i, file) in answer.files.iter().enumerate() {
        running += answer.read_tokens.get(i).copied().unwrap_or(0);
        if truth_set.contains(file.as_str()) {
            first_hit_rank = Some(i + 1);
            tokens_to_first_hit = Some(running);
            break;
        }
    }

    Outcome {
        task: task.to_string(),
        condition: condition.to_string(),
        recall: found.len() as f32 / truth_set.len().max(1) as f32,
        precision: if answer.files.is_empty() {
            0.0
        } else {
            found.len() as f32 / answer.files.len() as f32
        },
        first_hit_rank,
        tokens_to_first_hit,
        tokens_total: answer.total_tokens(),
        files_inspected: answer.files.len(),
        wrong_files: answer.files.len() - found.len(),
        elapsed_ms: answer.elapsed_ms,
    }
}

/// Aggregate figures for one condition across the whole task set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub condition: String,
    pub tasks: usize,
    /// Tasks where at least one ground-truth file was named.
    pub tasks_with_a_hit: usize,
    pub hit_rate: f32,
    pub mean_recall: f32,
    pub mean_precision: f32,
    /// Mean reciprocal rank of the first ground-truth file. Higher is better.
    pub mrr: f32,
    /// Median over tasks that found anything. Reported as a median because a handful
    /// of very expensive tasks otherwise dominate a mean.
    pub median_tokens_to_first_hit: Option<u32>,
    pub median_tokens_total: u32,
    pub median_files_inspected: usize,
    pub median_elapsed_ms: u128,
}

pub fn summarise(condition: &str, outcomes: &[Outcome]) -> Summary {
    let mine: Vec<&Outcome> = outcomes
        .iter()
        .filter(|o| o.condition == condition)
        .collect();
    let n = mine.len().max(1) as f32;
    let hits: Vec<&&Outcome> = mine.iter().filter(|o| o.first_hit_rank.is_some()).collect();

    Summary {
        condition: condition.to_string(),
        tasks: mine.len(),
        tasks_with_a_hit: hits.len(),
        hit_rate: hits.len() as f32 / n,
        mean_recall: mine.iter().map(|o| o.recall).sum::<f32>() / n,
        mean_precision: mine.iter().map(|o| o.precision).sum::<f32>() / n,
        mrr: mine
            .iter()
            .map(|o| o.first_hit_rank.map_or(0.0, |r| 1.0 / r as f32))
            .sum::<f32>()
            / n,
        median_tokens_to_first_hit: median(
            hits.iter().filter_map(|o| o.tokens_to_first_hit).collect(),
        ),
        median_tokens_total: median(mine.iter().map(|o| o.tokens_total).collect()).unwrap_or(0),
        median_files_inspected: median(mine.iter().map(|o| o.files_inspected).collect())
            .unwrap_or(0),
        median_elapsed_ms: median(mine.iter().map(|o| o.elapsed_ms).collect()).unwrap_or(0),
    }
}

/// Compare two conditions only on the tasks where **both** found something.
///
/// Comparing medians across conditions that hit different tasks is a trap: a tool that
/// only ever succeeds on easy tasks posts a flattering median precisely because it
/// fails everywhere else. The paired comparison removes that, and the miss-penalised
/// expectation below covers the tasks it excludes.
#[derive(Debug, Clone, Serialize)]
pub struct Paired {
    pub a: String,
    pub b: String,
    /// Tasks where both conditions found at least one changed file.
    pub common_tasks: usize,
    pub a_median_tokens: Option<u32>,
    pub b_median_tokens: Option<u32>,
    /// Tasks in the common set where `a` reached the first correct file for fewer tokens.
    pub a_cheaper_on: usize,
    pub b_cheaper_on: usize,
}

pub fn pair(a: &str, b: &str, outcomes: &[Outcome]) -> Paired {
    let pick = |condition: &str| -> std::collections::HashMap<&str, &Outcome> {
        outcomes
            .iter()
            .filter(|o| o.condition == condition)
            .map(|o| (o.task.as_str(), o))
            .collect()
    };
    let (left, right) = (pick(a), pick(b));

    let mut a_tokens = Vec::new();
    let mut b_tokens = Vec::new();
    let (mut a_cheaper, mut b_cheaper) = (0usize, 0usize);
    for (task, l) in &left {
        let Some(r) = right.get(task) else { continue };
        let (Some(lt), Some(rt)) = (l.tokens_to_first_hit, r.tokens_to_first_hit) else {
            continue;
        };
        a_tokens.push(lt);
        b_tokens.push(rt);
        match lt.cmp(&rt) {
            std::cmp::Ordering::Less => a_cheaper += 1,
            std::cmp::Ordering::Greater => b_cheaper += 1,
            std::cmp::Ordering::Equal => {}
        }
    }

    Paired {
        a: a.to_string(),
        b: b.to_string(),
        common_tasks: a_tokens.len(),
        a_median_tokens: median(a_tokens),
        b_median_tokens: median(b_tokens),
        a_cheaper_on: a_cheaper,
        b_cheaper_on: b_cheaper,
    }
}

/// Expected tokens to reach a changed file, charging a miss the full budget.
///
/// The single number that combines "did it find anything" with "how much did that
/// cost": a condition that misses has spent the whole budget and found nothing, which
/// is exactly what the agent experiences.
pub fn expected_tokens(condition: &str, outcomes: &[Outcome], budget: u32) -> f32 {
    let mine: Vec<&Outcome> = outcomes
        .iter()
        .filter(|o| o.condition == condition)
        .collect();
    if mine.is_empty() {
        return 0.0;
    }
    let total: f32 = mine
        .iter()
        .map(|o| o.tokens_to_first_hit.unwrap_or(budget) as f32)
        .sum();
    total / mine.len() as f32
}

// ---- the held-out-hunk metrics ----------------------------------------------

/// Pre-registered falsification condition for `reify verify`, stated before the first
/// run of this harness and not moved since.
///
/// > If `omission_recall` on this substrate is below **0.25**, or `false_alarm_rate`
/// > is above **0.1 per commit**, the `reify verify` feature does not get built on
/// > this substrate.
///
/// Both halves matter. Recall alone would be cleared by a checker that reports every
/// caller of everything; the false-alarm ceiling is what stops that, and it is
/// measured against complete merged commits, where a finding cannot be anything but
/// wrong. A result that fails either half is a result, not a failure of the harness:
/// the response is to publish it and not build the feature, never to widen the query
/// until it passes.
pub const VERIFY_RECALL_FLOOR: f32 = 0.25;
/// Findings per complete merged commit, above which the checker is too noisy to ship.
pub const VERIFY_FALSE_ALARM_CEILING: f32 = 0.1;

/// One held-out-hunk trial.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyOutcome {
    pub task: String,
    pub commit: String,
    /// The file whose only hunk was withheld.
    pub omission_file: String,
    /// `path:line` of the symbol the withheld hunk falls inside, when it falls inside
    /// one. `None` means the change was outside every indexed symbol — an import, a
    /// constant, a top-level statement — and the trial is not scorable at symbol
    /// granularity. It is excluded there rather than counted as a miss.
    pub omission_symbol: Option<String>,
    /// Some symbol in the omission's file calls a symbol in another file, at the
    /// parent commit. False means no caller-based checker could ever cite this file,
    /// whatever query it runs.
    pub omission_file_reachable: bool,
    /// A finding cites the omission's file.
    pub file_hit: bool,
    /// A finding cites the omission's file on the truncated diff and **not** on the
    /// complete one. A citation the negative control also produces was not caused by
    /// the omission, whatever it looks like next to it.
    pub file_hit_attributable: bool,
    /// A finding cites the omission's symbol. `None` when not scorable.
    pub symbol_hit: Option<bool>,
    pub findings: usize,
    /// Findings against the **complete** commit. Complete by construction, so every
    /// one of these is a false positive.
    pub false_alarms: usize,
    /// Symbols the truncated diff resolved to. Zero means the checker had nothing to
    /// work from, which is a different failure from having something and missing.
    pub changed_symbols: usize,
    pub verify_tokens: u32,
    pub verify_latency_ms: u128,
    /// Wall clock to extract and index the parent tree. The real feature would run
    /// against an index that already exists, so this is the harness's cost, not the
    /// checker's, and is kept out of `verify_latency_ms`.
    pub index_ms: u128,
    /// Every location the truncated run cited, and every location the complete run
    /// cited. Written out because a rate nobody can check is a claim, not a
    /// measurement: these are what `false_alarms` counts, one line each.
    pub cited: Vec<String>,
    pub cited_on_complete: Vec<String>,
}

/// Aggregate figures over a set of held-out-hunk trials.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifySummary {
    pub tasks: usize,
    /// Share of truncated diffs where some finding cites the omitted hunk's file.
    pub omission_recall: f32,
    pub omission_recall_ci: (f32, f32),
    /// The same share, counting only citations the complete commit does **not** also
    /// produce. `omission_recall` is the metric as specified; this is the one that
    /// says whether the checker responded to the omission or to the file's standing
    /// noise. Where the two differ, the gap is the part of the headline that the
    /// negative control already explains.
    pub omission_recall_attributable: f32,
    pub omission_recall_attributable_ci: (f32, f32),
    /// Trials whose omitted file calls out of itself at all. This is the ceiling:
    /// `omission_recall` cannot exceed `reachable_omissions / tasks` however the query
    /// is written, so the two numbers separate a query problem from a substrate
    /// problem.
    pub reachable_omissions: usize,
    /// `omission_recall` restricted to those trials — what the checker managed where
    /// there was something to find.
    pub omission_recall_reachable: Option<f32>,
    pub omission_recall_reachable_ci: Option<(f32, f32)>,
    /// Trials where the omission falls inside an indexed symbol — the denominator of
    /// the symbol-granular figure.
    pub symbol_scorable: usize,
    /// The same share at symbol granularity, over `symbol_scorable` trials.
    pub omission_recall_symbol: Option<f32>,
    pub omission_recall_symbol_ci: Option<(f32, f32)>,
    /// Findings per complete merged commit. A rate over counts, not a proportion, so
    /// it carries no Wilson interval; the proportion beside it does.
    pub false_alarm_rate: f32,
    /// Complete commits producing at least one finding.
    pub commits_with_a_false_alarm: usize,
    pub false_alarm_share_ci: (f32, f32),
    /// Median findings per truncated diff. A checker emitting thirty findings is
    /// unusable at any precision, which a mean would hide behind the quiet cases.
    pub median_findings_per_diff: usize,
    /// Median tokens the findings output would cost the agent that reads it.
    pub median_verify_tokens: u32,
    /// Median wall clock of the query alone.
    pub median_verify_latency_ms: u128,
    /// Median wall clock to extract and index one parent tree, reported so the cost of
    /// running this in CI is a measured number rather than a promise.
    pub median_index_ms: u128,
    /// Trials where the truncated diff resolved to no symbol at all. The checker
    /// cannot report anything for these; they stay in the denominator, because a
    /// substrate that cannot resolve a diff has failed the task.
    pub diffs_resolving_to_nothing: usize,
}

/// Does this substrate clear the pre-registered bar?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    Build,
    DoNotBuild,
}

impl VerifySummary {
    pub fn verdict(&self) -> Verdict {
        if self.omission_recall < VERIFY_RECALL_FLOOR
            || self.false_alarm_rate > VERIFY_FALSE_ALARM_CEILING
        {
            Verdict::DoNotBuild
        } else {
            Verdict::Build
        }
    }

    /// Why the verdict came out the way it did, in one line.
    pub fn why(&self) -> String {
        let mut failed = Vec::new();
        if self.omission_recall < VERIFY_RECALL_FLOOR {
            failed.push(format!(
                "omission_recall {:.2} < {VERIFY_RECALL_FLOOR:.2}",
                self.omission_recall
            ));
        }
        if self.false_alarm_rate > VERIFY_FALSE_ALARM_CEILING {
            failed.push(format!(
                "false_alarm_rate {:.2} > {VERIFY_FALSE_ALARM_CEILING:.2}",
                self.false_alarm_rate
            ));
        }
        if failed.is_empty() {
            format!(
                "omission_recall {:.2} >= {VERIFY_RECALL_FLOOR:.2} and false_alarm_rate \
                 {:.2} <= {VERIFY_FALSE_ALARM_CEILING:.2}",
                self.omission_recall, self.false_alarm_rate
            )
        } else {
            failed.join("; ")
        }
    }
}

/// Score one held-out-hunk trial.
pub fn score_verify(
    task: &crate::tasks::TruncatedTask,
    omission_symbol: Option<String>,
    omission_file_reachable: bool,
    truncated: &Findings,
    complete: &Findings,
    index_ms: u128,
) -> VerifyOutcome {
    let cites = |predicate: &dyn Fn(&Finding) -> bool| truncated.findings.iter().any(predicate);
    VerifyOutcome {
        task: task.id.clone(),
        commit: task.commit.clone(),
        omission_file: task.omission_file.clone(),
        omission_file_reachable,
        file_hit: cites(&|f| f.path == task.omission_file),
        file_hit_attributable: cites(&|f| f.path == task.omission_file)
            && !complete
                .findings
                .iter()
                .any(|f| f.path == task.omission_file),
        symbol_hit: omission_symbol
            .as_ref()
            .map(|symbol| cites(&|f| &f.location == symbol)),
        omission_symbol,
        findings: truncated.findings.len(),
        false_alarms: complete.findings.len(),
        changed_symbols: truncated.changed_symbols.len(),
        verify_tokens: truncated.answer_tokens,
        verify_latency_ms: truncated.elapsed_ms,
        index_ms,
        cited: truncated
            .findings
            .iter()
            .map(|f| f.location.clone())
            .collect(),
        cited_on_complete: complete
            .findings
            .iter()
            .map(|f| f.location.clone())
            .collect(),
    }
}

pub fn summarise_verify(outcomes: &[VerifyOutcome]) -> VerifySummary {
    let n = outcomes.len();
    let denominator = n.max(1) as f32;
    let file_hits = outcomes.iter().filter(|o| o.file_hit).count();
    let attributable = outcomes.iter().filter(|o| o.file_hit_attributable).count();
    let reachable: Vec<&VerifyOutcome> = outcomes
        .iter()
        .filter(|o| o.omission_file_reachable)
        .collect();
    let reachable_hits = reachable.iter().filter(|o| o.file_hit).count();
    let scorable: Vec<&VerifyOutcome> = outcomes
        .iter()
        .filter(|o| o.omission_symbol.is_some())
        .collect();
    let symbol_hits = scorable
        .iter()
        .filter(|o| o.symbol_hit == Some(true))
        .count();
    let noisy = outcomes.iter().filter(|o| o.false_alarms > 0).count();

    VerifySummary {
        tasks: n,
        omission_recall: file_hits as f32 / denominator,
        omission_recall_ci: wilson_interval(file_hits, n),
        omission_recall_attributable: attributable as f32 / denominator,
        omission_recall_attributable_ci: wilson_interval(attributable, n),
        reachable_omissions: reachable.len(),
        omission_recall_reachable: (!reachable.is_empty())
            .then(|| reachable_hits as f32 / reachable.len() as f32),
        omission_recall_reachable_ci: (!reachable.is_empty())
            .then(|| wilson_interval(reachable_hits, reachable.len())),
        symbol_scorable: scorable.len(),
        omission_recall_symbol: (!scorable.is_empty())
            .then(|| symbol_hits as f32 / scorable.len() as f32),
        omission_recall_symbol_ci: (!scorable.is_empty())
            .then(|| wilson_interval(symbol_hits, scorable.len())),
        false_alarm_rate: outcomes.iter().map(|o| o.false_alarms).sum::<usize>() as f32
            / denominator,
        commits_with_a_false_alarm: noisy,
        false_alarm_share_ci: wilson_interval(noisy, n),
        median_findings_per_diff: median(outcomes.iter().map(|o| o.findings).collect())
            .unwrap_or(0),
        median_verify_tokens: median(outcomes.iter().map(|o| o.verify_tokens).collect())
            .unwrap_or(0),
        median_verify_latency_ms: median(outcomes.iter().map(|o| o.verify_latency_ms).collect())
            .unwrap_or(0),
        median_index_ms: median(outcomes.iter().map(|o| o.index_ms).collect()).unwrap_or(0),
        diffs_resolving_to_nothing: outcomes.iter().filter(|o| o.changed_symbols == 0).count(),
    }
}

fn median<T: Ord + Copy>(mut values: Vec<T>) -> Option<T> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    Some(values[values.len() / 2])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(files: &[&str], read: &[u32], answer_tokens: u32) -> Answer {
        Answer {
            files: files.iter().map(|s| s.to_string()).collect(),
            answer_tokens,
            read_tokens: read.to_vec(),
            elapsed_ms: 1,
        }
    }

    fn trial(omission_file: &str) -> crate::tasks::TruncatedTask {
        crate::tasks::TruncatedTask {
            id: "v-1".into(),
            commit: "a".repeat(40),
            parent: "b".repeat(40),
            date: "2026-01-01".into(),
            prompt: "fix the credit limit check".into(),
            complete: crate::tasks::Patch::default(),
            truncated: crate::tasks::Patch::default(),
            omission_file: omission_file.into(),
            omission_line: 10,
        }
    }

    fn findings(locations: &[&str]) -> Findings {
        Findings {
            findings: locations
                .iter()
                .map(|location| Finding {
                    location: (*location).into(),
                    path: location
                        .rsplit_once(':')
                        .map_or(*location, |(p, _)| p)
                        .into(),
                    what: "f".into(),
                    reason: "calls g".into(),
                })
                .collect(),
            changed_symbols: vec!["app/x.py:1".into()],
            answer_tokens: 40,
            elapsed_ms: 1,
        }
    }

    #[test]
    fn a_citation_the_complete_commit_also_produces_is_a_hit_but_not_attributable() {
        // The negative control's whole purpose: the checker cited that file whether or
        // not anything was withheld, so the omission did not cause the citation.
        let outcome = score_verify(
            &trial("app/orders.py"),
            None,
            true,
            &findings(&["app/orders.py:5"]),
            &findings(&["app/orders.py:5"]),
            100,
        );
        assert!(outcome.file_hit);
        assert!(!outcome.file_hit_attributable);
        assert_eq!(outcome.false_alarms, 1);
    }

    #[test]
    fn a_citation_only_the_truncated_diff_produces_is_attributable() {
        let outcome = score_verify(
            &trial("app/orders.py"),
            Some("app/orders.py:5".into()),
            true,
            &findings(&["app/orders.py:5"]),
            &findings(&[]),
            100,
        );
        assert!(outcome.file_hit_attributable);
        assert_eq!(outcome.symbol_hit, Some(true));
        assert_eq!(outcome.false_alarms, 0);
    }

    #[test]
    fn an_omission_inside_no_symbol_is_unscorable_rather_than_a_miss() {
        let outcome = score_verify(
            &trial("app/orders.py"),
            None,
            false,
            &findings(&[]),
            &findings(&[]),
            100,
        );
        assert_eq!(
            outcome.symbol_hit, None,
            "counting it as a miss would be a lie"
        );
        let summary = summarise_verify(&[outcome]);
        assert_eq!(summary.symbol_scorable, 0);
        assert_eq!(summary.omission_recall_symbol, None);
        assert_eq!(summary.reachable_omissions, 0);
        assert_eq!(summary.omission_recall_reachable, None);
    }

    #[test]
    fn the_pre_registered_condition_fails_on_either_half_alone() {
        let quiet_but_blind = VerifySummary {
            omission_recall: 0.10,
            false_alarm_rate: 0.0,
            ..summarise_verify(&[])
        };
        assert_eq!(quiet_but_blind.verdict(), Verdict::DoNotBuild);
        assert!(quiet_but_blind.why().contains("omission_recall"));

        let sharp_but_noisy = VerifySummary {
            omission_recall: 0.90,
            false_alarm_rate: 3.0,
            ..summarise_verify(&[])
        };
        assert_eq!(sharp_but_noisy.verdict(), Verdict::DoNotBuild);
        assert!(sharp_but_noisy.why().contains("false_alarm_rate"));

        let good = VerifySummary {
            omission_recall: 0.40,
            false_alarm_rate: 0.05,
            ..summarise_verify(&[])
        };
        assert_eq!(good.verdict(), Verdict::Build);
    }

    #[test]
    fn the_false_alarm_rate_counts_findings_not_commits() {
        // A checker that shouts thirty times at one commit and stays silent at nine
        // others is not a checker with a 10% false-alarm problem.
        let outcomes: Vec<VerifyOutcome> = (0..10)
            .map(|i| {
                score_verify(
                    &trial("app/orders.py"),
                    None,
                    true,
                    &findings(&[]),
                    &findings(&if i == 0 { vec!["a.py:1"; 30] } else { vec![] }),
                    1,
                )
            })
            .collect();
        let summary = summarise_verify(&outcomes);
        assert_eq!(summary.commits_with_a_false_alarm, 1);
        assert!((summary.false_alarm_rate - 3.0).abs() < 1e-6);
        assert_eq!(summary.verdict(), Verdict::DoNotBuild);
    }

    #[test]
    fn a_perfect_answer_scores_perfectly() {
        let truth = vec!["a.py".to_string()];
        let out = score("t", "c", &answer(&["a.py"], &[100], 0), &truth);
        assert_eq!(out.recall, 1.0);
        assert_eq!(out.precision, 1.0);
        assert_eq!(out.first_hit_rank, Some(1));
        assert_eq!(out.tokens_to_first_hit, Some(100));
        assert_eq!(out.wrong_files, 0);
    }

    #[test]
    fn tokens_to_first_hit_charges_everything_read_before_it() {
        // An answer that eventually contains the right file on page four has not helped.
        let truth = vec!["c.py".to_string()];
        let out = score(
            "t",
            "c",
            &answer(&["a.py", "b.py", "c.py"], &[500, 500, 100], 0),
            &truth,
        );
        assert_eq!(out.first_hit_rank, Some(3));
        assert_eq!(out.tokens_to_first_hit, Some(1100));
    }

    #[test]
    fn the_cost_of_the_answer_itself_is_charged_too() {
        let truth = vec!["a.py".to_string()];
        let out = score("t", "c", &answer(&["a.py"], &[100], 1_400), &truth);
        assert_eq!(out.tokens_to_first_hit, Some(1_500), "the tool is not free");
    }

    #[test]
    fn a_miss_reports_no_rank_rather_than_a_default() {
        let truth = vec!["z.py".to_string()];
        let out = score("t", "c", &answer(&["a.py"], &[100], 0), &truth);
        assert_eq!(out.first_hit_rank, None);
        assert_eq!(out.tokens_to_first_hit, None);
        assert_eq!(out.recall, 0.0);
        assert_eq!(out.wrong_files, 1);
    }

    #[test]
    fn an_empty_answer_scores_zero_precision_not_a_division_by_zero() {
        let truth = vec!["a.py".to_string()];
        let out = score("t", "c", &answer(&[], &[], 0), &truth);
        assert_eq!(out.precision, 0.0);
        assert_eq!(out.recall, 0.0);
    }

    #[test]
    fn summaries_average_only_over_their_own_condition() {
        let outcomes = vec![
            score("t1", "a", &answer(&["x.py"], &[10], 0), &["x.py".into()]),
            score("t2", "a", &answer(&["y.py"], &[10], 0), &["z.py".into()]),
            score("t1", "b", &answer(&["q.py"], &[10], 0), &["x.py".into()]),
        ];
        let a = summarise("a", &outcomes);
        assert_eq!(a.tasks, 2);
        assert_eq!(a.tasks_with_a_hit, 1);
        assert!((a.hit_rate - 0.5).abs() < 1e-6);
        assert!((a.mrr - 0.5).abs() < 1e-6);
        let b = summarise("b", &outcomes);
        assert_eq!(b.tasks, 1);
        assert_eq!(b.tasks_with_a_hit, 0);
    }

    #[test]
    fn a_paired_comparison_only_counts_tasks_both_conditions_hit() {
        let outcomes = vec![
            score("t1", "a", &answer(&["x.py"], &[100], 0), &["x.py".into()]),
            score("t2", "a", &answer(&["y.py"], &[100], 0), &["y.py".into()]),
            score("t1", "b", &answer(&["x.py"], &[500], 0), &["x.py".into()]),
            score("t2", "b", &answer(&["q.py"], &[100], 0), &["y.py".into()]),
        ];
        let p = pair("a", "b", &outcomes);
        assert_eq!(p.common_tasks, 1, "t2 is excluded because b missed it");
        assert_eq!(p.a_cheaper_on, 1);
        assert_eq!(p.b_cheaper_on, 0);
    }

    #[test]
    fn a_miss_is_charged_the_whole_budget() {
        // Otherwise a condition improves its average by failing more often.
        let outcomes = vec![
            score("t1", "a", &answer(&["x.py"], &[100], 0), &["x.py".into()]),
            score("t2", "a", &answer(&["q.py"], &[100], 0), &["z.py".into()]),
        ];
        assert!((expected_tokens("a", &outcomes, 4_000) - 2_050.0).abs() < 1e-3);
    }

    #[test]
    fn medians_ignore_tasks_that_found_nothing() {
        let outcomes = vec![
            score("t1", "a", &answer(&["x.py"], &[100], 0), &["x.py".into()]),
            score("t2", "a", &answer(&["y.py"], &[9999], 0), &["z.py".into()]),
        ];
        let s = summarise("a", &outcomes);
        assert_eq!(s.median_tokens_to_first_hit, Some(100));
    }
}

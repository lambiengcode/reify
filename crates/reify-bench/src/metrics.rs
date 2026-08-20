//! Benchmark metrics.
//!
//! One rule governs everything here: a metric that cannot be defined precisely does
//! not get reported. Each field below states what it counts and what it does not.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::conditions::Answer;

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
#[derive(Debug, Clone, Serialize)]
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

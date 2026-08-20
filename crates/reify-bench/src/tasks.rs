//! Benchmark task construction.
//!
//! Tasks are derived from real merged commits rather than invented, because a
//! hand-written task set measures the taste of whoever wrote it. For each commit the
//! prompt is the developer's own description of the change and the ground truth is the
//! set of files they actually touched.
//!
//! What this measures, stated plainly: **retrieval** — whether a tool puts the files
//! that had to change in front of the agent, and at what token cost. It does not
//! measure whether an agent then makes the change correctly. See `LIMITATIONS` in the
//! generated report.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use reify::gitlog::{self, ChangeClass};

/// One benchmark task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    /// The developer's own description of the change, used verbatim as the prompt.
    pub prompt: String,
    /// Files the change actually touched, relative to the repository root.
    pub ground_truth: Vec<String>,
    /// The commit the task was derived from, so a result can be traced back.
    pub commit: String,
    pub date: String,
}

/// A frozen task set, pinned to the exact repository state it was built from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSet {
    pub repository: String,
    pub head: String,
    pub generated_from_commits: usize,
    /// The commit an index should be built at, when the set was generated with
    /// `--after`. Indexing here means the changes being asked for are **not** already
    /// in the code, which removes the benchmark's largest caveat.
    #[serde(default)]
    pub base: Option<String>,
    pub tasks: Vec<Task>,
}

/// Upper bound on files a task may touch.
///
/// A commit touching twenty files is a refactor or a rename; its "ground truth" would
/// be trivially wide and would flatter every condition equally.
const MAX_GROUND_TRUTH: usize = 6;
const MIN_GROUND_TRUTH: usize = 1;
/// Prompts shorter than this carry no retrievable signal ("fix: typo").
const MIN_PROMPT_WORDS: usize = 5;

/// Words that mark a commit as mechanical rather than behavioural.
const MECHANICAL: &[&str] = &[
    "bump",
    "version",
    "translation",
    "typo",
    "lint",
    "format",
    "changelog",
    "merge",
    "revert",
    "pre-commit",
    "ci:",
    "chore",
    "whitespace",
    "rename",
];

/// Build a task set from a repository's recent history.
///
/// `wanted` tasks are selected from the newest `scan` commits, taking every commit
/// that passes the filters in order. Selection is deterministic and happens before any
/// condition is run, so there is no opportunity to choose tasks that flatter a result.
pub fn generate(root: &Path, wanted: usize, scan: usize, after: Option<&str>) -> Result<TaskSet> {
    let head = gitlog::head_sha(root).context("reading HEAD")?;
    let history = gitlog::history(root, scan)?;

    // When a base is given, the walk stops there: every task must describe a change
    // made *after* the state the index will be built at.
    let base = after.map(|rev| resolve(root, rev)).transpose()?;
    let cutoff = base.as_ref().and_then(|sha| {
        history
            .commits
            .iter()
            .position(|c| c.sha.starts_with(sha) || sha.starts_with(&c.sha))
    });

    let mut tasks = Vec::new();
    for (i, commit) in history.commits.iter().enumerate() {
        if tasks.len() >= wanted {
            break;
        }
        if cutoff.is_some_and(|stop| i >= stop) {
            break;
        }
        let Some(mut task) = candidate(root, commit) else {
            continue;
        };
        // A file created by the change cannot be retrieved from a base that predates
        // it. Keeping it as ground truth would score every condition zero and measure
        // nothing, so the task is narrowed to the files that already existed.
        if let Some(base) = &base {
            task.ground_truth.retain(|path| exists_at(root, base, path));
            if task.ground_truth.is_empty() {
                continue;
            }
        }
        tasks.push(task);
    }

    Ok(TaskSet {
        repository: root.display().to_string(),
        head,
        generated_from_commits: history.commits.len(),
        base,
        tasks,
    })
}

/// Resolve a revision to a full sha.
fn resolve(root: &Path, rev: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", rev])
        .current_dir(root)
        .output()
        .context("running git rev-parse")?;
    anyhow::ensure!(
        output.status.success(),
        "cannot resolve `{rev}` in {}",
        root.display()
    );
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Did `path` exist at `commit`?
fn exists_at(root: &Path, commit: &str, path: &str) -> bool {
    Command::new("git")
        .args(["cat-file", "-e", &format!("{commit}:{path}")])
        .current_dir(root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn candidate(root: &Path, commit: &gitlog::Commit) -> Option<Task> {
    if !matches!(commit.class, ChangeClass::Fix | ChangeClass::Feature) {
        return None;
    }
    let prompt = clean_subject(&commit.subject);
    if prompt.split_whitespace().count() < MIN_PROMPT_WORDS {
        return None;
    }
    let lowered = prompt.to_lowercase();
    if MECHANICAL.iter().any(|w| lowered.contains(w)) {
        return None;
    }

    // Only files that still exist can be found by any condition; a task whose ground
    // truth has since been deleted would score zero for everyone and measure nothing.
    let mut truth: BTreeSet<String> = BTreeSet::new();
    for file in &commit.files {
        if !root.join(file).is_file() {
            continue;
        }
        // Test files are usually changed alongside; they make the target too easy.
        if file.contains("/test_") || file.ends_with("_test.py") {
            continue;
        }
        if file.ends_with(".py") || file.ends_with(".js") || file.ends_with(".ts") {
            truth.insert(file.clone());
        }
    }
    if !(MIN_GROUND_TRUTH..=MAX_GROUND_TRUTH).contains(&truth.len()) {
        return None;
    }

    Some(Task {
        id: format!("t-{}", &commit.sha[..8]),
        prompt,
        ground_truth: truth.into_iter().collect(),
        commit: commit.sha.clone(),
        date: commit.date(),
    })
}

/// Strip conventional-commit noise and PR numbers, leaving the human description.
///
/// The PR number must go: it appears in no source file, so leaving it in would hand
/// every condition a token that cannot possibly help, and lexical conditions a term
/// that cannot possibly match.
fn clean_subject(subject: &str) -> String {
    let without_prefix = subject
        .split_once(':')
        .map(|(head, rest)| {
            let head = head.trim().to_lowercase();
            let is_prefix = head
                .trim_end_matches(|c: char| c == ')' || c.is_alphanumeric() || c == '_' || c == '(')
                .is_empty()
                && head.len() < 24;
            if is_prefix {
                rest
            } else {
                subject
            }
        })
        .unwrap_or(subject);

    let mut out = String::with_capacity(without_prefix.len());
    let mut depth = 0usize;
    for ch in without_prefix.chars() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conventional_prefixes_and_pr_numbers_are_stripped() {
        assert_eq!(
            clean_subject("fix(selling): correct credit limit check (#58316)"),
            "correct credit limit check"
        );
        assert_eq!(
            clean_subject("feat: add discount tier for strategic accounts"),
            "add discount tier for strategic accounts"
        );
    }

    #[test]
    fn a_subject_with_a_colon_that_is_not_a_prefix_is_left_alone() {
        assert_eq!(
            clean_subject("Credit limit: block orders above the cap"),
            "Credit limit: block orders above the cap"
        );
    }

    #[test]
    fn mechanical_commits_are_not_tasks() {
        let commit = |subject: &str| gitlog::Commit {
            sha: "a".repeat(40),
            timestamp: 0,
            author: "x".into(),
            subject: subject.into(),
            class: ChangeClass::Fix,
            files: vec!["a.py".into()],
        };
        let root = Path::new("/nonexistent");
        assert!(candidate(root, &commit("chore: bump version to 15.2")).is_none());
        assert!(candidate(root, &commit("fix: typo")).is_none());
        assert!(candidate(root, &commit("fix: update translation strings")).is_none());
    }

    #[test]
    fn a_commit_touching_too_many_files_is_not_a_task() {
        let files: Vec<String> = (0..30).map(|i| format!("f{i}.py")).collect();
        let commit = gitlog::Commit {
            sha: "a".repeat(40),
            timestamp: 0,
            author: "x".into(),
            subject: "fix: correct the credit limit calculation everywhere".into(),
            class: ChangeClass::Fix,
            files,
        };
        assert!(candidate(Path::new("/nonexistent"), &commit).is_none());
    }
}

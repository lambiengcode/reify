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

impl TaskSet {
    /// The repository's name, for a prompt that has to say what is being worked on.
    ///
    /// `repository` is a path — `.bench/medusa` — because that is what the generator
    /// was pointed at. The final component is the name a developer would use, and a
    /// prompt that names the wrong repository is a validity defect rather than a
    /// cosmetic one: see the dated note at the top of `benchmarks/REPORT-medusa.md`.
    pub fn repository_name(&self) -> &str {
        let trimmed = self.repository.trim_end_matches(['/', '\\']);
        trimmed
            .rsplit(['/', '\\'])
            .find(|part| !part.is_empty())
            .unwrap_or(trimmed)
    }
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
pub fn generate(
    root: &Path,
    wanted: usize,
    scan: usize,
    after: Option<&str>,
    until: Option<&str>,
    exclude: &BTreeSet<String>,
) -> Result<TaskSet> {
    let head = gitlog::head_sha(root).context("reading HEAD")?;
    let history = gitlog::history(root, scan)?;

    // When a base is given, the walk stops there: every task must describe a change
    // made *after* the state the index will be built at. When `until` is given, the
    // walk *starts below it*: only strictly older commits qualify, which is how a
    // training corpus is kept disjoint from every evaluation window.
    let base = after.map(|rev| resolve(root, rev)).transpose()?;
    let (start, cutoff) = window(root, &history, &head, after, until)?;

    let tasks = take(&history, start, cutoff, exclude, wanted, |commit| {
        let mut task = candidate(root, commit)?;
        // A file created by the change cannot be retrieved from a base that predates
        // it. Keeping it as ground truth would score every condition zero and measure
        // nothing, so the task is narrowed to the files that already existed.
        if let Some(base) = &base {
            task.ground_truth.retain(|path| exists_at(root, base, path));
            if task.ground_truth.is_empty() {
                return None;
            }
        }
        Some(task)
    });

    Ok(TaskSet {
        repository: root.display().to_string(),
        head,
        generated_from_commits: history.commits.len(),
        base,
        tasks,
    })
}

/// The selection loop both generators share: newest first, stopping at the `--after`
/// cutoff, starting below the `--until` boundary, skipping excluded commits, taking
/// the first `wanted` for which `pick` yields something.
///
/// Shared rather than copied so a filter can never apply to one generator and not the
/// other — which is the way two task sets silently stop being comparable.
fn take<T>(
    history: &gitlog::History,
    start: usize,
    cutoff: Option<usize>,
    exclude: &BTreeSet<String>,
    wanted: usize,
    mut pick: impl FnMut(&gitlog::Commit) -> Option<T>,
) -> Vec<T> {
    let mut taken = Vec::new();
    for (i, commit) in history.commits.iter().enumerate().skip(start) {
        if taken.len() >= wanted {
            break;
        }
        if cutoff.is_some_and(|stop| i >= stop) {
            break;
        }
        if exclude.contains(&commit.sha) {
            continue;
        }
        if let Some(item) = pick(commit) {
            taken.push(item);
        }
    }
    taken
}

/// The slice of history a generator may draw from: `(start, cutoff)` as indices into
/// the merge-free scan, newest first.
///
/// `--after` sets the cutoff — the walk stops there, so every task describes a change
/// made after the state an index will be built at. `--until` sets the start — the
/// walk begins strictly *below* it, which is how a training corpus stays disjoint
/// from every evaluation window.
fn window(
    root: &Path,
    history: &gitlog::History,
    head: &str,
    after: Option<&str>,
    until: Option<&str>,
) -> Result<(usize, Option<usize>)> {
    let position = |sha: &str| {
        history
            .commits
            .iter()
            .position(|c| c.sha.starts_with(sha) || sha.starts_with(&c.sha))
    };
    let cutoff = after
        .map(|rev| resolve(root, rev))
        .transpose()?
        .and_then(|sha| position(&sha));
    let start = match until.map(|rev| resolve(root, rev)).transpose()? {
        None => 0,
        // The scanned list is merge-free, so a merge commit named as the boundary is
        // legitimately absent from it. When the boundary is HEAD itself, "strictly
        // older than HEAD" excludes nothing the list contains.
        Some(sha) if sha == head => 0,
        Some(sha) => match position(&sha) {
            Some(position) => position + 1,
            // A merge commit is legitimately absent from the merge-free list, so the
            // boundary falls back to its timestamp: strictly-older-than holds for
            // every commit authored before it.
            None => {
                let at = commit_time(root, &sha)?;
                history
                    .commits
                    .iter()
                    .position(|c| c.timestamp < at)
                    .with_context(|| {
                        format!("--until {sha}: nothing older within the scanned history")
                    })?
            }
        },
    };
    Ok((start, cutoff))
}

/// Author timestamp of a commit, for boundary fallback when the commit itself is a
/// merge and therefore missing from the merge-free scan.
fn commit_time(root: &Path, sha: &str) -> Result<i64> {
    let output = Command::new("git")
        .args(["show", "-s", "--format=%at", sha])
        .current_dir(root)
        .output()
        .context("running git show for a boundary timestamp")?;
    anyhow::ensure!(output.status.success(), "cannot read commit time of {sha}");
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .context("parsing a commit timestamp")
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
        // Whatever the indexer treats as code counts as ground truth. Hard-coding a
        // language list here silently produced zero tasks on a Java repository, which
        // looks like "no suitable commits" rather than like a bug.
        if reify::discover::classify(file).is_code() {
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

// ---- the held-out-hunk task set --------------------------------------------
//
// A retrieval task asks *which files should I open*. A held-out-hunk task asks the
// opposite question: given a patch that is deliberately incomplete, *what did it
// miss*? The construction is model-free — a merged commit is complete by definition,
// so removing one hunk from it manufactures a known omission and leaves the complete
// commit behind as a negative control. Nothing is hand-labelled and nothing is
// judged; the label is the hunk that was taken out.

/// One hunk of a unified diff, in pre-image coordinates.
///
/// Pre-image, because the index this is scored against is built at the parent commit:
/// post-image line numbers name lines that do not exist there.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hunk {
    /// First line of the hunk, context included.
    pub old_start: u32,
    /// Lines of the pre-image the hunk covers, context included.
    pub old_len: u32,
    /// Pre-image lines the hunk actually *changes*, with context excluded.
    ///
    /// Separate from the span because context lines routinely reach into the
    /// neighbouring function, and resolving a symbol from them would attribute a
    /// change to code the patch never touched.
    pub changed_lines: Vec<u32>,
}

impl Hunk {
    /// The first line this hunk changes, for resolving the symbol it lands in.
    pub fn first_changed(&self) -> u32 {
        self.changed_lines
            .first()
            .copied()
            .unwrap_or(self.old_start)
    }
}

/// One file's worth of a change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilePatch {
    /// Pre-image path, or the post-image path for a file the change creates.
    pub path: String,
    /// The change creates this file, so it has no pre-image and no indexed symbols.
    pub created: bool,
    pub hunks: Vec<Hunk>,
}

/// A change, as the set of files and pre-image lines it touches.
///
/// Structural rather than textual on purpose: the checker under test reads locations
/// and the graph, never diff text, so carrying the text would invite a checker that
/// greps it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Patch {
    pub files: Vec<FilePatch>,
}

impl Patch {
    /// The same change with one file left out entirely.
    fn without(&self, path: &str) -> Patch {
        Patch {
            files: self
                .files
                .iter()
                .filter(|f| f.path != path)
                .cloned()
                .collect(),
        }
    }
}

/// One held-out-hunk trial: a truncated change, and the complete one it came from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TruncatedTask {
    pub id: String,
    pub commit: String,
    /// The commit an index must be built at. The change is absent there by
    /// construction, which is the same guarantee `--after` gives the retrieval set.
    pub parent: String,
    pub date: String,
    /// The developer's own description, kept for tracing a result back to a change.
    pub prompt: String,
    /// The change as merged. Complete by construction, so every finding against it is
    /// a false positive — this is the negative control, not a second data point.
    pub complete: Patch,
    /// The change with the omission's file removed.
    pub truncated: Patch,
    /// The file whose only hunk was withheld.
    pub omission_file: String,
    /// First pre-image line the withheld hunk changes.
    pub omission_line: u32,
}

/// A frozen set of held-out-hunk trials.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TruncatedSet {
    pub repository: String,
    pub head: String,
    pub generated_from_commits: usize,
    /// Commits that passed every retrieval filter but could not be truncated, and why.
    /// Reported rather than dropped: a construction that silently discards most of its
    /// candidates is measuring the survivors, not the repository.
    pub rejected: Vec<(String, String)>,
    pub tasks: Vec<TruncatedTask>,
}

/// Build held-out-hunk trials from a repository's history.
///
/// Every filter `generate` applies applies here unchanged — the two share `take` and
/// `candidate` — plus two the construction needs:
///
/// 1. the change must touch **at least two** indexable files that exist at the parent,
///    so removing one leaves a patch behind;
/// 2. one of those files must be touched by **exactly one hunk**, which is the hunk
///    withheld. Removing it removes the file from the patch entirely, so a finding
///    that cites that file cannot be an echo of a hunk still present in it.
///
/// Among the files with exactly one hunk the **last by path order** is chosen. The
/// choice is arbitrary and fixed; it is made before any checker runs and there is no
/// knob on it.
pub fn generate_truncated(
    root: &Path,
    wanted: usize,
    scan: usize,
    after: Option<&str>,
    until: Option<&str>,
    exclude: &BTreeSet<String>,
) -> Result<TruncatedSet> {
    let head = gitlog::head_sha(root).context("reading HEAD")?;
    let history = gitlog::history(root, scan)?;
    let (start, cutoff) = window(root, &history, &head, after, until)?;

    let mut rejected: Vec<(String, String)> = Vec::new();
    let tasks = take(&history, start, cutoff, exclude, wanted, |commit| {
        let task = candidate(root, commit)?;
        if task.ground_truth.len() < 2 {
            return None; // a one-file change has nothing left after a truncation
        }
        let parent = match parent_of(root, &commit.sha) {
            Some(parent) => parent,
            None => {
                rejected.push((commit.sha.clone(), "no parent commit".into()));
                return None;
            }
        };
        let patch = match parse_patch(root, &commit.sha) {
            Ok(patch) => patch,
            Err(_) => {
                rejected.push((commit.sha.clone(), "unreadable diff".into()));
                return None;
            }
        };

        // Only files that exist at the parent and that the indexer treats as code can
        // carry a symbol the checker could ever cite.
        let indexable: Vec<&FilePatch> = patch
            .files
            .iter()
            .filter(|f| {
                !f.created
                    && !f.hunks.is_empty()
                    && reify::discover::classify(&f.path).is_code()
                    && exists_at(root, &parent, &f.path)
            })
            .collect();
        if indexable.len() < 2 {
            rejected.push((commit.sha.clone(), "fewer than two indexable files".into()));
            return None;
        }
        let Some(omission) = indexable
            .iter()
            .filter(|f| f.hunks.len() == 1)
            .max_by(|a, b| a.path.cmp(&b.path))
        else {
            rejected.push((
                commit.sha.clone(),
                "no file changed by exactly one hunk".into(),
            ));
            return None;
        };
        let omission_file = omission.path.clone();
        let omission_line = omission.hunks[0].first_changed();

        let complete = Patch {
            files: indexable.iter().map(|f| (*f).clone()).collect(),
        };
        Some(TruncatedTask {
            id: format!("v-{}", &commit.sha[..8]),
            commit: commit.sha.clone(),
            parent,
            date: commit.date(),
            prompt: task.prompt,
            truncated: complete.without(&omission_file),
            complete,
            omission_file,
            omission_line,
        })
    });

    Ok(TruncatedSet {
        repository: root.display().to_string(),
        head,
        generated_from_commits: history.commits.len(),
        rejected,
        tasks,
    })
}

/// First parent of a commit, or `None` for a root commit.
fn parent_of(root: &Path, sha: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", &format!("{sha}^")])
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// The change a commit made, against its first parent.
fn parse_patch(root: &Path, sha: &str) -> Result<Patch> {
    let output = Command::new("git")
        .args([
            "-c",
            "core.quotepath=false",
            "show",
            "--format=",
            "--no-color",
            // Renames would report a file as changed with no hunks in it, and a
            // similarity threshold is a knob this measurement should not have.
            "--no-renames",
            "--first-parent",
            sha,
        ])
        .current_dir(root)
        .output()
        .context("running git show for a patch")?;
    anyhow::ensure!(output.status.success(), "cannot read the diff of {sha}");
    Ok(parse_unified(&String::from_utf8_lossy(&output.stdout)))
}

/// Parse unified diff text into files and pre-image line ranges.
fn parse_unified(text: &str) -> Patch {
    let mut patch = Patch::default();
    let mut cursor = 0u32;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("--- ") {
            patch.files.push(FilePatch {
                path: strip_prefix_path(rest),
                created: rest == "/dev/null",
                hunks: Vec::new(),
            });
            continue;
        }
        let Some(file) = patch.files.last_mut() else {
            continue;
        };
        if let Some(rest) = line.strip_prefix("+++ ") {
            // A created file has no pre-image path, so the post-image one names it.
            if file.created {
                file.path = strip_prefix_path(rest);
            }
            continue;
        }
        if let Some(header) = line.strip_prefix("@@ ") {
            if let Some((old_start, old_len)) = parse_hunk_header(header) {
                file.hunks.push(Hunk {
                    old_start,
                    old_len,
                    changed_lines: Vec::new(),
                });
                cursor = old_start;
            }
            continue;
        }
        let Some(hunk) = file.hunks.last_mut() else {
            continue;
        };
        match line.chars().next() {
            Some(' ') => cursor += 1,
            Some('-') => {
                hunk.changed_lines.push(cursor);
                cursor += 1;
            }
            // An inserted line has no pre-image number of its own.
            //
            // When it replaces lines this hunk has already deleted, it needs no number:
            // those lines are recorded and the replacement is the same change. When it
            // is a genuine insertion it is attributed to the pre-image line it lands
            // *before*, not the one it lands after — code appended past the end of a
            // file then resolves to no symbol at all, which is the honest answer, where
            // attributing it backwards would credit the patch with changing a function
            // it only wrote underneath.
            Some('+') => {
                let replaces = cursor > 0 && hunk.changed_lines.last() == Some(&(cursor - 1));
                if !replaces {
                    hunk.changed_lines.push(cursor.max(hunk.old_start));
                }
            }
            // "\ No newline at end of file", or a blank line git emits as empty.
            _ => {}
        }
    }
    for file in &mut patch.files {
        for hunk in &mut file.hunks {
            hunk.changed_lines.sort_unstable();
            hunk.changed_lines.dedup();
        }
    }
    patch
}

/// `a/src/x.rs` -> `src/x.rs`; `/dev/null` -> empty.
fn strip_prefix_path(raw: &str) -> String {
    let raw = raw.trim_end();
    if raw == "/dev/null" {
        return String::new();
    }
    raw.split_once('/')
        .map_or(raw, |(_, rest)| rest)
        .to_string()
}

/// `-12,7 +12,8 @@ fn something` -> `(12, 7)`.
fn parse_hunk_header(header: &str) -> Option<(u32, u32)> {
    let old = header.split_whitespace().next()?.strip_prefix('-')?;
    let (start, len) = match old.split_once(',') {
        Some((start, len)) => (start, len.parse().ok()?),
        None => (old, 1u32),
    };
    Some((start.parse().ok()?, len))
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
    fn ground_truth_follows_the_indexer_rather_than_a_hard_coded_list() {
        // A Java repository must produce tasks; anything the indexer parses qualifies.
        for path in ["a/B.java", "a/b.py", "a/b.ts", "a/b.js", "db/x.sql"] {
            assert!(reify::discover::classify(path).is_code(), "{path}");
        }
        for path in ["docs/x.md", "a/b.json", "a/b.yml"] {
            assert!(!reify::discover::classify(path).is_code(), "{path}");
        }
    }

    #[test]
    fn the_repository_name_is_the_last_path_component() {
        let set = |repository: &str| TaskSet {
            repository: repository.into(),
            head: String::new(),
            generated_from_commits: 0,
            base: None,
            tasks: Vec::new(),
        };
        assert_eq!(set(".bench/medusa").repository_name(), "medusa");
        assert_eq!(set("/a/b/openmrs/").repository_name(), "openmrs");
        assert_eq!(set("reify").repository_name(), "reify");
    }

    const SAMPLE_DIFF: &str = "\
diff --git a/app/pricing.py b/app/pricing.py
index 111..222 100644
--- a/app/pricing.py
+++ b/app/pricing.py
@@ -10,6 +10,7 @@ class Pricing:
 ctx
 ctx
-    old_line
+    new_line
+    extra_line
 ctx
 ctx
@@ -40,3 +41,3 @@ def other():
 ctx
-    gone
+    added
diff --git a/app/new.py b/app/new.py
new file mode 100644
--- /dev/null
+++ b/app/new.py
@@ -0,0 +1,2 @@
+one
+two
";

    #[test]
    fn a_unified_diff_parses_into_pre_image_line_ranges() {
        let patch = parse_unified(SAMPLE_DIFF);
        assert_eq!(patch.files.len(), 2);
        let pricing = &patch.files[0];
        assert_eq!(pricing.path, "app/pricing.py");
        assert!(!pricing.created);
        assert_eq!(pricing.hunks.len(), 2);
        assert_eq!(
            (pricing.hunks[0].old_start, pricing.hunks[0].old_len),
            (10, 6)
        );
        // Line 12 is the deletion; the insertions replace it, so it is the only
        // pre-image line the hunk changes.
        assert_eq!(pricing.hunks[0].changed_lines, vec![12]);
        assert_eq!(pricing.hunks[1].changed_lines, vec![41]);
        assert_eq!(pricing.hunks[1].first_changed(), 41);
    }

    #[test]
    fn a_created_file_is_marked_and_named_from_its_post_image() {
        let patch = parse_unified(SAMPLE_DIFF);
        let created = &patch.files[1];
        assert!(
            created.created,
            "a file with no pre-image has no indexed symbols"
        );
        assert_eq!(created.path, "app/new.py");
    }

    #[test]
    fn code_appended_past_the_end_of_a_file_resolves_past_the_end() {
        // Attributing an append backwards would credit the patch with changing the
        // function it was written underneath. It changed no existing line.
        let patch = parse_unified(
            "--- a/a.py\n+++ b/a.py\n@@ -8,3 +8,5 @@\n ctx\n ctx\n ctx\n+new\n+new\n",
        );
        assert_eq!(patch.files[0].hunks[0].changed_lines, vec![11]);
    }

    #[test]
    fn truncating_removes_the_file_and_leaves_the_rest() {
        let patch = parse_unified(SAMPLE_DIFF);
        let truncated = patch.without("app/pricing.py");
        assert_eq!(truncated.files.len(), 1);
        assert_eq!(truncated.files[0].path, "app/new.py");
    }

    #[test]
    fn a_hunk_header_without_a_length_means_one_line() {
        assert_eq!(parse_hunk_header("-12 +12,3 @@ fn x"), Some((12, 1)));
        assert_eq!(parse_hunk_header("-12,7 +12,8 @@ fn x"), Some((12, 7)));
        assert_eq!(parse_hunk_header("nonsense"), None);
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

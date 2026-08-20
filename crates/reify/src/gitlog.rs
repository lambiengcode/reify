//! Git archaeology.
//!
//! Two granularities, deliberately. Indexing walks file-level history, which is cheap
//! and bounded. Precise symbol history is computed lazily at query time with
//! `git log -L`, because eagerly blaming a mature repository costs minutes and almost
//! all of it is never read. See `docs/PLAN.md` §H.5.
//!
//! Git is invoked as a subprocess rather than through a binding: a repository always
//! has `git`, the output formats used here are stable, and it keeps a large native
//! dependency out of the build.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Field separator in git's *output*. A NUL cannot appear in any of the fields.
const SEP: char = '\x00';

/// How long a query-time git call may take before it is abandoned.
///
/// `git log -L` normally answers in milliseconds, but on a partial (blobless or
/// treeless) clone it silently fetches objects over the network and can take a minute.
/// An interactive command must degrade to the history already indexed rather than
/// stall, so the call is bounded and its failure is not an error.
pub const QUERY_TIMEOUT: Duration = Duration::from_millis(1_500);

/// The `--format` argument that produces NUL-separated fields.
///
/// `%x00` rather than a literal NUL: argv is NUL-terminated, so a real NUL byte in the
/// argument cannot be passed to the child process at all. git expands the escape itself.
const LOG_FORMAT: &str = "--format=%H%x00%at%x00%an%x00%s";

/// What kind of change a commit made, inferred from its subject line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeClass {
    Fix,
    Feature,
    Refactor,
    Revert,
    Docs,
    Test,
    Chore,
    Other,
}

impl ChangeClass {
    pub fn as_str(self) -> &'static str {
        match self {
            ChangeClass::Fix => "fix",
            ChangeClass::Feature => "feat",
            ChangeClass::Refactor => "refactor",
            ChangeClass::Revert => "revert",
            ChangeClass::Docs => "docs",
            ChangeClass::Test => "test",
            ChangeClass::Chore => "chore",
            ChangeClass::Other => "other",
        }
    }

    /// Whether this class of commit tends to explain *why* code looks the way it does.
    ///
    /// Fixes and reverts encode incidents; chores and formatting encode nothing.
    pub fn is_explanatory(self) -> bool {
        matches!(
            self,
            ChangeClass::Fix | ChangeClass::Revert | ChangeClass::Feature | ChangeClass::Refactor
        )
    }
}

#[derive(Debug, Clone)]
pub struct Commit {
    pub sha: String,
    /// Unix timestamp of the author date.
    pub timestamp: i64,
    pub author: String,
    pub subject: String,
    pub class: ChangeClass,
    pub files: Vec<String>,
}

impl Commit {
    pub fn date(&self) -> String {
        format_date(self.timestamp)
    }
}

/// Repository history, newest commit first.
#[derive(Debug, Default)]
pub struct History {
    pub commits: Vec<Commit>,
    /// Whether the walk stopped at `max_commits` rather than at the root commit.
    pub truncated: bool,
}

impl History {
    /// Index positions of the commits touching each path, newest first.
    pub fn by_file(&self) -> HashMap<&str, Vec<usize>> {
        let mut map: HashMap<&str, Vec<usize>> = HashMap::new();
        for (i, commit) in self.commits.iter().enumerate() {
            for path in &commit.files {
                map.entry(path.as_str()).or_default().push(i);
            }
        }
        map
    }

    /// File pairs that change together at least `min_support` times.
    ///
    /// Co-change is the cheapest signal for the dependency that no call graph shows:
    /// the service and the report that must move together for reasons nobody wrote down.
    pub fn co_changes(&self, min_support: u32, max_pairs: usize) -> Vec<(String, String, u32)> {
        let mut counts: HashMap<(&str, &str), u32> = HashMap::new();
        for commit in &self.commits {
            // A sweeping commit couples everything to everything and tells us nothing.
            if commit.files.len() > 20 || commit.files.len() < 2 {
                continue;
            }
            let mut files: Vec<&str> = commit.files.iter().map(|s| s.as_str()).collect();
            files.sort_unstable();
            files.dedup();
            for i in 0..files.len() {
                for j in (i + 1)..files.len() {
                    *counts.entry((files[i], files[j])).or_default() += 1;
                }
            }
        }
        let mut pairs: Vec<(String, String, u32)> = counts
            .into_iter()
            .filter(|(_, n)| *n >= min_support)
            .map(|((a, b), n)| (a.to_string(), b.to_string(), n))
            .collect();
        // Sort by support then lexically so the truncation point is deterministic.
        pairs.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| (&a.0, &a.1).cmp(&(&b.0, &b.1))));
        pairs.truncate(max_pairs);
        pairs
    }
}

/// Is `root` inside a git working tree?
pub fn is_repository(root: &Path) -> bool {
    Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// The commit `HEAD` currently points at.
pub fn head_sha(root: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Walk history, newest first, up to `max_commits`.
pub fn history(root: &Path, max_commits: usize) -> Result<History> {
    let output = Command::new("git")
        .args([
            "log",
            "--no-merges",
            "--name-only",
            "-z",
            LOG_FORMAT,
            &format!("-n{max_commits}"),
        ])
        .current_dir(root)
        .output()
        .context("running git log")?;
    if !output.status.success() {
        anyhow::bail!(
            "git log failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut hist = parse_log(&text);
    hist.truncated = hist.commits.len() >= max_commits;
    Ok(hist)
}

/// Parse the output of `git log --name-only -z` with the format above.
///
/// The wire shape is easy to get wrong, so it is worth stating exactly. With `-z`,
/// git NUL-terminates every path *and* every `--format` field, and emits a newline
/// only after each format block. Records therefore run together:
///
/// ```text
/// <sha>\0<ts>\0<author>\0<subject>\0\n
/// <path>\0<path>\0<sha>\0<ts>\0<author>\0<subject>\0\n
/// <path>\0\n
/// ```
///
/// The next commit's header shares a line with the previous commit's file list. So
/// the rule is: on every line, the **last four** NUL-separated fields are a header,
/// and everything before them belongs to the commit already being built. Treating the
/// *first* field as the header instead silently yields exactly one commit for the
/// whole repository, with every path in history attributed to it.
fn parse_log(text: &str) -> History {
    let mut hist = History::default();
    let mut current: Option<Commit> = None;

    for line in text.split('\n') {
        let fields: Vec<&str> = line
            .split(SEP)
            .map(str::trim)
            .filter(|f| !f.is_empty())
            .collect();
        if fields.is_empty() {
            continue;
        }

        let header_at = fields
            .len()
            .checked_sub(4)
            .filter(|&i| is_header(&fields[i..]));
        let (paths, header) = match header_at {
            Some(i) => (&fields[..i], Some(&fields[i..])),
            None => (&fields[..], None),
        };

        if let Some(commit) = current.as_mut() {
            for path in paths {
                push_path(&mut commit.files, path);
            }
        }
        if let Some(header) = header {
            if let Some(commit) = current.take() {
                hist.commits.push(commit);
            }
            let subject = header[3].to_string();
            current = Some(Commit {
                sha: header[0].to_string(),
                timestamp: header[1].parse().unwrap_or(0),
                author: header[2].to_string(),
                class: classify(&subject),
                subject,
                files: Vec::new(),
            });
        }
    }
    if let Some(commit) = current.take() {
        hist.commits.push(commit);
    }
    hist
}

/// Do these four fields look like a commit header rather than four file paths?
fn is_header(fields: &[&str]) -> bool {
    fields.len() == 4
        && fields[0].len() == 40
        && fields[0].chars().all(|c| c.is_ascii_hexdigit())
        && fields[1].parse::<i64>().is_ok()
}

fn push_path(files: &mut Vec<String>, raw: &str) {
    let path = raw.trim().trim_matches('\u{0}');
    if !path.is_empty() {
        files.push(path.to_string());
    }
}

/// Commits that touched lines `start..=end` of `path`, newest first.
///
/// This is the precise, expensive query, run only when a user asks `reify why` about a
/// specific location.
pub fn line_history(
    root: &Path,
    path: &str,
    start: u32,
    end: u32,
    limit: usize,
) -> Result<Vec<Commit>> {
    let mut command = Command::new("git");
    command
        .args([
            "log",
            "--no-patch",
            LOG_FORMAT,
            &format!("-n{limit}"),
            "-L",
            &format!("{start},{end}:{path}"),
        ])
        .current_dir(root);
    let Some(output) = run_bounded(command, QUERY_TIMEOUT)? else {
        // Timed out or failed. A file added but never committed is the common
        // non-pathological cause; either way an empty history is the right answer.
        return Ok(Vec::new());
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut commits = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split(SEP).collect();
        if fields.len() >= 4 && fields[0].len() == 40 {
            let subject = fields[3].to_string();
            commits.push(Commit {
                sha: fields[0].to_string(),
                timestamp: fields[1].parse().unwrap_or(0),
                author: fields[2].to_string(),
                class: classify(&subject),
                subject,
                files: vec![path.to_string()],
            });
        }
    }
    Ok(commits)
}

/// Run a git command with a wall-clock bound.
///
/// Returns `Ok(None)` when the command failed or ran out of time; a slow repository is
/// a degraded answer, never an error the caller has to handle.
fn run_bounded(mut command: Command, timeout: Duration) -> Result<Option<std::process::Output>> {
    // Never let git block on a credential or terminal prompt inside a tool that is
    // being driven by an agent.
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().context("spawning git")?;
    let started = Instant::now();
    loop {
        match child.try_wait().context("waiting for git")? {
            Some(status) if !status.success() => return Ok(None),
            Some(_) => {
                let output = child.wait_with_output().context("reading git output")?;
                return Ok(Some(output));
            }
            None if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(None);
            }
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
}

/// Classify a commit from its subject line.
pub fn classify(subject: &str) -> ChangeClass {
    let s = subject.trim().to_ascii_lowercase();
    if s.starts_with("revert") || s.contains("this reverts commit") {
        return ChangeClass::Revert;
    }
    // Conventional-commit prefix, with or without a scope.
    let prefix = s.split([':', '(']).next().unwrap_or("").trim();
    match prefix {
        "fix" | "bugfix" | "hotfix" | "patch" => return ChangeClass::Fix,
        "feat" | "feature" => return ChangeClass::Feature,
        "refactor" | "perf" => return ChangeClass::Refactor,
        "docs" | "doc" => return ChangeClass::Docs,
        "test" | "tests" => return ChangeClass::Test,
        "chore" | "ci" | "build" | "style" | "deps" => return ChangeClass::Chore,
        _ => {}
    }
    // Fall back to keywords for repositories that never adopted the convention.
    if s.contains("fix") || s.contains("bug") || s.contains("issue #") || s.contains("regression") {
        ChangeClass::Fix
    } else if s.contains("refactor") || s.contains("cleanup") || s.contains("clean up") {
        ChangeClass::Refactor
    } else if s.contains("add ") || s.contains("implement") || s.contains("introduce") {
        ChangeClass::Feature
    } else {
        ChangeClass::Other
    }
}

/// Format a unix timestamp as `YYYY-MM-DD` in UTC.
///
/// Hand-rolled to avoid a date dependency for one format string; the civil-from-days
/// algorithm is Howard Hinnant's, and the tests pin it against known dates.
pub fn format_date(timestamp: i64) -> String {
    let days = timestamp.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(sha: &str, files: &[&str]) -> Commit {
        Commit {
            sha: sha.into(),
            timestamp: 0,
            author: "a".into(),
            subject: "s".into(),
            class: ChangeClass::Other,
            files: files.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn conventional_prefixes_classify_exactly() {
        assert_eq!(classify("fix: approval bypass"), ChangeClass::Fix);
        assert_eq!(classify("fix(order): approval bypass"), ChangeClass::Fix);
        assert_eq!(
            classify("feat(selling): add discount tier"),
            ChangeClass::Feature
        );
        assert_eq!(classify("refactor: extract policy"), ChangeClass::Refactor);
        assert_eq!(classify("docs: update BRD"), ChangeClass::Docs);
        assert_eq!(classify("chore(deps): bump"), ChangeClass::Chore);
    }

    #[test]
    fn reverts_are_recognised_before_anything_else() {
        assert_eq!(classify("Revert \"feat: add tier\""), ChangeClass::Revert);
        assert_eq!(
            classify("fix: undo, this reverts commit abc"),
            ChangeClass::Revert
        );
    }

    #[test]
    fn repositories_without_the_convention_fall_back_to_keywords() {
        assert_eq!(classify("Fix enterprise approval flow"), ChangeClass::Fix);
        assert_eq!(
            classify("Add strategic account handling"),
            ChangeClass::Feature
        );
        assert_eq!(classify("Clean up dead code"), ChangeClass::Refactor);
        assert_eq!(classify("Merge branch main"), ChangeClass::Other);
    }

    #[test]
    fn only_explanatory_classes_are_worth_surfacing_as_history() {
        assert!(ChangeClass::Fix.is_explanatory());
        assert!(ChangeClass::Revert.is_explanatory());
        assert!(!ChangeClass::Chore.is_explanatory());
        assert!(!ChangeClass::Docs.is_explanatory());
    }

    /// Build output in git's exact `-z` shape: paths of one commit share a line with
    /// the next commit's header, and only the header is newline-terminated.
    fn wire(records: &[(&str, i64, &str, &str, &[&str])]) -> String {
        let mut out = String::new();
        for (i, (sha, ts, author, subject, _files)) in records.iter().enumerate() {
            if i > 0 {
                // Files of the previous commit precede this header on the same line.
                for f in records[i - 1].4 {
                    out.push_str(f);
                    out.push(SEP);
                }
            }
            out.push_str(&format!("{sha}{SEP}{ts}{SEP}{author}{SEP}{subject}{SEP}\n"));
        }
        if let Some(last) = records.last() {
            for f in last.4 {
                out.push_str(f);
                out.push(SEP);
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn log_output_parses_into_commits_with_their_files() {
        let sha1 = "a".repeat(40);
        let sha2 = "b".repeat(40);
        let sha3 = "c".repeat(40);
        let text = wire(&[
            (
                &sha1,
                1_555_459_200,
                "Kai",
                "fix: approval flow",
                &["a.py", "b.py"],
            ),
            (&sha2, 1_555_372_800, "Lan", "feat: add tier", &["c.ts"]),
            (&sha3, 1_555_286_400, "Minh", "chore: bump", &["d.md"]),
        ]);
        let hist = parse_log(&text);
        assert_eq!(hist.commits.len(), 3, "every record must be recognised");
        assert_eq!(hist.commits[0].author, "Kai");
        assert_eq!(hist.commits[0].class, ChangeClass::Fix);
        assert_eq!(hist.commits[0].files, vec!["a.py", "b.py"]);
        assert_eq!(hist.commits[1].files, vec!["c.ts"]);
        assert_eq!(hist.commits[2].files, vec!["d.md"]);
    }

    #[test]
    fn a_subject_containing_a_nul_lookalike_does_not_split_a_record() {
        let sha = "e".repeat(40);
        let text = wire(&[(
            &sha,
            1_555_459_200,
            "Kai",
            "fix: handle a:b:c ids",
            &["x.py"],
        )]);
        let hist = parse_log(&text);
        assert_eq!(hist.commits.len(), 1);
        assert_eq!(hist.commits[0].subject, "fix: handle a:b:c ids");
    }

    #[test]
    fn a_forty_character_hex_path_is_not_mistaken_for_a_header() {
        // Guards the header heuristic: four consecutive path-like fields where the
        // first happens to be hex must not start a commit.
        let fields = ["a".repeat(40), "notanumber".into(), "x".into(), "y".into()];
        let refs: Vec<&str> = fields.iter().map(|s| s.as_str()).collect();
        assert!(!is_header(&refs));
    }

    #[test]
    fn by_file_indexes_commits_newest_first() {
        let hist = History {
            commits: vec![commit("aa", &["a.py"]), commit("bb", &["a.py", "b.py"])],
            ..Default::default()
        };
        let map = hist.by_file();
        assert_eq!(map["a.py"], vec![0, 1]);
        assert_eq!(map["b.py"], vec![1]);
    }

    #[test]
    fn co_change_requires_repeated_support() {
        let hist = History {
            commits: vec![
                commit("1", &["a.py", "b.py"]),
                commit("2", &["a.py", "b.py"]),
                commit("3", &["a.py", "c.py"]),
            ],
            ..Default::default()
        };
        let pairs = hist.co_changes(2, 10);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0], ("a.py".into(), "b.py".into(), 2));
    }

    #[test]
    fn sweeping_commits_are_excluded_from_co_change() {
        // A 500-file reformat would otherwise couple the entire repository.
        let wide: Vec<String> = (0..40).map(|i| format!("f{i}.py")).collect();
        let hist = History {
            commits: vec![Commit {
                sha: "x".into(),
                timestamp: 0,
                author: "a".into(),
                subject: "s".into(),
                class: ChangeClass::Other,
                files: wide,
            }],
            ..Default::default()
        };
        assert!(hist.co_changes(1, 100).is_empty());
    }

    #[test]
    fn co_change_truncation_is_deterministic() {
        let hist = History {
            commits: vec![
                commit("1", &["a.py", "b.py"]),
                commit("2", &["c.py", "d.py"]),
            ],
            ..Default::default()
        };
        let first = hist.co_changes(1, 1);
        let second = hist.co_changes(1, 1);
        assert_eq!(first, second);
    }

    #[test]
    fn dates_format_against_known_values() {
        assert_eq!(format_date(0), "1970-01-01");
        assert_eq!(format_date(1_555_459_200), "2019-04-17");
        assert_eq!(format_date(1_700_000_000), "2023-11-14");
    }

    #[test]
    fn the_log_format_argument_contains_no_real_nul_byte() {
        // argv is NUL-terminated; a literal NUL here makes every git call fail to spawn.
        assert!(!LOG_FORMAT.contains('\x00'));
        assert!(LOG_FORMAT.contains("%x00"));
    }

    #[test]
    fn a_query_time_git_call_is_bounded() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        if !is_repository(root) {
            return;
        }
        let started = Instant::now();
        // A path that does not exist makes git fail fast; the point is that the
        // bounded runner returns a value rather than propagating an error.
        let commits = line_history(root, "does/not/exist.rs", 1, 2, 3).unwrap();
        assert!(commits.is_empty());
        assert!(started.elapsed() < QUERY_TIMEOUT * 3);
    }

    #[test]
    fn a_command_that_outruns_its_budget_is_abandoned_not_awaited() {
        let mut sleeper = Command::new("sleep");
        sleeper.arg("30");
        let started = Instant::now();
        let result = run_bounded(sleeper, Duration::from_millis(120)).unwrap();
        assert!(result.is_none());
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the child was not killed"
        );
    }

    #[test]
    fn history_of_this_repository_is_readable() {
        // Integration-flavoured but hermetic: the crate always lives in a git tree.
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        if !is_repository(root) {
            return;
        }
        let hist = history(root, 5).expect("git log should succeed in a repository");
        assert!(!hist.commits.is_empty());
        assert!(hist.commits.iter().all(|c| c.sha.len() == 40));
        // Distinct shas: the -z record shape once collapsed the whole history onto one.
        let distinct: std::collections::BTreeSet<&str> =
            hist.commits.iter().map(|c| c.sha.as_str()).collect();
        assert_eq!(distinct.len(), hist.commits.len());
        assert!(head_sha(root).is_some());
    }
}

//! Should this repository use Reify at all?
//!
//! A tool that always recommends itself is worthless, and this project has already
//! published a repository — `benchmarks/REPORT-medusa.md` — where Reify ties grep. The
//! most valuable answer this module can give is a confident *no*, because that is what
//! makes the *yes* worth anything.
//!
//! # Where the signals come from
//!
//! Four repositories were measured end to end (`benchmarks/REPORT*.md`), and two
//! hypotheses were tested against them. Both failed:
//!
//! - **Size.** OFBiz has 1,364 code files and shows the largest margin over grep
//!   (70% against 12%); Medusa has 11,821 and shows none (18% against 18%).
//! - **Declared vocabulary.** OFBiz declares almost nothing and still wins.
//!
//! Two signals *do* fit all four outcomes, and each explains a different way the tool
//! fails. Measured over the newest [`MAX_COMMITS`] commits of each repository:
//!
//! | | grep margin | commit focus | subject→path |
//! |---|---:|---:|---:|
//! | OFBiz | +58 | 0.96 | 0.80 |
//! | ERPNext | +48 | 0.98 | 0.85 |
//! | OpenMRS | +9 | 0.98 | **0.48** |
//! | Medusa | 0 | **0.84** | 0.79 |
//!
//! **Commit focus** is the share of commits touching few enough files that their
//! subject says something about them. Medusa is the only measured repository where it
//! falls away, and it is the only one where Reify did not win. That is not a
//! coincidence: Reify attaches a commit's vocabulary to every file it touched, so a
//! history of sweeping squashed merges smears each subject across the tree. The same
//! assumption is already load-bearing in [`crate::gitlog::History::co_changes`], which
//! skips commits touching more than [`FOCUSED_COMMIT_FILES`] files because "a sweeping
//! commit couples everything to everything and tells us nothing".
//!
//! **Subject→path locality** is the share of those focused commits whose subject shares
//! a word with a path it changed — the direct test of whether the words a change is
//! described in point at the code it touches. OpenMRS is the one measured repository
//! where it falls away, and it is the one whose margin over grep was small.
//!
//! Between them the two account for every measured outcome, without either one having
//! to explain a case it does not fit.
//!
//! # What was tried and dropped
//!
//! Corpus-wide overlap between commit vocabulary and path vocabulary — the obvious
//! reading of "history and file naming speak the same vocabulary" — was measured first
//! and **inverts**: Medusa scores 0.43 against OFBiz's 0.38. Pooling every subject into
//! one bag throws away the attribution that makes the signal mean anything, so it is
//! not computed here. Neither is a 0-100 suitability score: `docs/metrics.md` forbids
//! printing a number that cannot be defined, and a weighted blend of heuristics tuned on
//! four repositories is exactly that.
//!
//! # What this is not
//!
//! A heuristic fitted to four repositories, not a measurement of yours. `reify-bench`
//! measures a specific repository; this reads one in about a second.

use anyhow::Result;
use std::collections::BTreeSet;
use std::path::Path;

use crate::concepts::{meaningful_words, stem};
use crate::discover::{self, Discovery};
use crate::gitlog;
use crate::model::Lang;

pub const SCHEMA: &str = "reify.doctor/1";

/// The line count below which the README's FAQ already says not to bother.
///
/// Deliberately the number the documentation publishes — "Under roughly 20k LOC Reify
/// buys you nothing a grep and a scroll wheel don't" — rather than a second, quieter
/// threshold that contradicts it.
pub const LINES_FLOOR: u64 = 20_000;

/// How far back history is read.
///
/// Bounded because a doctor that takes a minute does not get run. One `git log
/// --name-only` of a thousand commits answers in well under a second even on a
/// repository with a hundred thousand of them.
pub const MAX_COMMITS: usize = 1_000;

/// A commit touching more files than this tells you nothing about any of them.
///
/// The same threshold [`crate::gitlog::History::co_changes`] already applies, for the
/// same reason, so the two agree about what a meaningful commit is.
pub const FOCUSED_COMMIT_FILES: usize = 20;

/// Share of commits that must be focused for history to be usable evidence.
///
/// The three measured repositories where Reify won all sit at 0.96 or above; Medusa,
/// where it tied, sits at 0.84.
const FOCUS_OK: f32 = 0.90;

/// Share of focused commits whose subject must name something in a path it changed.
///
/// OFBiz 0.80, ERPNext 0.85 and Medusa 0.79 clear it; OpenMRS, whose margin over grep
/// was 9 points rather than 48, sits at 0.48.
const LOCALITY_STRONG: f32 = 0.70;

/// Enough commits that the shares above can be told apart from their thresholds.
///
/// Not a round number picked for feel. Medusa — the measured repository that fails the
/// focus test — sits at 0.84. A 95% Wilson interval around 0.84 lies entirely below
/// [`FOCUS_OK`] at n = 200 (upper bound 0.88) but straddles it at n = 50 (upper bound
/// 0.92). Below this a `no` would be an artefact of the sample size, so a short history
/// is reported as short rather than condemned.
const MIN_COMMITS: usize = 200;

/// How much of the repository will be indexed at all.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Scale {
    /// Files Reify would index.
    pub indexable_files: usize,
    /// Of those, files in a language Reify parses as code.
    pub code_files: usize,
    /// Lines across every indexable file. Includes blanks and comments.
    pub lines: u64,
}

/// Do the words a change is described in point at the code it touches?
#[derive(Debug, Clone, serde::Serialize)]
pub struct Vocabulary {
    /// Focused commits with a usable subject — the denominator.
    pub commits_considered: usize,
    /// Of those, commits whose subject shares a word with a path they changed.
    pub commits_local: usize,
    /// `commits_local / commits_considered`.
    pub locality: f32,
}

/// Is the history attributable, or is every subject smeared across the tree?
#[derive(Debug, Clone, serde::Serialize)]
pub struct HistorySignal {
    /// Commits read, bounded by [`MAX_COMMITS`].
    pub commits_read: usize,
    /// Whether the walk stopped at that bound rather than at the root commit.
    pub truncated: bool,
    /// Commits whose subject carries at least two meaningful words.
    pub usable_subjects: usize,
    /// `usable_subjects / commits_read`. Sat at 1.0 on all five repositories this was
    /// calibrated against, so it discriminates nothing there — but a history of `wip`
    /// and version bumps is real, and this is what would catch it.
    pub usable_share: f32,
    /// Commits touching at most [`FOCUSED_COMMIT_FILES`] files.
    pub focused_commits: usize,
    /// `focused_commits / commits_read`.
    pub focus: f32,
    /// Files changed by the median commit.
    pub median_files_changed: usize,
}

/// Documents whose text a grep cannot reach.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Documents {
    /// Files in a format Reify converts and an agent cannot read.
    pub unreadable_by_grep: usize,
    /// A few examples, so the claim can be checked.
    pub examples: Vec<String>,
}

/// The answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Under the line floor. Nothing else was worth measuring.
    TooSmall,
    /// The signals that separated the measured repositories are present here.
    LikelyWorthIt,
    /// Mixed. Worth measuring rather than guessing.
    Marginal,
    /// This repository has the shape of the one where Reify tied grep.
    UnlikelyToHelp,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::TooSmall => "TOO SMALL",
            Verdict::LikelyWorthIt => "LIKELY WORTH IT",
            Verdict::Marginal => "MARGINAL",
            Verdict::UnlikelyToHelp => "UNLIKELY TO HELP",
        }
    }
}

/// One measured repository, named so a reader can check the comparison.
pub struct Comparable {
    pub name: &'static str,
    pub outcome: &'static str,
    pub report: &'static str,
}

/// The measured repository where Reify did worst.
///
/// Pointed at every favourable verdict on purpose: naming a repository where Reify won
/// proves nothing to someone deciding whether to spend an afternoon on it.
pub const MEDUSA: Comparable = Comparable {
    name: "Medusa",
    outcome: "Reify tied grep — 18% of tasks each",
    report: "benchmarks/REPORT-medusa.md",
};

/// The measured repository where Reify did best.
pub const OFBIZ: Comparable = Comparable {
    name: "OFBiz",
    outcome: "Reify reached a changed file on 70% of tasks against grep's 12%",
    report: "benchmarks/REPORT-ofbiz.md",
};

#[derive(Debug, Clone, serde::Serialize)]
pub struct Diagnosis {
    pub schema: &'static str,
    pub root: String,
    pub scale: Scale,
    pub git_repository: bool,
    /// Absent below the floor, and when history cannot be read.
    pub vocabulary: Option<Vocabulary>,
    /// Absent below the floor, and when history cannot be read.
    pub history: Option<HistorySignal>,
    pub documents: Documents,
    pub verdict: Verdict,
    /// The one sentence carrying the verdict.
    pub reason: String,
    /// What would change a no or a maybe. Empty for a clear yes.
    pub what_would_change_it: Vec<String>,
    /// Wall clock of the measurement itself.
    pub elapsed_ms: u64,
}

/// Formats whose text a grep cannot reach.
///
/// The one categorical advantage: no agent greps a PDF. RTF is nominally text, but its
/// words are broken up by control words, so a grep on it misleads rather than fails —
/// which is worse.
fn unreadable_by_grep(lang: Lang) -> bool {
    matches!(
        lang,
        Lang::Docx | Lang::Doc | Lang::Odt | Lang::Rtf | Lang::Xlsx | Lang::Pptx | Lang::Pdf
    )
}

/// Does this subject say anything about the change?
///
/// Two meaningful words is a low bar that "Merge pull request #123 from acme/topic",
/// "Bump version to 4.2.1" and "wip" all fail and that any sentence describing a change
/// passes. Merges are excluded outright: a merge subject names a branch, not a change.
pub fn subject_is_usable(subject: &str) -> bool {
    !subject.starts_with("Merge ") && meaningful_words(subject).len() >= 2
}

/// Stem-folded words of a string, so `customer` and `customers` are one word.
fn stems(text: &str) -> BTreeSet<String> {
    meaningful_words(text)
        .iter()
        .map(|w| stem(w).to_string())
        .collect()
}

/// Read the repository and decide.
///
/// Reads the working tree and `git log`, never the store: the whole point is deciding
/// before committing to the tool, so the answer must not depend on having run it. It
/// also means the answer does not change once `reify index` has run — there is nothing
/// in the store this would rather use.
pub fn diagnose(root: &Path) -> Result<Diagnosis> {
    let started = std::time::Instant::now();
    let found = discover::discover(root)?;
    let scale = measure_scale(&found);
    let documents = measure_documents(&found);
    let git_repository = gitlog::is_repository(root);

    // Below the floor nothing else is worth measuring, and saying so plainly is the
    // whole value of the answer.
    if scale.lines < LINES_FLOOR {
        return Ok(Diagnosis {
            schema: SCHEMA,
            root: root.display().to_string(),
            reason: format!(
                "{}. Under roughly {} lines, ripgrep and a scroll wheel do this job. \
                 Nothing else here is worth measuring.",
                scale_text(&scale),
                floor_text()
            ),
            scale,
            git_repository,
            vocabulary: None,
            history: None,
            documents,
            verdict: Verdict::TooSmall,
            what_would_change_it: Vec::new(),
            elapsed_ms: started.elapsed().as_millis() as u64,
        });
    }

    // A repository whose history git will not read still gets an answer, with the
    // signals honestly absent rather than silently defaulted.
    let log = git_repository
        .then(|| gitlog::history(root, MAX_COMMITS).ok())
        .flatten();
    let history = log.as_ref().map(measure_history);
    let vocabulary = log.as_ref().map(measure_vocabulary);

    let (verdict, reason, what_would_change_it) =
        decide(vocabulary.as_ref(), history.as_ref(), &documents);

    Ok(Diagnosis {
        schema: SCHEMA,
        root: root.display().to_string(),
        scale,
        git_repository,
        vocabulary,
        history,
        documents,
        verdict,
        reason,
        what_would_change_it,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

fn measure_scale(found: &Discovery) -> Scale {
    Scale {
        indexable_files: found.files.len(),
        code_files: found.files.iter().filter(|f| f.lang.is_code()).count(),
        lines: found.files.iter().map(|f| u64::from(f.lines)).sum(),
    }
}

/// Count document formats across everything walked, indexed or not.
///
/// Both lists are read on purpose: a `.docx` is binary, so discovery records it as
/// skipped even though indexing converts it. Counting only the indexable list would
/// report zero documents for a repository full of them.
fn measure_documents(found: &Discovery) -> Documents {
    let mut examples = Vec::new();
    let mut count = 0;
    let paths = found
        .files
        .iter()
        .map(|f| f.path.as_str())
        .chain(found.skipped.iter().map(|(p, _)| p.as_str()));
    for path in paths {
        if unreadable_by_grep(discover::classify(path)) {
            count += 1;
            if examples.len() < 3 {
                examples.push(path.to_string());
            }
        }
    }
    examples.sort();
    Documents {
        unreadable_by_grep: count,
        examples,
    }
}

fn measure_history(log: &gitlog::History) -> HistorySignal {
    let commits_read = log.commits.len();
    let usable = log
        .commits
        .iter()
        .filter(|c| subject_is_usable(&c.subject))
        .count();
    let mut sizes: Vec<usize> = log.commits.iter().map(|c| c.files.len()).collect();
    sizes.sort_unstable();
    let focused = sizes
        .iter()
        .filter(|&&n| (1..=FOCUSED_COMMIT_FILES).contains(&n))
        .count();
    HistorySignal {
        commits_read,
        truncated: log.truncated,
        usable_subjects: usable,
        usable_share: share(usable, commits_read),
        focused_commits: focused,
        focus: share(focused, commits_read),
        median_files_changed: sizes.get(sizes.len() / 2).copied().unwrap_or(0),
    }
}

/// How often a commit subject names something in a path that commit changed.
///
/// Per commit rather than pooled across the repository. The pooled version — every
/// subject word against every path word — was measured on the same four repositories
/// and inverts, because pooling throws away the attribution that makes the question
/// mean anything: it asks whether the words appear *somewhere*, not whether they point
/// at the code that actually changed.
fn measure_vocabulary(log: &gitlog::History) -> Vocabulary {
    let mut considered = 0;
    let mut local = 0;
    for commit in &log.commits {
        if !(1..=FOCUSED_COMMIT_FILES).contains(&commit.files.len())
            || !subject_is_usable(&commit.subject)
        {
            continue;
        }
        considered += 1;
        let subject = stems(&commit.subject);
        if commit
            .files
            .iter()
            .any(|path| stems(path).iter().any(|w| subject.contains(w)))
        {
            local += 1;
        }
    }
    Vocabulary {
        commits_considered: considered,
        commits_local: local,
        locality: share(local, considered),
    }
}

fn share(part: usize, whole: usize) -> f32 {
    if whole == 0 {
        0.0
    } else {
        part as f32 / whole as f32
    }
}

/// Below this, a line count is printed exactly rather than abbreviated.
///
/// Rounding to the nearest thousand is at most a 5% misstatement here and grows worse
/// the smaller the number gets: at 2 lines it is not an abbreviation, it is a wrong
/// answer. In a command whose whole job is honest measurement, that is the one thing it
/// must not do.
const ABBREVIATE_LINES_ABOVE: u64 = 10_000;

/// A line count, abbreviated only where abbreviating is not misleading.
///
/// Carries its own `~` when it is approximate, so no caller can mark an exact figure as
/// an estimate or an estimate as exact.
///
/// Discovery counts lines, not statements: blanks and comments are in there. Above the
/// threshold a figure printed to the unit invites it to be read as a measurement of code
/// size, which it is not.
pub fn lines_text(lines: u64) -> String {
    if lines < ABBREVIATE_LINES_ABOVE {
        return lines.to_string();
    }
    format!("~{}k", (lines as f64 / 1000.0).round() as u64)
}

/// The line floor, for prose that names it. A threshold, so never marked approximate.
pub fn floor_text() -> String {
    format!("{}k", LINES_FLOOR / 1000)
}

/// `n file` or `n files`.
///
/// Trivial, and shared rather than inlined so the signal line and the verdict sentence
/// cannot disagree about the same count.
fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("{n} {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// The scale signal as one phrase, used by both the signal line and the verdict.
///
/// One function rather than two format strings: they state the same measurement, and the
/// only reason they were ever two was that nobody had noticed the duplication yet.
pub fn scale_text(scale: &Scale) -> String {
    format!(
        "{}, {} lines",
        count(scale.indexable_files, "indexable file"),
        lines_text(scale.lines)
    )
}

/// Pick the verdict, and say what it rests on.
///
/// Four rules, each traceable to a measured repository:
///
/// - a history of sweeping commits is the Medusa shape, the one case measured where
///   Reify won nothing;
/// - subjects that do name the code they change is the OFBiz and ERPNext shape, the two
///   large margins;
/// - subjects that do not, over an otherwise focused history, is the OpenMRS shape,
///   where Reify won by 9 points rather than 48;
/// - documents no grep can read are a categorical advantage rather than a comparative
///   one, so they are stated wherever they exist.
fn decide(
    vocabulary: Option<&Vocabulary>,
    history: Option<&HistorySignal>,
    documents: &Documents,
) -> (Verdict, String, Vec<String>) {
    let documents_note = format!(
        "{} document(s) here hold text no grep can reach, and Reify converts and \
         indexes them",
        documents.unreadable_by_grep
    );

    let (Some(vocabulary), Some(history)) = (vocabulary, history) else {
        let mut changes = vec![
            "`reify-bench` measures this repository directly, rather than comparing its \
             shape to four others."
                .to_string(),
        ];
        if documents.unreadable_by_grep == 0 {
            changes.push(
                "A readable git history. Reify reads commit subjects to connect a change \
                 request to code, and without one the strongest signal is missing."
                    .to_string(),
            );
        }
        return (
            Verdict::Marginal,
            format!(
                "No readable git history, so neither measured signal can be computed here.{}",
                if documents.unreadable_by_grep > 0 {
                    format!(" What is clear is that {documents_note}.")
                } else {
                    String::new()
                }
            ),
            changes,
        );
    };

    if history.commits_read < MIN_COMMITS {
        return (
            Verdict::Marginal,
            format!(
                "Only {} commits to read. Both measured signals are shares over commits, \
                 and below {MIN_COMMITS} their confidence intervals straddle the \
                 thresholds — so a verdict either way would be an artefact of the sample \
                 size rather than a reading of this repository.",
                history.commits_read
            ),
            vec![
                format!("More history: at least {MIN_COMMITS} commits."),
                "`reify-bench` measures this repository directly, and does not need a \
                 long history to do it."
                    .to_string(),
            ],
        );
    }

    // The Medusa shape. Reify attaches a commit's vocabulary to every file it touched,
    // so a history of sweeping merges spreads each subject across the tree.
    if history.focus < FOCUS_OK {
        let reason = format!(
            "Commits here are sweeping: only {} touch few enough files for their subject \
             to say anything about them, and the median commit changes {} files. Reify \
             attaches a commit's words to every file it touched, so that history is \
             spread too thin to retrieve on. This is the shape of the one measured \
             repository where Reify tied grep.",
            percent(history.focus),
            history.median_files_changed
        );
        if documents.unreadable_by_grep > 0 {
            return (
                Verdict::Marginal,
                format!("{reason} Against that, {documents_note} — which is an advantage no search tool recovers however well it is used."),
                vec![
                    "Nothing, for retrieval. The documents are the reason to run it here, \
                     not the ranking."
                        .to_string(),
                ],
            );
        }
        return (
            Verdict::UnlikelyToHelp,
            reason,
            vec![
                "Smaller commits, whose subject names what they changed. Squashed merges \
                 of a hundred files carry no vocabulary any one of them can be found by."
                    .to_string(),
                "Business documents in `.docx`, `.pdf`, `.xlsx` or `.pptx` committed to \
                 the tree. Reify reads those and an agent cannot, whatever the history \
                 looks like."
                    .to_string(),
            ],
        );
    }

    // The OFBiz and ERPNext shape: the words changes are described in name the code.
    if vocabulary.locality >= LOCALITY_STRONG {
        let mut reason = format!(
            "{} of this repository's focused commits have a subject naming something in \
             a path they changed. That agreement between how changes are described and \
             how code is named is what separated the repositories where Reify helped \
             from the one where it did not.",
            percent(vocabulary.locality)
        );
        if documents.unreadable_by_grep > 0 {
            reason.push_str(&format!(" On top of that, {documents_note}."));
        }
        return (Verdict::LikelyWorthIt, reason, Vec::new());
    }

    // The OpenMRS shape: an attributable history whose subjects nonetheless describe
    // changes in words the code does not use. Measured a modest win, not a large one.
    let mut reason = format!(
        "History here is attributable, but only {} of its commits have a subject naming \
         something in a path they changed. Of the four measured repositories the one \
         that looked like this beat grep by 9 points rather than 48.",
        percent(vocabulary.locality)
    );
    if documents.unreadable_by_grep > 0 {
        reason.push_str(&format!(
            " That said, {documents_note}, which is an advantage no search tool recovers."
        ));
        return (Verdict::LikelyWorthIt, reason, Vec::new());
    }
    (
        Verdict::Marginal,
        reason,
        vec![
            "Commit subjects that name the thing being changed, in the words the code \
             uses for it. That is the signal Reify's retrieval is built on."
                .to_string(),
            "A declared glossary — `.reify/glossary.toml` — which bridges the words your \
             team uses to the identifiers the code uses."
                .to_string(),
            "`reify-bench` measures this repository, rather than comparing its shape to \
             four others."
                .to_string(),
        ],
    )
}

pub fn percent(fraction: f32) -> String {
    format!("{}%", (fraction * 100.0).round() as i64)
}

/// The measured repository a verdict should be read against.
pub fn comparable(verdict: Verdict) -> Option<Comparable> {
    match verdict {
        Verdict::LikelyWorthIt | Verdict::Marginal => Some(MEDUSA),
        Verdict::UnlikelyToHelp => Some(OFBIZ),
        Verdict::TooSmall => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("reify-doctor-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn vocab(locality: f32) -> Vocabulary {
        Vocabulary {
            commits_considered: 400,
            commits_local: (400.0 * locality) as usize,
            locality,
        }
    }

    fn history(focus: f32) -> HistorySignal {
        HistorySignal {
            commits_read: 500,
            truncated: true,
            usable_subjects: 500,
            usable_share: 1.0,
            focused_commits: (500.0 * focus) as usize,
            focus,
            median_files_changed: if focus < FOCUS_OK { 4 } else { 1 },
        }
    }

    fn no_documents() -> Documents {
        Documents {
            unreadable_by_grep: 0,
            examples: Vec::new(),
        }
    }

    #[test]
    fn a_tiny_repository_is_told_not_to_bother_and_nothing_else_is_measured() {
        let d = tmp("tiny");
        fs::write(d.join("main.py"), "def f():\n    return 1\n").unwrap();
        let answer = diagnose(&d).unwrap();
        assert_eq!(answer.verdict, Verdict::TooSmall);
        assert!(
            answer.vocabulary.is_none() && answer.history.is_none(),
            "below the floor nothing else is worth measuring"
        );
        assert!(answer.reason.contains("ripgrep"));
        assert!(
            answer.reason.starts_with("1 indexable file, 2 lines."),
            "a two-line repository is reported as two lines, not as ~1k: {}",
            answer.reason
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn the_floor_is_the_one_the_documentation_publishes() {
        // A second, quieter threshold that contradicts the README would be worse than
        // no threshold at all.
        assert_eq!(LINES_FLOOR, 20_000);
    }

    #[test]
    fn the_focus_threshold_agrees_with_what_co_change_already_calls_a_sweeping_commit() {
        // Two different numbers for "a commit too broad to learn from" would be two
        // different definitions of the same thing.
        assert_eq!(FOCUSED_COMMIT_FILES, 20);
    }

    // The four measured repositories, as they were measured over their newest 500
    // commits. These are the fit; a threshold change that reclassifies one of them is
    // a change of claim, not a tweak.
    #[test]
    fn each_measured_repository_lands_where_its_benchmark_says_it_should() {
        let cases = [
            // repo, focus, locality, expected
            ("ofbiz +58", 0.956, 0.803, Verdict::LikelyWorthIt),
            ("erpnext +48", 0.982, 0.845, Verdict::LikelyWorthIt),
            ("openmrs +9", 0.976, 0.480, Verdict::Marginal),
            ("medusa +0", 0.836, 0.792, Verdict::UnlikelyToHelp),
        ];
        for (name, focus, locality, expected) in cases {
            let (verdict, _, _) = decide(
                Some(&vocab(locality)),
                Some(&history(focus)),
                &no_documents(),
            );
            assert_eq!(verdict, expected, "{name}");
        }
    }

    #[test]
    fn a_sweeping_history_is_a_no_and_says_what_would_change_it() {
        let (verdict, reason, changes) =
            decide(Some(&vocab(0.79)), Some(&history(0.84)), &no_documents());
        assert_eq!(
            verdict,
            Verdict::UnlikelyToHelp,
            "strong subject vocabulary must not rescue a history it cannot be attributed to"
        );
        assert!(reason.contains("tied grep"));
        assert!(!changes.is_empty(), "a no must say what would change it");
    }

    #[test]
    fn documents_no_grep_can_read_are_stated_wherever_they_exist() {
        let documents = Documents {
            unreadable_by_grep: 12,
            examples: vec!["docs/spec.pdf".into()],
        };
        // They lift the OpenMRS shape to a yes...
        let (verdict, reason, _) = decide(Some(&vocab(0.48)), Some(&history(0.97)), &documents);
        assert_eq!(verdict, Verdict::LikelyWorthIt);
        assert!(reason.contains("12 document"));

        // ...and they are the reason to bother even where retrieval looks unpromising,
        // but they do not turn a sweeping history into a good one.
        let (verdict, reason, _) = decide(Some(&vocab(0.79)), Some(&history(0.84)), &documents);
        assert_eq!(verdict, Verdict::Marginal);
        assert!(reason.contains("tied grep") && reason.contains("12 document"));
    }

    #[test]
    fn an_unreadable_history_is_marginal_rather_than_a_guess() {
        let (verdict, reason, changes) = decide(None, None, &no_documents());
        assert_eq!(verdict, Verdict::Marginal);
        assert!(reason.contains("No readable git history"));
        assert!(changes.iter().any(|c| c.contains("reify-bench")));
    }

    #[test]
    fn too_little_history_is_admitted_rather_than_measured() {
        let mut thin = history(0.5);
        thin.commits_read = MIN_COMMITS - 1;
        let (verdict, reason, changes) = decide(Some(&vocab(0.9)), Some(&thin), &no_documents());
        assert_eq!(
            verdict,
            Verdict::Marginal,
            "a sweeping-looking history too short to measure is reported as short, not \
             condemned: at this sample size the interval straddles the threshold"
        );
        assert!(reason.contains(&format!("{} commits", MIN_COMMITS - 1)));
        assert!(changes.iter().any(|c| c.contains("More history")));
    }

    #[test]
    fn a_history_of_merges_and_version_bumps_does_not_read_as_usable() {
        assert!(!subject_is_usable(
            "Merge pull request #123 from acme/topic"
        ));
        assert!(!subject_is_usable("wip"));
        assert!(!subject_is_usable("v1.2.3"));
        assert!(subject_is_usable(
            "fix: sales order approval ignores the credit limit"
        ));
    }

    #[test]
    fn locality_counts_a_subject_that_names_a_path_it_changed() {
        let commit = |subject: &str, files: &[&str]| gitlog::Commit {
            sha: "0".repeat(40),
            timestamp: 0,
            author: "a".into(),
            subject: subject.into(),
            class: gitlog::classify(subject),
            files: files.iter().map(|f| f.to_string()).collect(),
        };
        let log = gitlog::History {
            commits: vec![
                commit("fix invoice rounding", &["app/invoice.py"]),
                commit("tighten the release checklist", &["app/invoice.py"]),
                // Excluded: too sweeping to attribute either way.
                commit("reformat everything", &vec!["f.py"; 40]),
            ],
            truncated: false,
        };
        let measured = measure_vocabulary(&log);
        assert_eq!(measured.commits_considered, 2, "the sweep is not counted");
        assert_eq!(measured.commits_local, 1);
        assert_eq!(measured.locality, 0.5);
    }

    #[test]
    fn a_yes_is_pointed_at_the_least_favourable_measured_repository() {
        // Naming a repository where Reify won proves nothing to someone deciding
        // whether to spend an afternoon on it.
        assert_eq!(comparable(Verdict::LikelyWorthIt).unwrap().name, "Medusa");
        assert_eq!(comparable(Verdict::UnlikelyToHelp).unwrap().name, "OFBiz");
        assert!(comparable(Verdict::TooSmall).is_none());
    }

    #[test]
    fn documents_are_counted_even_though_discovery_skips_them_as_binary() {
        let d = tmp("docs");
        // Real `.docx` bytes are a zip; what matters here is the NUL that makes
        // discovery classify it as binary and skip it.
        fs::write(d.join("spec.docx"), [0x50, 0x4b, 0x03, 0x04, 0x00, 0x01]).unwrap();
        fs::write(d.join("a.py"), "x = 1\n").unwrap();
        let found = discover::discover(&d).unwrap();
        assert!(
            found.files.iter().all(|f| f.path != "spec.docx"),
            "the premise of this test: discovery skips it as binary"
        );
        assert_eq!(measure_documents(&found).unreadable_by_grep, 1);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_line_count_is_abbreviated_only_where_abbreviating_is_not_misleading() {
        // Large counts are estimates and say so.
        assert_eq!(lines_text(612_345), "~612k");
        assert_eq!(lines_text(10_000), "~10k");
        // Small ones are exact. Rounding 2 lines up to "1k" in a command whose job is
        // honest measurement is the one thing it must not do.
        assert_eq!(lines_text(9_999), "9999");
        assert_eq!(lines_text(4_200), "4200");
        assert_eq!(lines_text(2), "2");
        assert_eq!(lines_text(0), "0");
        // A threshold is never marked approximate.
        assert_eq!(floor_text(), "20k");
    }

    #[test]
    fn a_count_of_one_reads_as_one() {
        let one = Scale {
            indexable_files: 1,
            code_files: 1,
            lines: 2,
        };
        assert_eq!(scale_text(&one), "1 indexable file, 2 lines");
        assert_eq!(
            scale_text(&Scale {
                indexable_files: 4_178,
                code_files: 2_974,
                lines: 713_087,
            }),
            "4178 indexable files, ~713k lines"
        );
    }
}

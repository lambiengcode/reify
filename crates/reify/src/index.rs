//! The indexing pipeline.
//!
//! Ten stages, of which the first seven are deterministic and always run. Nothing here
//! touches the network. See `docs/PLAN.md` §H.
//!
//! The load-bearing property is that an incremental run produces the same store as a
//! full rebuild. That is achieved by giving every stage a disjoint set of edge kinds
//! and its own invalidation trigger (see [`crate::store::CONTENT_EDGE_KINDS`]), and by
//! persisting unresolved references so a changed file can be re-resolved against the
//! whole repository rather than only against itself.

use anyhow::{Context, Result};
use rayon::prelude::*;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::concepts::{self, Bridge, Concept, Glossary, TermIndex};
use crate::discover::{self, Discovered, Discovery};
use crate::extract::{self, code::SymbolIndex, FileExtract};
use crate::gitlog;
use crate::model::{uid, EdgeKind, Lang, NewEdge, NewNode, NodeKind, Status};
use crate::rules::{self, RuleCandidate};
use crate::store::{Batch, FileRecord, Store};

/// Where Reify keeps its store and configuration, relative to the repository root.
pub const REIFY_DIR: &str = ".reify";
pub const STORE_FILE: &str = "store.db";
pub const GLOSSARY_FILE: &str = "glossary.toml";

/// Default bound on how far back history is walked.
///
/// A fifteen-year monorepo must not make the first `reify index` a forty-minute
/// operation; the applied bound is reported rather than silently imposed.
pub const DEFAULT_MAX_COMMITS: usize = 20_000;

/// `(path, language, rows)` for one translation file.
type TranslationFile = (String, String, Vec<(String, String)>);

/// Fact kinds persisted per file so repository-wide stages can rebuild without
/// re-parsing unchanged files. See [`crate::store::Store::put_facts`].
const FACT_RULE: &str = "rule";
const FACT_CONCEPT: &str = "concept";
const FACT_TRANSLATION: &str = "translation";

/// Minimum number of shared commits before two files are called co-changing.
const CO_CHANGE_MIN_SUPPORT: u32 = 3;
const CO_CHANGE_MAX_PAIRS: usize = 20_000;

/// Most recent commits linked per file. Older history stays reachable through
/// `reify why`, which queries git directly for a precise line range.
const COMMITS_PER_FILE: usize = 8;

/// Called as indexing advances. `(stage, done, total)`, where `total` is zero when a
/// stage has no countable unit of work.
///
/// A callback rather than printing: a library that writes to stderr cannot be embedded,
/// and indexing a large repository takes over a minute, which without any output is
/// indistinguishable from a hang.
pub type ProgressFn = Arc<dyn Fn(&str, usize, usize) + Send + Sync>;

#[derive(Clone)]
pub struct IndexOptions {
    pub root: PathBuf,
    /// Rebuild from scratch, ignoring cached per-file results.
    pub force: bool,
    pub max_commits: usize,
    /// Optional progress reporting.
    pub progress: Option<ProgressFn>,
}

impl std::fmt::Debug for IndexOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IndexOptions")
            .field("root", &self.root)
            .field("force", &self.force)
            .field("max_commits", &self.max_commits)
            .field("progress", &self.progress.is_some())
            .finish()
    }
}

impl IndexOptions {
    /// Report progress, if a callback is installed.
    fn report(&self, stage: &str, done: usize, total: usize) {
        if let Some(progress) = &self.progress {
            progress(stage, done, total);
        }
    }
}

impl IndexOptions {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        IndexOptions {
            root: root.into(),
            force: false,
            max_commits: DEFAULT_MAX_COMMITS,
            progress: None,
        }
    }

    pub fn store_path(&self) -> PathBuf {
        self.root.join(REIFY_DIR).join(STORE_FILE)
    }

    pub fn glossary_path(&self) -> PathBuf {
        self.root.join(REIFY_DIR).join(GLOSSARY_FILE)
    }
}

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct IndexReport {
    pub files_total: usize,
    pub files_parsed: usize,
    pub files_unchanged: usize,
    pub files_removed: usize,
    pub files_skipped: usize,
    pub skip_reasons: Vec<(String, usize)>,
    pub symbols: u64,
    pub doc_sections: u64,
    pub database_objects: u64,
    pub concepts: u64,
    pub entities: u64,
    pub commits: u64,
    pub rules: u64,
    pub conflicts: usize,
    pub edges: u64,
    pub unresolved_refs: usize,
    pub history_rebuilt: bool,
    pub history_truncated: bool,
    pub parse_errors: Vec<String>,
    pub elapsed_ms: u128,
}

impl IndexReport {
    /// Whether this run did the cheap thing: nothing changed.
    pub fn was_noop(&self) -> bool {
        self.files_parsed == 0 && self.files_removed == 0 && !self.history_rebuilt
    }
}

/// Run the pipeline against `store`.
pub fn index(store: &mut Store, opts: &IndexOptions) -> Result<IndexReport> {
    let started = Instant::now();
    let mut report = IndexReport::default();

    // --- 1-2. discover and classify ------------------------------------------
    opts.report("discovering files", 0, 0);
    let found: Discovery = discover::discover(&opts.root)
        .with_context(|| format!("walking {}", opts.root.display()))?;
    report.files_total = found.files.len();
    report.files_skipped = found.skipped.len();
    report.skip_reasons = found
        .skip_summary()
        .into_iter()
        .map(|(reason, n)| (reason.to_string(), n))
        .collect();

    // --- decide what actually needs work -------------------------------------
    let previous = store.file_hashes()?;
    let present: HashSet<&str> = found.files.iter().map(|f| f.path.as_str()).collect();
    for gone in previous.keys().filter(|p| !present.contains(p.as_str())) {
        store.forget_file(gone)?;
        report.files_removed += 1;
    }
    let changed: Vec<&Discovered> = found
        .files
        .iter()
        .filter(|f| opts.force || previous.get(&f.path) != Some(&f.hash))
        .collect();
    report.files_parsed = changed.len();
    report.files_unchanged = found.files.len() - changed.len();

    // Nothing changed, and `HEAD` has not moved: the store is already correct, and
    // every repository-wide stage below would rebuild an identical result. This is the
    // common case when `reify index` runs from a git hook, so it is worth an early
    // exit rather than seven seconds of recomputing what is already there.
    let head_now = gitlog::head_sha(&opts.root);
    let head_unchanged = head_now == store.meta("head_sha")?;
    if !opts.force
        && changed.is_empty()
        && report.files_removed == 0
        && head_unchanged
        && store.count_of_kind(NodeKind::Symbol)? > 0
    {
        report.symbols = store.count_of_kind(NodeKind::Symbol)?;
        report.doc_sections = store.count_of_kind(NodeKind::DocSection)?;
        report.database_objects = store.count_of_kind(NodeKind::DatabaseObject)?;
        report.concepts = store.count_of_kind(NodeKind::Concept)?;
        report.entities = report.database_objects;
        report.commits = store.count_of_kind(NodeKind::Commit)?;
        report.edges = store.count_edges()?;
        report.elapsed_ms = started.elapsed().as_millis();
        return Ok(report);
    }

    for file in &changed {
        store.forget_file_content(&file.path)?;
    }

    // --- 3-6. parse changed files in parallel --------------------------------
    // Extraction is pure: text in, staged knowledge out. That is what makes it safe
    // to run across cores and what would make it cacheable by content hash later.
    opts.report("parsing", 0, changed.len());
    let parsed = AtomicUsize::new(0);
    let outcomes: Vec<FileOutcome> = changed
        .par_iter()
        .map(|file| {
            let outcome = extract_file(file);
            // Reported in batches rather than per file: a callback that writes to a
            // terminal thousands of times measurably slows a parallel stage down.
            let done = parsed.fetch_add(1, Ordering::Relaxed) + 1;
            if done % 64 == 0 || done == changed.len() {
                opts.report("parsing", done, changed.len());
            }
            outcome
        })
        .collect();

    let mut staged = Batch::default();
    let mut pending: Vec<(String, String, String, EdgeKind)> = Vec::new();
    let mut translations: Vec<TranslationFile> = Vec::new();
    let mut mined_rules: Vec<(String, Vec<String>)> = Vec::new();
    let mut declared_concepts: Vec<(String, Vec<String>)> = Vec::new();

    for (file, outcome) in changed.iter().zip(outcomes) {
        match outcome {
            FileOutcome::Failed(message) => {
                // One unparseable file must never fail a whole index; it is reported.
                report
                    .parse_errors
                    .push(format!("{}: {message}", file.path));
                staged.node(file_node(file));
                continue;
            }
            FileOutcome::Parsed(parsed) => {
                let ParsedFile { extract, rows } = *parsed;
                staged.node(file_node(file));
                staged.absorb(extract.batch);
                for reference in extract.pending {
                    pending.push((
                        reference.from,
                        reference.name,
                        reference.file,
                        reference.kind,
                    ));
                }
                for module in extract.imports {
                    if let Some(target) = resolve_module(&module, &file.path, &present) {
                        staged.edge(NewEdge::new(
                            uid::file(&file.path),
                            uid::file(&target),
                            EdgeKind::Imports,
                            Status::Observed,
                            0.9,
                        ));
                    }
                }
                if let Some((lang, rows)) = rows {
                    translations.push((file.path.clone(), lang, rows));
                }
                if !extract.concepts.is_empty() {
                    declared_concepts.push((
                        file.path.clone(),
                        extract
                            .concepts
                            .iter()
                            .filter_map(|c| serde_json::to_string(c).ok())
                            .collect(),
                    ));
                }
                if !extract.rules.is_empty() {
                    let payloads: Vec<String> = extract
                        .rules
                        .iter()
                        .filter_map(|r| serde_json::to_string(r).ok())
                        .collect();
                    mined_rules.push((file.path.clone(), payloads));
                }
            }
        }
        store.upsert_file(&FileRecord {
            path: file.path.clone(),
            lang: file.lang,
            hash: file.hash.clone(),
            bytes: file.bytes,
            lines: file.lines,
        })?;
    }

    let stats = store.commit(staged)?;
    report.unresolved_refs = stats.unresolved_edges;
    store.put_refs(&pending)?;
    for (path, payloads) in &mined_rules {
        store.put_facts(FACT_RULE, path, payloads)?;
    }
    for (path, payloads) in &declared_concepts {
        store.put_facts(FACT_CONCEPT, path, payloads)?;
    }
    for (path, lang, rows) in &translations {
        let payloads: Vec<String> = rows
            .iter()
            .filter_map(|(source, target)| serde_json::to_string(&(lang, source, target)).ok())
            .collect();
        store.put_facts(FACT_TRANSLATION, path, &payloads)?;
    }

    // --- 7. resolve every reference in the repository ------------------------
    // Repository-wide rather than incremental on purpose: editing one file changes
    // which symbols other files' references resolve to, and re-resolving is a hash
    // lookup per reference.
    opts.report("resolving references", 0, 0);
    let mut symbols = SymbolIndex::default();
    for (name, node_uid, path, lang) in store.symbol_triples()? {
        symbols.add(&name, &node_uid, &path, lang);
    }
    let all_refs = store.all_refs()?;
    let pending_refs: Vec<extract::PendingRef> = all_refs
        .into_iter()
        .map(|(from, name, file, kind)| extract::PendingRef {
            from,
            name,
            file,
            kind,
        })
        .collect();
    let resolved = extract::code::resolve(&pending_refs, &symbols);
    let stats = store.commit(resolved)?;
    report.unresolved_refs += stats.unresolved_edges;

    // --- 8. concepts, rebuilt globally every run -----------------------------
    opts.report("linking concepts", 0, 0);
    store.forget_concepts()?;
    let mut grounding = TermIndex::default();
    for (name, node_uid) in store.groundable_names()? {
        grounding.add(&name, &node_uid);
    }
    let glossary = Glossary::load(&opts.glossary_path())?;
    let mut sources = vec![glossary.concepts];

    // Concepts declared by structured model metadata, from every file — including
    // files this run did not parse, which is exactly what the fact store is for.
    sources.push(
        store
            .all_facts(FACT_CONCEPT)?
            .iter()
            .filter_map(|payload| serde_json::from_str::<Concept>(payload).ok())
            .collect(),
    );

    // Translation rows, grouped by target language.
    let mut by_language: std::collections::BTreeMap<String, Vec<(String, String)>> =
        std::collections::BTreeMap::new();
    for payload in store.all_facts(FACT_TRANSLATION)? {
        if let Ok((lang, source, target)) =
            serde_json::from_str::<(String, String, String)>(&payload)
        {
            by_language.entry(lang).or_default().push((source, target));
        }
    }
    for (lang, rows) in &by_language {
        sources.push(concepts::from_translations(lang, rows, &grounding));
    }
    let headings: Vec<String> = store
        .nodes_of_kind(NodeKind::DocSection)?
        .into_iter()
        .map(|n| n.name)
        .collect();
    sources.push(concepts::from_headings(&headings, &grounding));
    let merged = concepts::merge(sources);
    store.commit(concepts::stage(&merged, &grounding))?;

    // --- 9. rules and conflicts, rebuilt globally every run ------------------
    // Corroboration and conflict detection are repository-wide: a rule's confidence
    // depends on what other files say, so the layer is rebuilt whole rather than
    // patched. The candidates themselves are per-file and were invalidated above.
    opts.report("mining rules", 0, 0);
    store.forget_rules()?;
    let mut candidates: Vec<RuleCandidate> = store
        .all_facts(FACT_RULE)?
        .iter()
        .filter_map(|payload| serde_json::from_str(payload).ok())
        .collect();
    rules::corroborate(&mut candidates);
    let conflicts = rules::detect_conflicts(&candidates);
    report.conflicts = conflicts.len();
    store.commit(rules::stage(&candidates, &conflicts))?;

    // --- 10. history, rebuilt only when HEAD moves ---------------------------
    let head = gitlog::head_sha(&opts.root);
    let stored_head = store.meta("head_sha")?;
    let history_stale =
        opts.force || head.is_none() != stored_head.is_none() || head != stored_head;
    if gitlog::is_repository(&opts.root) && history_stale {
        opts.report("reading history", 0, 0);
        report.history_rebuilt = true;
        store.forget_history()?;
        let history = gitlog::history(&opts.root, opts.max_commits)?;
        report.history_truncated = history.truncated;
        store.commit(stage_history(&history, &present))?;
        if let Some(sha) = &head {
            store.set_meta("head_sha", sha)?;
        }
    }

    // --- 11. finish ----------------------------------------------------------
    opts.report("finishing", 0, 0);
    store.set_meta("indexed_at", &format!("{}", now_unix()))?;
    store.set_meta("root", &opts.root.to_string_lossy())?;
    store.optimize()?;

    report.symbols = store.count_of_kind(NodeKind::Symbol)?;
    report.doc_sections = store.count_of_kind(NodeKind::DocSection)?;
    report.database_objects = store.count_of_kind(NodeKind::DatabaseObject)?;
    report.concepts = store.count_of_kind(NodeKind::Concept)?;
    report.entities = store.count_of_kind(NodeKind::DatabaseObject)?;
    report.commits = store.count_of_kind(NodeKind::Commit)?;
    report.rules = store.count_of_kind(NodeKind::BusinessRule)? - report.conflicts as u64;
    report.edges = store.count_edges()?;
    report.elapsed_ms = started.elapsed().as_millis();
    Ok(report)
}

enum FileOutcome {
    /// Boxed because a `FileExtract` is large and this enum is returned once per file
    /// across the whole repository; the failure variant should not pay for its size.
    Parsed(Box<ParsedFile>),
    Failed(String),
}

struct ParsedFile {
    extract: FileExtract,
    /// Translation rows, when the file is a localisation table.
    rows: Option<(String, Vec<(String, String)>)>,
}

/// Extract one file. Pure with respect to Reify's own state: no store access and no
/// shared mutable state. Rich document formats may invoke an external text extractor.
fn extract_file(file: &Discovered) -> FileOutcome {
    // Binary document formats are read as bytes, never as UTF-8 text.
    if file.lang == Lang::Docx {
        return match std::fs::read(&file.abs)
            .map_err(|e| anyhow::anyhow!("{e}"))
            .and_then(|bytes| extract::richdoc::docx_to_markdown(&bytes))
        {
            Ok(markdown) => document_outcome(file, &markdown),
            Err(e) => FileOutcome::Failed(e.to_string()),
        };
    }
    if file.lang == Lang::Pdf {
        return match extract::richdoc::pdf_to_text(&file.abs) {
            Ok(text) => document_outcome(file, &text),
            Err(e) => FileOutcome::Failed(e.to_string()),
        };
    }

    let text = match file.read() {
        Ok(t) => t,
        Err(e) => return FileOutcome::Failed(e.to_string()),
    };
    let text = if file.lang == Lang::Html {
        extract::richdoc::html_to_markdown(&text)
    } else {
        text
    };
    match file.lang {
        Lang::Python | Lang::TypeScript | Lang::JavaScript | Lang::Java => {
            match extract::code::extract(&file.path, &text, file.lang) {
                Ok(mut fx) => {
                    // SQL embedded in source is attributed to the enclosing symbol, so
                    // the access edge points at the function performing the query.
                    let spans = symbol_spans(&fx);
                    let owner_at = |line: u32| owner_for_line(&spans, line);
                    fx.absorb(extract::sqlish::extract_embedded(&text, &owner_at));
                    FileOutcome::Parsed(Box::new(ParsedFile {
                        extract: fx,
                        rows: None,
                    }))
                }
                Err(e) => FileOutcome::Failed(e.to_string()),
            }
        }
        Lang::Sql => FileOutcome::Parsed(Box::new(ParsedFile {
            extract: extract::sqlish::extract_file(&file.path, &text),
            rows: None,
        })),
        Lang::Markdown | Lang::Text | Lang::Html | Lang::Docx | Lang::Pdf => {
            document_outcome(file, &text)
        }
        Lang::Csv => {
            let rows = concepts::translation_language(&file.path)
                .map(|lang| (lang, concepts::parse_translation_csv(&text)));
            FileOutcome::Parsed(Box::new(ParsedFile {
                extract: FileExtract::default(),
                rows,
            }))
        }
        Lang::Json => FileOutcome::Parsed(Box::new(ParsedFile {
            extract: extract::schema::extract(&file.path, &text),
            rows: None,
        })),
        Lang::Yaml | Lang::Other => FileOutcome::Parsed(Box::new(ParsedFile {
            extract: FileExtract::default(),
            rows: None,
        })),
    }
}

/// Stage a document, whatever format it arrived in.
fn document_outcome(file: &Discovered, text: &str) -> FileOutcome {
    match extract::docs::extract(&file.path, text, file.lang) {
        Ok(fx) => FileOutcome::Parsed(Box::new(ParsedFile {
            extract: fx,
            rows: None,
        })),
        Err(e) => FileOutcome::Failed(e.to_string()),
    }
}

/// `(start, end, uid)` for every symbol staged by an extraction, innermost last.
fn symbol_spans(fx: &FileExtract) -> Vec<(u32, u32, String)> {
    let mut spans: Vec<(u32, u32, String)> = fx
        .batch
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Symbol)
        .map(|n| (n.line_start, n.line_end, n.uid.clone()))
        .collect();
    // Narrowest last, so a linear scan finds the innermost owner.
    spans.sort_by_key(|(start, end, _)| std::cmp::Reverse(end.saturating_sub(*start)));
    spans
}

fn owner_for_line(spans: &[(u32, u32, String)], line: u32) -> Option<String> {
    spans
        .iter()
        .rfind(|(start, end, _)| *start <= line && line <= *end)
        .map(|(_, _, uid)| uid.clone())
}

fn file_node(file: &Discovered) -> NewNode {
    NewNode::new(uid::file(&file.path), NodeKind::File, &file.path)
        .at(&file.path, 0, file.lines)
        .lang(file.lang)
        .status(Status::Confirmed, 1.0)
        .search(file.path.replace(['/', '_', '-', '.'], " "))
        .data(serde_json::json!({
            "lines": file.lines,
            "bytes": file.bytes,
            "summary": format!("file {}", file.path),
        }))
}

/// Map an import specifier onto a file in this repository, if it names one.
///
/// Deliberately conservative: a specifier that does not land on a known path produces
/// no edge rather than a guess. Third-party imports are supposed to miss.
fn resolve_module(module: &str, from: &str, present: &HashSet<&str>) -> Option<String> {
    let candidates: Vec<String> = if module.starts_with('.') {
        let base = from.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
        let mut relative = module.trim_start_matches('.').replace('.', "/");
        let up = module.len() - module.trim_start_matches('.').len();
        let mut dir: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
        for _ in 1..up {
            dir.pop();
        }
        relative = relative.trim_start_matches('/').to_string();
        let joined = if dir.is_empty() {
            relative
        } else {
            format!("{}/{}", dir.join("/"), relative)
        };
        expand_extensions(&joined)
    } else {
        expand_extensions(&module.replace('.', "/"))
    };
    candidates
        .into_iter()
        .find(|c| present.contains(c.as_str()))
}

fn expand_extensions(stem: &str) -> Vec<String> {
    let stem = stem.trim_end_matches('/');
    [
        format!("{stem}.py"),
        format!("{stem}/__init__.py"),
        format!("{stem}.ts"),
        format!("{stem}.tsx"),
        format!("{stem}/index.ts"),
        format!("{stem}.js"),
    ]
    .into_iter()
    .collect()
}

/// Stage commit nodes and the history edges that point at indexed files.
fn stage_history(history: &gitlog::History, present: &HashSet<&str>) -> Batch {
    let mut batch = Batch::default();
    let by_file = history.by_file();
    let mut linked: BTreeSet<usize> = BTreeSet::new();

    for (path, positions) in by_file {
        if !present.contains(path) {
            continue;
        }
        let file_uid = uid::file(path);
        // Newest first, so the last entry is the introducing commit within the walked
        // window. When the walk was truncated this is the oldest commit we can see,
        // not necessarily the true first — reported via `history_truncated`.
        if let Some(&oldest) = positions.last() {
            linked.insert(oldest);
            batch.edge(NewEdge::new(
                file_uid.clone(),
                uid::commit(&history.commits[oldest].sha),
                EdgeKind::IntroducedBy,
                Status::Confirmed,
                1.0,
            ));
        }
        for &position in positions.iter().take(COMMITS_PER_FILE) {
            let commit = &history.commits[position];
            // Chores and formatting explain nothing about why code looks as it does.
            if !commit.class.is_explanatory() {
                continue;
            }
            linked.insert(position);
            batch.edge(NewEdge::new(
                file_uid.clone(),
                uid::commit(&commit.sha),
                EdgeKind::ChangedBy,
                Status::Confirmed,
                1.0,
            ));
        }
    }

    for position in linked {
        let commit = &history.commits[position];
        batch.node(
            NewNode::new(
                uid::commit(&commit.sha),
                NodeKind::Commit,
                &commit.sha[..7.min(commit.sha.len())],
            )
            .status(Status::Confirmed, 1.0)
            .search(commit.subject.clone())
            .data(serde_json::json!({
                "sha": commit.sha,
                "date": commit.date(),
                "author": commit.author,
                "subject": commit.subject,
                "class": commit.class.as_str(),
                "summary": format!("{} {}", commit.date(), commit.subject),
            })),
        );
    }

    for (a, b, support) in history.co_changes(CO_CHANGE_MIN_SUPPORT, CO_CHANGE_MAX_PAIRS) {
        if !present.contains(a.as_str()) || !present.contains(b.as_str()) {
            continue;
        }
        // Confidence rises with support but never reaches certainty: co-change is
        // evidence of coupling, not proof of it.
        let confidence = (0.4 + 0.05 * support as f32).min(0.85);
        batch.edge(NewEdge::new(
            uid::file(&a),
            uid::file(&b),
            EdgeKind::CoChangesWith,
            Status::Observed,
            confidence,
        ));
    }
    batch
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Concept counts by bridge, for `reify report`.
pub fn concept_bridges(store: &Store) -> Result<HashMap<String, u64>> {
    let mut counts: HashMap<String, u64> = HashMap::new();
    for node in store.nodes_of_kind(NodeKind::Concept)? {
        let bridge = node
            .data
            .get("bridge")
            .and_then(|v| v.as_str())
            .unwrap_or(Bridge::CoOccurrence.as_str())
            .to_string();
        *counts.entry(bridge).or_default() += 1;
    }
    Ok(counts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn fixture(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "reify-index-{}-{name}-{}",
            std::process::id(),
            now_unix()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("app")).unwrap();
        fs::create_dir_all(dir.join("docs")).unwrap();
        fs::create_dir_all(dir.join("translations")).unwrap();

        fs::write(
            dir.join("app/order.py"),
            r#"
class SalesOrder:
    """A customer order."""

    def requires_approval(self):
        if self.customer_group == 7:
            return self.bypass_level_two()
        return True

    def bypass_level_two(self):
        rows = self.db.sql("SELECT name FROM `tabSales Order` WHERE docstatus = 1")
        return rows
"#,
        )
        .unwrap();
        fs::write(
            dir.join("app/strategic.py"),
            "class StrategicAccount:\n    def rate(self):\n        return 0.15\n",
        )
        .unwrap();
        fs::write(
            dir.join("docs/BRD-42.md"),
            "# Approval BRD\n\n## Sales Order approval\n\nOrders above 50M require L2 approval.\n",
        )
        .unwrap();
        fs::write(
            dir.join("translations/vi.csv"),
            "Strategic Account,khách hàng chiến lược\nSales Order,đơn bán hàng\nPlease wait,Vui lòng đợi\n",
        )
        .unwrap();
        dir
    }

    fn index_fresh(root: &Path) -> (Store, IndexReport) {
        let mut store = Store::in_memory().unwrap();
        let opts = IndexOptions::new(root);
        let report = index(&mut store, &opts).unwrap();
        (store, report)
    }

    #[test]
    fn progress_is_reported_for_every_stage() {
        use std::sync::Mutex;
        let root = fixture("progress");
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let recorder = seen.clone();
        let opts = IndexOptions {
            progress: Some(Arc::new(move |stage: &str, _, _| {
                recorder.lock().expect("lock").push(stage.to_string());
            })),
            ..IndexOptions::new(&root)
        };
        let mut store = Store::in_memory().unwrap();
        index(&mut store, &opts).unwrap();

        let stages = seen.lock().unwrap().clone();
        for expected in [
            "discovering files",
            "parsing",
            "linking concepts",
            "finishing",
        ] {
            assert!(
                stages.iter().any(|s| s == expected),
                "missing stage {expected}: {stages:?}"
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn indexing_works_with_no_progress_callback() {
        let root = fixture("noprogress");
        let (_, report) = index_fresh(&root);
        assert!(report.symbols > 0);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_full_index_finds_symbols_docs_tables_and_concepts() {
        let root = fixture("full");
        let (store, report) = index_fresh(&root);

        assert!(report.symbols >= 5, "symbols: {}", report.symbols);
        assert!(report.doc_sections >= 2);
        assert!(
            report.database_objects >= 1,
            "the embedded SQL table must be found"
        );
        assert!(
            report.concepts >= 1,
            "the translation bridge must produce concepts"
        );
        assert!(report.parse_errors.is_empty(), "{:?}", report.parse_errors);

        assert!(store
            .node_by_uid("sym:app/order.py#SalesOrder.requires_approval")
            .unwrap()
            .is_some());
        assert!(store.node_by_uid("db:tabsales order").unwrap().is_some());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn embedded_sql_is_attributed_to_the_method_that_runs_it() {
        let root = fixture("sql");
        let (store, _) = index_fresh(&root);
        let method = store
            .node_by_uid("sym:app/order.py#SalesOrder.bypass_level_two")
            .unwrap()
            .unwrap();
        let out = store
            .neighbors(method.id, crate::store::Direction::Out, &[EdgeKind::Reads])
            .unwrap();
        assert_eq!(out.len(), 1, "expected one table read");
        assert_eq!(out[0].0.uid, "db:tabsales order");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_vietnamese_term_reaches_english_code_through_a_concept() {
        let root = fixture("vi");
        let (store, _) = index_fresh(&root);
        let concept = store
            .node_by_uid("concept:STRATEGIC_ACCOUNT")
            .unwrap()
            .expect("the grounded translation row must become a concept");
        assert_eq!(concept.data["labels"]["vie"], "khách hàng chiến lược");

        let mapped = store
            .neighbors(
                concept.id,
                crate::store::Direction::Out,
                &[EdgeKind::MapsTo],
            )
            .unwrap();
        let targets: Vec<&str> = mapped.iter().map(|(n, _, _)| n.uid.as_str()).collect();
        assert!(
            targets.contains(&"sym:app/strategic.py#StrategicAccount"),
            "got {targets:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn ungrounded_translation_rows_do_not_become_concepts() {
        let root = fixture("ungrounded");
        let (store, _) = index_fresh(&root);
        assert!(
            store.node_by_uid("concept:PLEASE_WAIT").unwrap().is_none(),
            "a UI string must not become a business concept"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn calls_resolve_across_the_repository() {
        let root = fixture("calls");
        let (store, _) = index_fresh(&root);
        let caller = store
            .node_by_uid("sym:app/order.py#SalesOrder.requires_approval")
            .unwrap()
            .unwrap();
        let out = store
            .neighbors(caller.id, crate::store::Direction::Out, &[EdgeKind::Calls])
            .unwrap();
        let names: Vec<&str> = out.iter().map(|(n, _, _)| n.name.as_str()).collect();
        assert!(names.contains(&"bypass_level_two"), "got {names:?}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn reindexing_an_unchanged_tree_parses_nothing_and_rebuilds_nothing() {
        let root = fixture("noop");
        let mut store = Store::in_memory().unwrap();
        let opts = IndexOptions::new(&root);
        let first = index(&mut store, &opts).unwrap();
        let second = index(&mut store, &opts).unwrap();

        assert_eq!(second.files_parsed, 0);
        assert!(second.files_unchanged > 0);
        assert!(second.was_noop());
        // The early exit must not lose counts: a caller reads them either way.
        assert_eq!(second.symbols, first.symbols);
        assert_eq!(second.concepts, first.concepts);
        assert_eq!(second.edges, first.edges);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_early_exit_leaves_the_store_byte_identical() {
        // The exit is only safe because a rebuild would produce the same thing.
        let root = fixture("noop-identical");
        let mut store = Store::in_memory().unwrap();
        let opts = IndexOptions::new(&root);
        index(&mut store, &opts).unwrap();
        let before = store.canonical_dump().unwrap();
        index(&mut store, &opts).unwrap();
        assert_eq!(before, store.canonical_dump().unwrap());

        // And a forced rebuild must reach the same state the exit preserved.
        let mut rebuilt = Store::in_memory().unwrap();
        index(
            &mut rebuilt,
            &IndexOptions {
                force: true,
                ..IndexOptions::new(&root)
            },
        )
        .unwrap();
        assert_eq!(before, rebuilt.canonical_dump().unwrap());
        let _ = fs::remove_dir_all(&root);
    }

    /// The property the whole incremental design exists to preserve.
    #[test]
    fn incremental_indexing_equals_a_full_rebuild() {
        let root = fixture("equiv");
        let opts = IndexOptions::new(&root);

        // Build incrementally through a sequence of realistic edits.
        let mut incremental = Store::in_memory().unwrap();
        index(&mut incremental, &opts).unwrap();

        fs::write(
            root.join("app/strategic.py"),
            "class StrategicAccount:\n    def rate(self):\n        return 0.20\n\n    def tier(self):\n        return 'S'\n",
        )
        .unwrap();
        index(&mut incremental, &opts).unwrap();

        fs::write(
            root.join("app/invoice.py"),
            "class Invoice:\n    def total(self):\n        return 0\n",
        )
        .unwrap();
        index(&mut incremental, &opts).unwrap();

        fs::remove_file(root.join("docs/BRD-42.md")).unwrap();
        index(&mut incremental, &opts).unwrap();

        fs::write(
            root.join("docs/BRD-43.md"),
            "# Pricing\n\n## Strategic Account discounts\n\nStrategic accounts get 15%.\n",
        )
        .unwrap();
        index(&mut incremental, &opts).unwrap();

        // Build the same final state from scratch.
        let mut full = Store::in_memory().unwrap();
        index(&mut full, &opts).unwrap();

        let a = incremental.canonical_dump().unwrap();
        let b = full.canonical_dump().unwrap();
        if a != b {
            let only_incremental: Vec<&str> =
                a.lines().filter(|l| !b.contains(*l)).take(8).collect();
            let only_full: Vec<&str> = b.lines().filter(|l| !a.contains(*l)).take(8).collect();
            panic!(
                "incremental diverged from full\n  only incremental: {only_incremental:#?}\n  only full: {only_full:#?}"
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

    /// A fixture with a deliberately planted disagreement between a document and the
    /// code that implements it.
    fn conflict_fixture(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "reify-conflict-{}-{name}-{}",
            std::process::id(),
            now_unix()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("app")).unwrap();
        fs::create_dir_all(dir.join("docs")).unwrap();
        fs::write(
            dir.join("docs/BRD-42.md"),
            "# Approval\n\n## Corporate approval\n\nCorporate customers must require approval before an order is confirmed.\n",
        )
        .unwrap();
        fs::write(
            dir.join("app/order.py"),
            "class Order:\n    def check(self):\n        if self.corporate_customers:\n            return self.bypass_approval()\n        return True\n",
        )
        .unwrap();
        dir
    }

    #[test]
    fn a_documented_rule_contradicting_the_code_is_detected_end_to_end() {
        let root = conflict_fixture("detect");
        let (store, report) = index_fresh(&root);
        assert!(report.rules >= 2, "both sides must be mined: {report:?}");
        assert_eq!(
            report.conflicts, 1,
            "the planted contradiction must be found"
        );

        let conflict = store
            .nodes_of_kind(NodeKind::BusinessRule)
            .unwrap()
            .into_iter()
            .find(|n| n.uid.starts_with("conflict:"))
            .expect("a conflict node must be staged");
        assert_eq!(conflict.status, Status::Conflicted);
        assert_eq!(conflict.data["resolution"], "UNRESOLVED");
        assert!(conflict.data["documented"]
            .as_str()
            .unwrap()
            .contains("require"));
        assert!(conflict.data["observed"]
            .as_str()
            .unwrap()
            .contains("bypass"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_repository_that_agrees_with_itself_reports_no_conflicts() {
        // The expensive failure mode is a false positive, so this is the test that
        // matters most for the feature being trusted.
        let root = fixture("agrees");
        let (_, report) = index_fresh(&root);
        assert_eq!(
            report.conflicts, 0,
            "a consistent repository must stay silent"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn removing_the_contradicting_document_clears_the_conflict() {
        // Exercises the global rebuild: a per-file upsert could never lower a rule's
        // confidence or retract a conflict.
        let root = conflict_fixture("clears");
        let mut store = Store::in_memory().unwrap();
        let opts = IndexOptions::new(&root);
        assert_eq!(index(&mut store, &opts).unwrap().conflicts, 1);

        fs::remove_file(root.join("docs/BRD-42.md")).unwrap();
        assert_eq!(index(&mut store, &opts).unwrap().conflicts, 0);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn incremental_indexing_equals_a_full_rebuild_with_rules_in_play() {
        let root = conflict_fixture("equiv");
        let opts = IndexOptions::new(&root);

        let mut incremental = Store::in_memory().unwrap();
        index(&mut incremental, &opts).unwrap();
        fs::write(
            root.join("app/order.py"),
            "class Order:\n    def check(self):\n        if self.corporate_customers:\n            return self.bypass_approval()\n        return False\n",
        )
        .unwrap();
        index(&mut incremental, &opts).unwrap();
        fs::write(
            root.join("docs/BRD-43.md"),
            "# Discounts\n\n## Strategic discount\n\nStrategic accounts must receive a discount on every order.\n",
        )
        .unwrap();
        index(&mut incremental, &opts).unwrap();

        let mut full = Store::in_memory().unwrap();
        index(&mut full, &opts).unwrap();

        assert_eq!(
            incremental.canonical_dump().unwrap(),
            full.canonical_dump().unwrap()
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn deleting_a_file_removes_everything_it_contributed() {
        let root = fixture("delete");
        let mut store = Store::in_memory().unwrap();
        let opts = IndexOptions::new(&root);
        index(&mut store, &opts).unwrap();
        assert!(store
            .node_by_uid("sym:app/strategic.py#StrategicAccount")
            .unwrap()
            .is_some());

        fs::remove_file(root.join("app/strategic.py")).unwrap();
        let report = index(&mut store, &opts).unwrap();
        assert_eq!(report.files_removed, 1);
        assert!(store
            .node_by_uid("sym:app/strategic.py#StrategicAccount")
            .unwrap()
            .is_none());
        assert!(store
            .node_by_uid("file:app/strategic.py")
            .unwrap()
            .is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_file_that_fails_to_parse_is_reported_and_does_not_stop_the_index() {
        let root = fixture("badfile");
        // Valid UTF-8, but not valid Python in a way tree-sitter still recovers from.
        fs::write(
            root.join("app/broken.py"),
            "def ok():\n    pass\n\nclass !!!:\n",
        )
        .unwrap();
        let (store, report) = index_fresh(&root);
        assert!(store.node_by_uid("sym:app/broken.py#ok").unwrap().is_some());
        assert!(report.symbols > 0);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn relative_imports_resolve_to_files_in_the_repository() {
        let present: HashSet<&str> = ["app/order.py", "app/util/helpers.py"]
            .into_iter()
            .collect();
        assert_eq!(
            resolve_module(".order", "app/main.py", &present).as_deref(),
            Some("app/order.py")
        );
        assert_eq!(
            resolve_module("app.util.helpers", "app/main.py", &present).as_deref(),
            Some("app/util/helpers.py")
        );
        assert_eq!(resolve_module("os", "app/main.py", &present), None);
    }

    #[test]
    fn the_innermost_symbol_owns_an_embedded_query() {
        let spans = vec![
            (1, 100, "sym:a.py#Outer".to_string()),
            (10, 20, "sym:a.py#Outer.inner".to_string()),
        ];
        assert_eq!(
            owner_for_line(&spans, 15).as_deref(),
            Some("sym:a.py#Outer.inner")
        );
        assert_eq!(
            owner_for_line(&spans, 50).as_deref(),
            Some("sym:a.py#Outer")
        );
        assert_eq!(owner_for_line(&spans, 500), None);
    }
}

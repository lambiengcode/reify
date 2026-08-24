//! Context compilation: a task in, the smallest useful system knowledge out.
//!
//! This is the product. Everything else in the crate exists to make this command
//! cheap, deterministic and citable.
//!
//! The output is deliberately *assertions with coordinates*, never pasted file
//! contents. An agent that receives "rule R, evidence `order.py:812`" reads fifty-eight
//! lines; an agent that receives the file reads two thousand. `next_reads` turns the
//! answer into a reading plan, which is where the token saving actually comes from.
//!
//! Four stages: seed, spread, select, render.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};

use crate::concepts::meaningful_words;
use crate::model::{EdgeKind, Node, NodeKind, Status};
use crate::store::{Direction, Store};
use crate::tokens;

/// Default budget. Chosen to sit far below what an agent would spend exploring.
///
/// The budget governs the **whole** cost of following the answer: the context output
/// plus every span it recommends reading. Budgeting only the output would be a lie by
/// omission — an answer costing 1,400 tokens that tells the agent to read 20,000 more
/// has not reduced anything.
pub const DEFAULT_BUDGET: u32 = 4_000;

/// Share of the total budget the compiled context itself may use.
///
/// The remainder funds the reading plan. Weighted toward the reads because the answer
/// is a map, not the territory: its job is to buy precise reads, not to replace them.
const CONTEXT_SHARE: f32 = 0.4;

/// How far relevance spreads from a seed. Beyond two hops nearly everything in a
/// mature repository is reachable, so distance stops discriminating.
const MAX_HOPS: u32 = 2;
/// Relevance retained per hop.
const HOP_DECAY: f32 = 0.55;
/// Below this, a node is not worth a line of an agent's context.
const MIN_SCORE: f32 = 0.02;
/// Seeds taken from the lexical index before spreading.
const LEXICAL_SEEDS: usize = 60;
/// Cap on the reading plan.
const MAX_NEXT_READS: usize = 6;
/// Symbol slots any one file may claim.
///
/// Relevance spreads along edges, so every member of a file that matched loosely
/// arrives holding a plausible score. Without this cap a single such file takes the
/// whole symbol budget: measured on Medusa, one HTTP router and one arithmetic helper
/// between them held 13 of 20 slots for a task about discounts, and the promotion
/// service that actually had to change ranked eighteenth. Four is enough to show that
/// several signals agree on a file, and low enough to leave room for five others.
const MAX_SYMBOLS_PER_FILE: usize = 4;
/// Rough tokens per line of source, for estimating the cost of a recommended read.
const TOKENS_PER_LINE: u32 = 10;

/// How much a directory path agreeing with the task lifts everything inside it.
const PATH_AFFINITY_WEIGHT: f32 = 1.0;

/// Floor of the question-coverage factor in seed scoring.
const COVERAGE_FLOOR: f32 = 0.35;

/// Score a test, fixture or mock gives up to the implementation it exercises.
///
/// Half, not all: enough to put the source above its own test in every ordering
/// measured, small enough that a task genuinely about a test still reaches it.
const TEST_PATH_PENALTY: f32 = 0.5;

/// Concepts whose other surface forms get searched too.
const CONCEPT_EXPANSIONS: usize = 3;

/// Tokens in a task that look like identifiers rather than prose.
///
/// A token containing an underscore, interior capitals, or a dot-path is something
/// the user copied out of the code, and deserves an exact lookup in one piece.
fn identifier_tokens(task: &str) -> Vec<String> {
    task.split_whitespace()
        .map(|t| t.trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '.'))
        .filter(|t| t.len() >= 4)
        .flat_map(|t| {
            // `PackedItem.update_info` names two things worth looking up.
            let mut out: Vec<String> = t.split('.').map(str::to_string).collect();
            if out.len() > 1 {
                out.push(t.to_string());
            }
            out
        })
        .filter(|t| {
            t.contains('_')
                || (t.chars().any(|c| c.is_lowercase())
                    && t.chars().skip(1).any(|c| c.is_uppercase()))
        })
        .collect()
}

/// Seeded files whose symbols get lifted, and how many symbols each may lift.
const FILE_FANOUT: usize = 6;
const FILE_FANOUT_SYMBOLS: usize = 8;
/// A symbol inherits this share of its file's seed score.
const FILE_TO_SYMBOL: f32 = 0.8;

/// A file whose last interesting line falls within this many lines is offered whole
/// rather than as regions: fragmenting a short file buys nothing and costs coherence.
const WHOLE_FILE_LINES: u32 = 400;

/// How many more entries an edit plan may hold than a reading plan, because each
/// region is a fraction of a file rather than a whole one.
const EDIT_PLAN_WIDTH: usize = 4;

/// Lines of scaffolding kept above a region so an edit lands in visible context:
/// the decorator, signature and doc comment that precede a definition.
const EDIT_LEAD_LINES: u32 = 12;
/// Lines kept below, so a closing brace or the next signature is visible.
const EDIT_TRAIL_LINES: u32 = 4;
/// Header of a file — imports, module docstring — included with any region in it,
/// because a patch that adds a call needs to know what is already imported.
const EDIT_HEADER_LINES: u32 = 40;

/// Weight of the bounded content scan's file seeds; zero disables the scan.
const LEXICAL_FILES_WEIGHT: f32 = 0.6;
/// Files the content scan may seed per query.
const LEXICAL_FILE_SEEDS: usize = 12;
/// How much of one file the content scan reads. Signal for this purpose lives in
/// string literals, labels and comments near the top; a megabyte of generated code
/// past this point is cost without evidence.
const CONTENT_SCAN_MAX_BYTES: u64 = 64_000;

/// Default share of the top offered file's score below which files are not offered.
///
/// 0.2 by the training-only rule "the largest cutoff that costs zero hit rate":
/// on both training corpora it trimmed the weakest offers without dropping a single
/// found task (`/tmp` fit surface committed as `benchmarks/weights/`). Raising it
/// buys precision at the price of recall — 0.45 roughly doubles precision and costs
/// two to three tasks in forty — which is the caller's trade to make, not a default.
const OFFER_CUTOFF: f32 = 0.2;
/// A hit found through an expansion is worth this much of the concept that found it.
const CONCEPT_EXPANSION_DECAY: f32 = 0.5;

/// Weight of the repository's own history as a retrieval signal.
///
/// The prior is independent evidence — commits that used the task's words touched
/// these files — so it *adds* to a node's score rather than competing under max().
///
/// 0.9 — the pre-fit default, kept after the fit **failed validation**.
///
/// `reify-bench fit` preferred 2.2–5.5 on training tasks (commits before the
/// benchmark base; `benchmarks/weights/fit-20260821.json`), and every value in that
/// range scored *worse* than 0.9 on the held-out frozen tasks. That is the fit's own
/// pre-registered falsification clause firing: the association between commit
/// vocabulary and files is real but nonstationary, and a weight tuned on one window
/// overshoots the next. Per the clause, the result is published and the default
/// reverts to the value chosen before any evaluation was seen. Do not raise this from
/// training evidence alone.
const HISTORY_PRIOR_WEIGHT: f32 = 0.9;
/// Files taken from the prior per query.
const HISTORY_PRIOR_FILES: usize = 8;
/// Symbols boosted per prior file. Fitted alongside [`HISTORY_PRIOR_WEIGHT`]: six,
/// so a single strong prior file cannot spend the symbol quota before the second-best
/// file — which is sometimes the right one — gets a seat.
const HISTORY_PRIOR_SYMBOLS_PER_FILE: usize = 6;

/// How many items of each kind a compiled context may contain.
///
/// A token share alone does not shape the answer, because the cheapest kinds are the
/// ones that over-fill it: a concept name costs about eight tokens, so twenty percent
/// of a four-thousand-token budget is a hundred concepts and no code. The count is the
/// binding constraint; the token share stops any one kind monopolising a small budget.
///
/// These numbers encode a claim about usefulness: past roughly this many items of one
/// kind, an agent is reading a list rather than an answer.
fn max_items(kind: NodeKind) -> usize {
    match kind {
        NodeKind::Concept => 6,
        NodeKind::DocSection => 5,
        NodeKind::BusinessRule => 6,
        NodeKind::Commit => 4,
        NodeKind::DatabaseObject => 6,
        NodeKind::Symbol => 20,
        NodeKind::File => 0,
    }
}

/// Share of the budget any one kind may claim in the first pass.
fn budget_share(kind: NodeKind) -> f32 {
    match kind {
        NodeKind::Concept => 0.20,
        NodeKind::DocSection => 0.30,
        NodeKind::BusinessRule => 0.25,
        NodeKind::Commit => 0.10,
        NodeKind::DatabaseObject => 0.15,
        NodeKind::Symbol => 0.60,
        NodeKind::File => 0.05,
    }
}

/// The tunable half of the ranking function.
///
/// These were hand-picked numbers defended by comments until `reify-bench fit`
/// existed; now they are parameters so the repository's own history can choose them.
/// The defaults are the fitted values — provenance in `benchmarks/weights/` — and the
/// fit must only ever run against commits *earlier* than any benchmark task, because
/// fitting on the evaluation set is the easiest way to fake a benchmark.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RankWeights {
    /// Weight of the repository's-own-history prior.
    pub history_prior: f32,
    /// Symbols boosted per prior file.
    pub history_symbols_per_file: usize,
    /// Floor of the question-coverage factor: how much a candidate scores when it
    /// matches the task lexically but names none of the task's words itself.
    pub coverage_floor: f32,
    /// How much a directory path agreeing with the task lifts everything inside it.
    pub path_affinity: f32,
    /// Strength of concept-driven query expansion; zero disables it.
    pub concept_expansion: f32,
    /// Offered files below this share of the top file's score are dropped.
    ///
    /// This is the precision knob: a task whose vocabulary matches nothing produces a
    /// tail of weak candidates, and offering seven wrong files is worse than offering
    /// two plausible ones plus an honest "unknown". Zero disables the cutoff.
    pub offer_cutoff: f32,
    /// Weight of the bounded content scan's file seeds; zero disables the scan.
    pub lexical_files: f32,
    /// Seeded files whose symbols get lifted, per fan-out pass.
    pub file_fanout: usize,
    /// Symbols each fanned file may lift.
    pub fanout_symbols: usize,
    /// Share of a file's score its symbols inherit through fan-out.
    pub file_to_symbol: f32,
    /// Score a test, fixture or mock gives up to the implementation it exercises;
    /// zero disables the penalty, one hides tests entirely.
    pub test_path_penalty: f32,
}

impl Default for RankWeights {
    fn default() -> Self {
        RankWeights {
            history_prior: HISTORY_PRIOR_WEIGHT,
            history_symbols_per_file: HISTORY_PRIOR_SYMBOLS_PER_FILE,
            coverage_floor: COVERAGE_FLOOR,
            path_affinity: PATH_AFFINITY_WEIGHT,
            concept_expansion: CONCEPT_EXPANSION_DECAY,
            offer_cutoff: OFFER_CUTOFF,
            lexical_files: LEXICAL_FILES_WEIGHT,
            file_fanout: FILE_FANOUT,
            fanout_symbols: FILE_FANOUT_SYMBOLS,
            file_to_symbol: FILE_TO_SYMBOL,
            test_path_penalty: TEST_PATH_PENALTY,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContextOptions {
    /// Emit regions sized to be *edited* rather than spans sized to be *read*.
    ///
    /// Reading and editing want different things. A reader wants the smallest span
    /// that answers the question; a model writing a patch needs the whole enclosing
    /// definition, the imports above it, and enough neighbouring code to match the
    /// file's conventions — otherwise it patches blind. Measured on SWE-bench
    /// Verified, feeding whole files instead cost more than it bought: the window
    /// filled with one large file and the file that mattered never arrived.
    pub for_edit: bool,
    pub budget: u32,
    pub max_next_reads: usize,
    pub weights: RankWeights,
    /// Files the agent has already read or ruled out.
    ///
    /// This is the whole iteration mechanism: Reify stays a pure function and the
    /// agent carries the state. A second call excluding the first answer's files *is*
    /// the refinement, with nothing to persist and nothing to invalidate.
    pub exclude: Vec<String>,
}

impl Default for ContextOptions {
    fn default() -> Self {
        ContextOptions {
            for_edit: false,
            budget: DEFAULT_BUDGET,
            max_next_reads: MAX_NEXT_READS,
            weights: RankWeights::default(),
            exclude: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BudgetInfo {
    /// The total an agent may spend following this answer.
    pub requested: u32,
    /// Tokens the context output itself costs.
    pub context: u32,
    /// Tokens the recommended reads will cost.
    pub reads: u32,
    /// `context + reads`. Never exceeds `requested` except for conflicts, which are
    /// admitted regardless because a known contradiction must never be dropped.
    pub used: u32,
    pub unit: &'static str,
    /// Named so a reported number can be traced to how it was counted.
    pub estimator: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConceptOut {
    pub id: String,
    pub status: Status,
    pub labels: serde_json::Value,
    pub code: serde_json::Value,
    pub db: serde_json::Value,
    pub bridge: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodeOut {
    pub path: String,
    pub symbol: String,
    pub lines: String,
    /// The relationship that earned this symbol its place, in checkable words.
    pub why: String,
    pub status: Status,
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocOut {
    pub location: String,
    pub document: String,
    pub section: String,
    pub lang: Option<String>,
    pub excerpt: String,
    pub status: Status,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataOut {
    pub table: String,
    pub why: String,
    pub status: Status,
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoryOut {
    pub commit: String,
    pub date: String,
    pub subject: String,
    pub class: String,
    pub why_relevant: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleOut {
    pub id: String,
    pub status: Status,
    pub confidence: f32,
    pub claim: String,
    pub subject: String,
    pub source: String,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConflictOut {
    pub id: String,
    pub status: Status,
    pub subject: String,
    pub documented: String,
    pub documented_at: String,
    pub observed: String,
    pub observed_at: String,
    pub resolution: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NextRead {
    pub path: String,
    pub lines: String,
    pub est_tokens: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Context {
    pub schema: &'static str,
    pub task: String,
    pub budget: BudgetInfo,
    pub concepts: Vec<ConceptOut>,
    pub rules: Vec<RuleOut>,
    pub code: Vec<CodeOut>,
    pub documents: Vec<DocOut>,
    pub data: Vec<DataOut>,
    pub history: Vec<HistoryOut>,
    /// Known contradictions touching this task. Never dropped for budget reasons.
    pub conflicts: Vec<ConflictOut>,
    /// What Reify could not determine. Populated deliberately, so an agent does not
    /// read absence as evidence of absence.
    pub unknowns: Vec<String>,
    /// A reading plan: the precise spans worth opening next.
    pub next_reads: Vec<NextRead>,
}

/// One node with the score and the reason it earned.
#[derive(Debug, Clone)]
struct Scored {
    node: Node,
    score: f32,
    reason: String,
}

/// Compile the minimum useful context for `task`.
pub fn compile(store: &Store, task: &str, opts: &ContextOptions) -> Result<Context> {
    let mut scored = rank(store, task, &opts.weights)?;
    if !opts.exclude.is_empty() {
        // Dropped before selection, not after: the budget freed by an excluded file
        // must go to the next-best candidate, or iteration returns thinner answers
        // instead of different ones.
        scored.retain(|s| {
            s.node
                .path
                .as_deref()
                .is_none_or(|path| !opts.exclude.iter().any(|e| e == path))
        });
    }
    let context_budget = (opts.budget as f32 * CONTEXT_SHARE) as u32;
    let selected = select(&scored, context_budget);
    let context_cost: u32 = selected.iter().map(|s| s.node.tokens).sum();

    let mut context = Context {
        schema: "reify.context/1",
        task: task.to_string(),
        budget: BudgetInfo {
            requested: opts.budget,
            context: context_cost,
            reads: 0,
            used: context_cost,
            unit: "tokens",
            estimator: tokens::ESTIMATOR,
        },
        concepts: Vec::new(),
        rules: Vec::new(),
        code: Vec::new(),
        documents: Vec::new(),
        data: Vec::new(),
        history: Vec::new(),
        conflicts: Vec::new(),
        unknowns: Vec::new(),
        next_reads: Vec::new(),
    };

    // Symbols render in ranked-file order, so the code list, the reading plan and the
    // offered files all agree about which file comes first — the agent reads them in
    // presentation order, and MRR is measured on it.
    for (_, items) in ranked_files(&selected, opts.weights.offer_cutoff) {
        for item in items {
            context.code.push(code_out(&item.node, &item.reason));
        }
    }
    for item in &selected {
        match item.node.kind {
            NodeKind::Concept => context.concepts.push(concept_out(&item.node)),
            NodeKind::Symbol => {}
            NodeKind::DocSection => context.documents.push(doc_out(&item.node)),
            NodeKind::DatabaseObject => context.data.push(DataOut {
                table: item.node.name.clone(),
                why: item.reason.clone(),
                status: item.node.status,
            }),
            NodeKind::Commit => context.history.push(history_out(&item.node, &item.reason)),
            NodeKind::BusinessRule => match conflict_out(&item.node) {
                Some(conflict) => context.conflicts.push(conflict),
                None => context.rules.push(rule_out(store, &item.node)?),
            },
            NodeKind::File => {}
        }
    }

    // Whatever the context did not spend funds the reading plan.
    let read_budget = opts.budget.saturating_sub(context_cost);
    context.next_reads = if opts.for_edit {
        // Each region costs a fraction of a whole file, so the same budget reaches
        // far more files. Capping at `max_next_reads` here would throw that away.
        edit_plan(
            &selected,
            opts.max_next_reads * EDIT_PLAN_WIDTH,
            read_budget,
            opts.weights.offer_cutoff,
        )
    } else {
        reading_plan(
            &selected,
            opts.max_next_reads,
            read_budget,
            opts.weights.offer_cutoff,
        )
    };
    context.budget.reads = context.next_reads.iter().map(|r| r.est_tokens).sum();
    context.budget.used = context.budget.context + context.budget.reads;

    if context.concepts.is_empty() {
        context
            .unknowns
            .push("no business concept matched this task".into());
    }
    if context.documents.is_empty() {
        context
            .unknowns
            .push("no document section describes this task".into());
    }
    if context.code.is_empty() {
        context
            .unknowns
            .push("no code matched this task; the index may not cover it".into());
    }
    Ok(context)
}

/// Seed from lexical and exact-name matches, then spread across the graph.
fn rank(store: &Store, task: &str, weights: &RankWeights) -> Result<Vec<Scored>> {
    let mut scores: HashMap<i64, (f32, String)> = HashMap::new();
    let mut nodes: HashMap<i64, Node> = HashMap::new();

    // --- seeds ---------------------------------------------------------------
    let asked = meaningful_words(task);
    let lexical = store.search(task, LEXICAL_SEEDS)?;
    let best = lexical
        .iter()
        .map(|(_, score)| *score)
        .fold(f32::MIN, f32::max)
        .max(1e-6);
    for (node, raw) in lexical {
        // bm25 is unbounded, so it is normalised against the best hit for this query
        // rather than used directly; only the ordering it expresses is meaningful.
        let relevance = (raw / best).clamp(0.0, 1.0);
        // How much of the question this candidate actually answers. bm25 rewards
        // repeating one term; a task names several things and the candidate that
        // mentions more of them is the better answer, so coverage is scored directly.
        let coverage = term_coverage(&node, &asked);
        // In a repository organised by domain — and mature business systems almost
        // always are — the path is itself evidence. A symbol living under
        // `.../timesheet_billing_summary/` is about timesheet billing summaries no
        // matter what the symbol is called, so path agreement lifts everything in it.
        let affinity = path_affinity(&node, &asked);
        let score = relevance
            * seed_weight(node.kind)
            * node.confidence
            * (weights.coverage_floor + (1.0 - weights.coverage_floor) * coverage)
            * (1.0 + weights.path_affinity * affinity);
        bump(
            &mut scores,
            &mut nodes,
            node,
            score,
            "matches the task text",
        );
    }

    // Exact identifier hits beat anything lexical scoring can express: naming a symbol
    // is a statement of intent, not a coincidence of vocabulary.
    //
    // Whole tokens are looked up *before* word-splitting, because splitting is exactly
    // what destroys them: a task quoting `update_packed_item_with_pick_list_info`
    // reduces to a handful of stopword-filtered fragments, and the one perfect signal
    // in the sentence — the user typed the symbol's name — is gone.
    for token in identifier_tokens(task) {
        for node in store.symbols_named(&token)? {
            bump(
                &mut scores,
                &mut nodes,
                node,
                1.2,
                "named verbatim in the task",
            );
        }
    }
    for word in meaningful_words(task) {
        for node in store.symbols_named(&word)? {
            bump(&mut scores, &mut nodes, node, 0.9, "named in the task");
        }
    }

    // --- the repository's own text as a seed source ---------------------------
    // FTS covers what extraction chose to index: names, signatures, summaries. A
    // task's words often live only in a file's *body* — a JSX label, a string
    // literal, a comment — which is exactly the signal a plain grep retrieves and
    // the four-repo union analysis showed reify losing tasks to. So reify runs its
    // own bounded content scan as one more seed source and lets selection arbitrate.
    if weights.lexical_files > 0.0 {
        if let Some(root) = store.repo_root() {
            for (path, strength) in content_seed_files(&root, &asked, LEXICAL_FILE_SEEDS) {
                if let Some(file) = store.node_by_uid(&crate::model::uid::file(&path))? {
                    let reason = format!("{path} mentions the task's words");
                    bump(
                        &mut scores,
                        &mut nodes,
                        file,
                        weights.lexical_files * strength,
                        &reason,
                    );
                }
            }
        }
    }

    // --- expansion through concepts ------------------------------------------
    // The concept layer exists because the analyst's word and the code's word differ.
    // Applying it only *after* seeding leaves the lexical search blind to every other
    // surface form, so the strongest seeded concepts contribute one extra search each:
    // a task that says "approval" also searches "phê duyệt", "approval_status" and
    // whatever else the concept knows itself as.
    let mut top_concepts: Vec<(i64, f32)> = if weights.concept_expansion <= 0.0 {
        Vec::new()
    } else {
        scores
            .iter()
            .filter(|(id, _)| nodes.get(id).is_some_and(|n| n.kind == NodeKind::Concept))
            .map(|(id, (score, _))| (*id, *score))
            .collect()
    };
    top_concepts.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    top_concepts.truncate(CONCEPT_EXPANSIONS);
    for (concept_id, concept_score) in top_concepts {
        let Some(concept) = nodes.get(&concept_id).cloned() else {
            continue;
        };
        let mut surface = String::new();
        if let Some(labels) = concept.data.get("labels").and_then(|v| v.as_object()) {
            for label in labels.values().filter_map(|v| v.as_str()) {
                surface.push_str(label);
                surface.push(' ');
            }
        }
        if let Some(code) = concept.data.get("code").and_then(|v| v.as_array()) {
            for identifier in code.iter().filter_map(|v| v.as_str()) {
                surface.push_str(identifier);
                surface.push(' ');
            }
        }
        // Only the forms the task did not already say; searching the task's own words
        // again would just re-rank the same seeds.
        let novel: Vec<String> = meaningful_words(&surface)
            .into_iter()
            .filter(|w| !asked.contains(w))
            .collect();
        if novel.is_empty() {
            continue;
        }
        let query = novel.into_iter().collect::<Vec<_>>().join(" ");
        for (node, raw) in store.search(&query, LEXICAL_SEEDS / 4)? {
            let relevance = (raw / best).clamp(0.0, 1.0);
            let score = concept_score * weights.concept_expansion * relevance;
            if score < MIN_SCORE {
                continue;
            }
            let reason = format!("other name for {}", concept.name);
            bump(&mut scores, &mut nodes, node, score, &reason);
        }
    }

    // --- the repository's own history as a prior -----------------------------
    // Every merged commit is a labelled example: its message is a ticket description
    // and its files are the correct answer. This is the signal no static analysis has,
    // and it is fully citable — N commits said these words and touched this file.
    let asked_vec: Vec<String> = asked
        .iter()
        .map(|w| crate::concepts::stem(w).to_string())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let prior = store.history_prior(&asked_vec, HISTORY_PRIOR_FILES)?;
    if let Some((_, strongest)) = prior.first().map(|(p, s)| (p.clone(), *s)) {
        for (path, raw) in &prior {
            let strength = weights.history_prior * (raw / strongest.max(1e-6));
            let reason = format!("past commits about this touched {path}");
            for symbol in store
                .symbols_in_file(path)?
                .into_iter()
                .take(weights.history_symbols_per_file)
            {
                boost(&mut scores, &mut nodes, symbol, strength * 0.5, &reason);
            }
            if let Some(file) = store.node_by_uid(&crate::model::uid::file(path))? {
                boost(&mut scores, &mut nodes, file, strength, &reason);
            }
        }
    }

    // --- files lift their contents -------------------------------------------
    // A file whose *path* matches the task is often the answer — `cloud-auth-login.tsx`
    // for "remove the cloud auth button" — but a file node has no outgoing edges to
    // its symbols and is not itself rendered, so without this step a perfectly
    // matching file contributes nothing at all to the answer.
    let mut fanned: BTreeSet<i64> = BTreeSet::new();
    fan_out_files(store, &mut scores, &mut nodes, &mut fanned, weights)?;

    // --- spread --------------------------------------------------------------
    let mut frontier: Vec<i64> = scores.keys().copied().collect();
    frontier.sort_unstable();
    for hop in 1..=MAX_HOPS {
        let mut next: Vec<i64> = Vec::new();
        for id in std::mem::take(&mut frontier) {
            let Some(parent_score) = scores.get(&id).map(|(score, _)| *score) else {
                continue;
            };
            let parent_name = nodes.get(&id).map(|n| n.name.clone()).unwrap_or_default();
            for direction in [Direction::Out, Direction::In] {
                for (neighbour, kind, confidence) in store.neighbors(id, direction, SPREAD_EDGES)? {
                    let score =
                        parent_score * kind.weight() * confidence * HOP_DECAY.powi(hop as i32 - 1);
                    if score < MIN_SCORE {
                        continue;
                    }
                    let reason = explain(kind, direction, &parent_name);
                    let id = neighbour.id;
                    if bump(&mut scores, &mut nodes, neighbour, score, &reason) && hop < MAX_HOPS {
                        next.push(id);
                    }
                }
            }
        }
        next.sort_unstable();
        next.dedup();
        frontier = next;
    }

    // --- files reached indirectly lift their contents too ---------------------
    // The first fan-out pass ran on seeds; by now the history prior, co-change
    // edges and the spread have scored *more* files — and a file node still renders
    // nothing itself. Without this second pass, "these two files always change
    // together" scores the neighbour file and then throws that evidence away.
    fan_out_files(store, &mut scores, &mut nodes, &mut fanned, weights)?;

    let mut out: Vec<Scored> = scores
        .into_iter()
        .filter_map(|(id, (score, reason))| {
            nodes.remove(&id).map(|node| {
                // Applied here, once, rather than at seed time: a test is reached far
                // more often by spreading from the code it exercises than by matching
                // the task itself, so penalising only the seeds would miss most of them.
                let score = score * test_path_factor(&node, weights.test_path_penalty);
                Scored {
                    node,
                    score,
                    reason,
                }
            })
        })
        .filter(|s| s.score >= MIN_SCORE)
        .collect();
    // Ties broken by uid so the same store and query always produce the same context.
    out.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(a.node.uid.cmp(&b.node.uid))
    });
    Ok(out)
}

/// The share of the task's distinct domain words this node's own identity mentions.
///
/// Deliberately computed over the node's name, path and one-line summary rather than
/// its whole indexed body: a file that merely *contains* a word somewhere is not the
/// same as one that is *about* it.
fn term_coverage(node: &Node, asked: &BTreeSet<String>) -> f32 {
    if asked.is_empty() {
        return 1.0;
    }
    let mut identity = String::with_capacity(128);
    identity.push_str(&node.name);
    identity.push(' ');
    if let Some(path) = &node.path {
        identity.push_str(path);
        identity.push(' ');
    }
    for key in ["summary", "qualified", "document"] {
        if let Some(text) = node.data.get(key).and_then(|v| v.as_str()) {
            identity.push_str(text);
            identity.push(' ');
        }
    }
    let words = meaningful_words(&identity);
    let matched = asked
        .iter()
        .filter(|w| words.contains(*w) || words.iter().any(|c| stem_match(c, w)))
        .count();
    matched as f32 / asked.len() as f32
}

/// The share of the task's words the node's *directory path* mentions.
///
/// Computed on the path alone, and on directories rather than the file stem, because
/// the question it answers is "is this the right area of the system", not "is this the
/// right function".
fn path_affinity(node: &Node, asked: &BTreeSet<String>) -> f32 {
    if asked.is_empty() {
        return 0.0;
    }
    let Some(path) = &node.path else {
        return 0.0;
    };
    let words = meaningful_words(path);
    let matched = asked
        .iter()
        .filter(|w| words.contains(*w) || words.iter().any(|c| stem_match(c, w)))
        .count();
    matched as f32 / asked.len() as f32
}

/// Match singular against plural. One shared definition, in `concepts`.
fn stem_match(candidate: &str, asked: &str) -> bool {
    crate::concepts::same_word(candidate, asked)
}

/// How much of its score a node keeps for living in a test, fixture or mock.
///
/// Tests are excellent evidence about *which* code matters and poor evidence about
/// *what to change*: a test names the domain vocabulary densely, so it scores well,
/// and then tells a reader nothing they can edit. Measured on Medusa, a promotion
/// spec and its fixture both outranked the promotion service they exercise.
///
/// A penalty rather than an exclusion, deliberately. "Fix the failing test for X" is a
/// real task, and a reproduction is genuinely the right place to start; the test
/// should lose to its implementation, not disappear behind it.
fn test_path_factor(node: &Node, penalty: f32) -> f32 {
    let Some(path) = &node.path else {
        return 1.0;
    };
    if is_test_path(path) {
        (1.0 - penalty).clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// Whether a path is test, fixture or mock material.
///
/// Segment-wise rather than substring, so `src/contest/` and `src/latest/` are not
/// mistaken for tests. Covers the conventions the indexed languages actually use.
fn is_test_path(path: &str) -> bool {
    path.split('/').any(|segment| {
        let s = segment.trim_matches('_').to_ascii_lowercase();
        let stem = s.split('.').next().unwrap_or("");
        matches!(
            s.as_str(),
            "test"
                | "tests"
                | "testing"
                | "spec"
                | "specs"
                | "fixture"
                | "fixtures"
                | "mock"
                | "mocks"
                | "e2e"
                | "testdata"
                | "integration-tests"
        ) || stem.ends_with("_test")
            || stem.ends_with("_spec")
            || stem.starts_with("test_")
            || s.contains(".test.")
            || s.contains(".spec.")
    })
}

/// Edge kinds relevance travels along. History edges are excluded from the general
/// spread and reached only through the file a selected symbol lives in, because
/// "changed in the same commit" is far weaker evidence than "calls".
const SPREAD_EDGES: &[EdgeKind] = &[
    EdgeKind::MapsTo,
    EdgeKind::DocumentedBy,
    EdgeKind::ImplementsRule,
    EdgeKind::Calls,
    EdgeKind::Reads,
    EdgeKind::Writes,
    EdgeKind::Inherits,
    EdgeKind::TestedBy,
    EdgeKind::Contradicts,
];

/// Add independent evidence to a node's score.
///
/// Distinct from [`bump`] on purpose: `bump` keeps the *strongest* single claim, which
/// is right when two paths derive the same fact. A prior is a different fact about the
/// same node, so it accumulates instead — and the reason string keeps whichever
/// contribution was larger, so the citation matches the dominant evidence.
fn boost(
    scores: &mut HashMap<i64, (f32, String)>,
    nodes: &mut HashMap<i64, Node>,
    node: Node,
    amount: f32,
    reason: &str,
) {
    let id = node.id;
    nodes.entry(id).or_insert(node);
    match scores.get_mut(&id) {
        Some((score, why)) => {
            if amount > *score {
                *why = reason.to_string();
            }
            *score += amount;
        }
        None => {
            scores.insert(id, (amount, reason.to_string()));
        }
    }
}

/// Record a score, keeping the strongest claim. Returns whether it was an improvement.
fn bump(
    scores: &mut HashMap<i64, (f32, String)>,
    nodes: &mut HashMap<i64, Node>,
    node: Node,
    score: f32,
    reason: &str,
) -> bool {
    let id = node.id;
    nodes.entry(id).or_insert(node);
    match scores.get_mut(&id) {
        Some(existing) if existing.0 >= score => false,
        Some(existing) => {
            *existing = (score, reason.to_string());
            true
        }
        None => {
            scores.insert(id, (score, reason.to_string()));
            true
        }
    }
}

fn explain(kind: EdgeKind, direction: Direction, parent: &str) -> String {
    let arrow = match (kind, direction) {
        (EdgeKind::MapsTo, Direction::Out) => "realises the concept",
        (EdgeKind::MapsTo, Direction::In) => "is named by the concept",
        (EdgeKind::Calls, Direction::Out) => "is called by",
        (EdgeKind::Calls, Direction::In) => "calls",
        (EdgeKind::Reads, Direction::Out) => "is read by",
        (EdgeKind::Reads, Direction::In) => "reads",
        (EdgeKind::Writes, Direction::Out) => "is written by",
        (EdgeKind::Writes, Direction::In) => "writes",
        (EdgeKind::DocumentedBy, Direction::Out) => "documents",
        (EdgeKind::DocumentedBy, Direction::In) => "is documented by",
        (EdgeKind::Inherits, Direction::Out) => "is a base of",
        (EdgeKind::Inherits, Direction::In) => "inherits from",
        (EdgeKind::TestedBy, _) => "is tested with",
        (EdgeKind::Contradicts, _) => "contradicts",
        _ => "is related to",
    };
    format!("{arrow} {parent}")
}

/// How much a seed of each kind is worth before spreading.
/// Lift the symbols of the highest-scoring file nodes that have not fanned yet.
///
/// Callable twice per query — once on the seeds, once after the spread — with
/// `fanned` preventing a file from spending the symbol quota twice.
fn fan_out_files(
    store: &Store,
    scores: &mut HashMap<i64, (f32, String)>,
    nodes: &mut HashMap<i64, Node>,
    fanned: &mut BTreeSet<i64>,
    weights: &RankWeights,
) -> Result<()> {
    let mut top_files: Vec<(i64, f32)> = scores
        .iter()
        .filter(|(id, _)| {
            !fanned.contains(*id) && nodes.get(id).is_some_and(|n| n.kind == NodeKind::File)
        })
        .map(|(id, (score, _))| (*id, *score))
        .collect();
    top_files.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    top_files.truncate(weights.file_fanout);
    for (file_id, file_score) in top_files {
        fanned.insert(file_id);
        let Some(path) = nodes.get(&file_id).and_then(|n| n.path.clone()) else {
            continue;
        };
        let reason = format!("in {path}, which matches the task");
        for symbol in store
            .symbols_in_file(&path)?
            .into_iter()
            .take(weights.fanout_symbols)
        {
            boost(
                scores,
                nodes,
                symbol,
                file_score * weights.file_to_symbol,
                &reason,
            );
        }
    }
    Ok(())
}

/// Files whose *contents* mention the task's words, scored by distinct-word count.
///
/// Bounded and parallel: code files only, the first 64KB of each. On a 5,000-file
/// repository the scan costs tens of milliseconds, and it buys the one signal FTS
/// structurally lacks — string literals, UI labels and comments that never became a
/// symbol name. Scores are normalised to 0..=1 against the best file.
fn content_seed_files(
    root: &std::path::Path,
    asked: &BTreeSet<String>,
    cap: usize,
) -> Vec<(String, f32)> {
    use rayon::prelude::*;
    if asked.is_empty() {
        return Vec::new();
    }
    let words: Vec<String> = asked.iter().map(|w| w.to_lowercase()).collect();
    let corpus = scan_corpus(root);
    let mut scored: Vec<(String, f32)> = corpus
        .par_iter()
        .filter_map(|(rel, text)| {
            let mut distinct = 0usize;
            let mut total = 0usize;
            for word in &words {
                let n = text.matches(word.as_str()).count();
                if n > 0 {
                    distinct += 1;
                    total += n.min(50);
                }
            }
            if distinct == 0 {
                return None;
            }
            Some((rel.clone(), distinct as f32 + total as f32 / 200.0))
        })
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    scored.truncate(cap);
    let best = scored.first().map(|(_, s)| *s).unwrap_or(1.0).max(1e-6);
    scored.into_iter().map(|(p, s)| (p, s / best)).collect()
}

/// The lowercased scan corpus for one repository root, walked once per process.
///
/// The walk dominates the scan's cost; a resident process (the MCP server, the
/// benchmark harness) asks about the same repository hundreds of times, and paying
/// the walk each time would turn a tens-of-milliseconds match into seconds. Memory is
/// bounded by [`CONTENT_SCAN_MAX_BYTES`] per file — the same order the benchmark's
/// own grep corpus already holds. A one-shot CLI process pays one walk, as before.
fn scan_corpus(root: &std::path::Path) -> std::sync::Arc<Vec<(String, String)>> {
    // (return type spelled out once here; the local alias below keeps clippy content)
    use std::collections::HashMap as Map;
    use std::sync::{Arc, Mutex, OnceLock};
    type Corpus = Arc<Vec<(String, String)>>;
    static CACHE: OnceLock<Mutex<Map<std::path::PathBuf, Corpus>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(Map::new()));
    if let Some(hit) = cache.lock().ok().and_then(|c| c.get(root).cloned()) {
        return hit;
    }
    use rayon::prelude::*;
    let files: Vec<std::path::PathBuf> = ignore::WalkBuilder::new(root)
        .build()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_some_and(|t| t.is_file()))
        .map(ignore::DirEntry::into_path)
        .filter(|path| crate::discover::classify(&path.to_string_lossy()).is_code())
        .collect();
    let corpus: Corpus = Arc::new(
        files
            .par_iter()
            .filter_map(|path| {
                let text = bounded_read(path)?.to_lowercase();
                let rel = path
                    .strip_prefix(root)
                    .ok()?
                    .to_string_lossy()
                    .replace('\\', "/");
                Some((rel, text))
            })
            .collect(),
    );
    if let Ok(mut c) = cache.lock() {
        c.insert(root.to_path_buf(), corpus.clone());
    }
    corpus
}

/// Read at most [`CONTENT_SCAN_MAX_BYTES`] of a file, lossily.
fn bounded_read(path: &std::path::Path) -> Option<String> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let mut buf = Vec::with_capacity(8 * 1024);
    file.take(CONTENT_SCAN_MAX_BYTES)
        .read_to_end(&mut buf)
        .ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

fn seed_weight(kind: NodeKind) -> f32 {
    match kind {
        NodeKind::Concept => 1.0,
        NodeKind::BusinessRule => 1.0,
        NodeKind::DocSection => 0.9,
        NodeKind::Symbol => 0.85,
        NodeKind::DatabaseObject => 0.7,
        NodeKind::File => 0.35,
        NodeKind::Commit => 0.3,
    }
}

/// The file a symbol lives in, for the per-file cap. `None` for everything else,
/// because concepts, rules and documents are already capped by count and by share.
fn symbol_file(item: &Scored) -> Option<String> {
    match item.node.kind {
        NodeKind::Symbol => item.node.path.clone(),
        _ => None,
    }
}

/// Fill the budget, best value per token first.
///
/// Three deliberate asymmetries. A `CONFLICTED` node is admitted regardless of budget,
/// because budget pressure may drop useful context but must never drop a known
/// contradiction. The single best concept and document are reserved, so a
/// symbol-heavy result never crowds out the two things that explain it. And no single
/// file may claim more than [`MAX_SYMBOLS_PER_FILE`] of the symbol slots, so one
/// loosely-matched file cannot spend the window on its own members.
fn select(scored: &[Scored], budget: u32) -> Vec<Scored> {
    let mut chosen: Vec<Scored> = Vec::new();
    let mut spent = 0u32;
    let mut taken: Vec<i64> = Vec::new();

    let admit = |item: &Scored, chosen: &mut Vec<Scored>, spent: &mut u32, taken: &mut Vec<i64>| {
        if taken.contains(&item.node.id) {
            return;
        }
        taken.push(item.node.id);
        *spent += item.node.tokens;
        chosen.push(item.clone());
    };

    for item in scored
        .iter()
        .filter(|s| s.node.status == Status::Conflicted)
    {
        admit(item, &mut chosen, &mut spent, &mut taken);
    }
    for kind in [NodeKind::Concept, NodeKind::DocSection] {
        if let Some(item) = scored.iter().find(|s| s.node.kind == kind) {
            admit(item, &mut chosen, &mut spent, &mut taken);
        }
    }

    let mut remaining: Vec<&Scored> = scored
        .iter()
        .filter(|s| !taken.contains(&s.node.id))
        .collect();
    remaining.sort_by(|a, b| {
        let ratio = |s: &Scored| s.score / s.node.tokens.max(1) as f32;
        ratio(b)
            .total_cmp(&ratio(a))
            .then(a.node.uid.cmp(&b.node.uid))
    });

    // Count what the reserved picks already used, so the caps bind on the total.
    let mut count_by_kind: HashMap<NodeKind, usize> = HashMap::new();
    let mut tokens_by_kind: HashMap<NodeKind, u32> = HashMap::new();
    let mut symbols_by_file: HashMap<String, usize> = HashMap::new();
    for item in &chosen {
        *count_by_kind.entry(item.node.kind).or_insert(0) += 1;
        *tokens_by_kind.entry(item.node.kind).or_insert(0) += item.node.tokens;
        if let Some(path) = symbol_file(item) {
            *symbols_by_file.entry(path).or_insert(0) += 1;
        }
    }

    // Best value per token, subject to every cap.
    for item in &remaining {
        let kind = item.node.kind;
        let token_cap = (budget as f32 * budget_share(kind)) as u32;
        let count = *count_by_kind.get(&kind).unwrap_or(&0);
        let used = *tokens_by_kind.get(&kind).unwrap_or(&0);
        let file = symbol_file(item);
        if spent + item.node.tokens > budget
            || count >= max_items(kind)
            || used + item.node.tokens > token_cap
            || file.as_ref().is_some_and(|p| {
                symbols_by_file
                    .get(p)
                    .is_some_and(|n| *n >= MAX_SYMBOLS_PER_FILE)
            })
        {
            continue;
        }
        *count_by_kind.entry(kind).or_insert(0) += 1;
        *tokens_by_kind.entry(kind).or_insert(0) += item.node.tokens;
        if let Some(path) = file {
            *symbols_by_file.entry(path).or_insert(0) += 1;
        }
        admit(item, &mut chosen, &mut spent, &mut taken);
    }
    // Spend budget the shares left unused on more code, which is what a change needs
    // — but never past the count cap, or the answer becomes a directory listing, and
    // never past the per-file cap, or it becomes one file's table of contents.
    for item in &remaining {
        let kind = item.node.kind;
        let file = symbol_file(item);
        if taken.contains(&item.node.id)
            || kind != NodeKind::Symbol
            || *count_by_kind.get(&kind).unwrap_or(&0) >= max_items(kind)
            || spent + item.node.tokens > budget
            || file.as_ref().is_some_and(|p| {
                symbols_by_file
                    .get(p)
                    .is_some_and(|n| *n >= MAX_SYMBOLS_PER_FILE)
            })
        {
            continue;
        }
        *count_by_kind.entry(kind).or_insert(0) += 1;
        if let Some(path) = file {
            *symbols_by_file.entry(path).or_insert(0) += 1;
        }
        admit(item, &mut chosen, &mut spent, &mut taken);
    }

    chosen.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(a.node.uid.cmp(&b.node.uid))
    });
    chosen
}

/// The spans an agent should open next, highest relevance first, within budget.
///
/// A span that does not fit is skipped rather than truncated, and a cheaper span
/// further down the list may still be taken — a 400-line class must not block six
/// precise 20-line methods.
///
/// Spans are drawn one file at a time in rounds rather than by draining each file in
/// turn. Draining spent the whole plan on the top file: measured on Medusa, two tasks
/// in three produced six entries naming a single file, so the plan pointed at one
/// place and called it a reading list. A round-robin gives every ranked file a span
/// before any file gets a second, which is what makes the plan a *plan*.
fn reading_plan(selected: &[Scored], limit: usize, budget: u32, cutoff: f32) -> Vec<NextRead> {
    // Ordered by *file aggregate* rather than by individual symbol score: three
    // moderately-scored symbols in one file are stronger evidence about that file
    // than one well-scored symbol elsewhere, and the agent opens files, not symbols.
    let ranked = ranked_files(selected, cutoff);

    let mut plan: Vec<NextRead> = Vec::new();
    let mut spent = 0u32;
    let deepest = ranked
        .iter()
        .map(|(_, items)| items.len())
        .max()
        .unwrap_or(0);
    for round in 0..deepest {
        for (path, items) in &ranked {
            if plan.len() >= limit {
                return plan;
            }
            let Some(item) = items.get(round) else {
                continue;
            };
            if item.node.line_start == 0 {
                continue;
            }
            let span = item.node.line_end.saturating_sub(item.node.line_start) + 1;
            let cost = span * TOKENS_PER_LINE;
            if spent + cost > budget {
                continue;
            }
            spent += cost;
            plan.push(NextRead {
                path: path.clone(),
                lines: format!("{}-{}", item.node.line_start, item.node.line_end),
                est_tokens: cost,
            });
        }
    }
    plan
}

/// A reading plan sized for *writing a patch* rather than for answering a question.
///
/// Three differences from [`reading_plan`], each measured rather than assumed:
///
/// 1. **Regions are padded to complete definitions.** A span that starts at the first
///    line of a function body leaves the model guessing at the signature it must
///    preserve; a few lines of lead and trail make the edit site self-contained.
/// 2. **Each file contributes its header once.** Imports decide whether a patch can
///    call something, and they are never inside the span that matched.
/// 3. **Overlapping regions in a file are merged**, so a file with three neighbouring
///    hits costs one contiguous read instead of three overlapping ones.
///
/// The budget is still hard: regions are taken in ranked-file order until it is spent,
/// which is what keeps a large file from swallowing the whole window.
fn edit_plan(selected: &[Scored], limit: usize, budget: u32, cutoff: f32) -> Vec<NextRead> {
    let ranked = ranked_files(selected, cutoff);
    let mut plan: Vec<NextRead> = Vec::new();
    let mut spent = 0u32;

    for (path, items) in &ranked {
        if plan.len() >= limit {
            break;
        }
        // Collect this file's regions, padded to whole definitions.
        let mut regions: Vec<(u32, u32)> = items
            .iter()
            .filter(|i| i.node.line_start > 0)
            .map(|i| {
                (
                    i.node.line_start.saturating_sub(EDIT_LEAD_LINES).max(1),
                    i.node.line_end + EDIT_TRAIL_LINES,
                )
            })
            .collect();
        if regions.is_empty() {
            continue;
        }
        regions.sort_unstable();
        // Merge overlaps and near-touching regions into one contiguous read.
        let mut merged: Vec<(u32, u32)> = Vec::new();
        for (start, end) in regions {
            match merged.last_mut() {
                Some((_, prev_end)) if start <= *prev_end + EDIT_LEAD_LINES => {
                    *prev_end = (*prev_end).max(end);
                }
                _ => merged.push((start, end)),
            }
        }
        // A file small enough to read whole is offered whole. Regions exist because
        // large files do not fit a finite window; below that size they only fragment
        // the file, and a model handed lines 120-210 and 245-300 will reason about
        // the gap it cannot see. One contiguous read is strictly better here.
        let span_end = merged.last().map(|(_, e)| *e).unwrap_or(0);
        if span_end <= WHOLE_FILE_LINES {
            let cost = span_end * TOKENS_PER_LINE;
            if spent + cost <= budget {
                spent += cost;
                plan.push(NextRead {
                    path: path.clone(),
                    lines: format!("1-{span_end}"),
                    est_tokens: cost,
                });
            }
            continue;
        }

        // A file earns its header only once its first region is affordable: imports
        // with no code beneath them are scaffolding around nothing, and paying for
        // several files' headers is exactly how a budget gets spent saying nothing.
        let (first_start, first_end) = merged[0];
        let first_cost = (first_end.saturating_sub(first_start) + 1) * TOKENS_PER_LINE;
        let header_end = EDIT_HEADER_LINES.min(first_start.saturating_sub(1));
        let header_cost = header_end * TOKENS_PER_LINE;
        if spent + first_cost + header_cost > budget {
            continue;
        }
        if header_end > 0 {
            spent += header_cost;
            plan.push(NextRead {
                path: path.clone(),
                lines: format!("1-{header_end}"),
                est_tokens: header_cost,
            });
        }
        for (start, end) in merged {
            if plan.len() >= limit {
                break;
            }
            let cost = (end.saturating_sub(start) + 1) * TOKENS_PER_LINE;
            if spent + cost > budget {
                continue;
            }
            spent += cost;
            plan.push(NextRead {
                path: path.clone(),
                lines: format!("{start}-{end}"),
                est_tokens: cost,
            });
        }
    }
    plan
}

/// Selected symbols grouped by file, files ordered by aggregate score, weak tail cut.
///
/// The aggregate is the sum of a file's top three symbol scores: enough to reward
/// several signals agreeing on one file, bounded so a file with twenty weak symbols
/// cannot outvote a file with one strong hit. Files whose aggregate falls below
/// `cutoff` of the best file's are dropped entirely — a weak tail of guesses costs
/// the reader more than an honest gap.
fn ranked_files(selected: &[Scored], cutoff: f32) -> Vec<(String, Vec<&Scored>)> {
    let mut by_file: Vec<(String, f32, Vec<&Scored>)> = Vec::new();
    for item in selected {
        if item.node.kind != NodeKind::Symbol {
            continue;
        }
        let Some(path) = &item.node.path else {
            continue;
        };
        match by_file.iter_mut().find(|(p, _, _)| p == path) {
            Some((_, aggregate, items)) => {
                if items.len() < 3 {
                    *aggregate += item.score;
                }
                items.push(item);
            }
            None => by_file.push((path.clone(), item.score, vec![item])),
        }
    }
    by_file.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    if let Some(best) = by_file.first().map(|(_, score, _)| *score) {
        by_file.retain(|(_, aggregate, _)| *aggregate >= best * cutoff);
    }
    by_file
        .into_iter()
        .map(|(path, _, items)| (path, items))
        .collect()
}

/// A rule node, with the evidence rows that justify it.
///
/// Evidence is loaded here rather than cached on the node because an `INFERRED` claim
/// without a checkable citation is exactly what an agent must not be handed.
fn rule_out(store: &Store, node: &Node) -> Result<RuleOut> {
    let get = |key: &str| {
        node.data
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    };
    let evidence = store
        .evidence_for(node.id)?
        .into_iter()
        .map(|(source, locator, _)| {
            if source == locator {
                source
            } else {
                format!("{source} {locator}")
            }
        })
        .collect();
    Ok(RuleOut {
        id: node.uid.clone(),
        status: node.status,
        confidence: node.confidence,
        claim: get("claim"),
        subject: get("subject"),
        source: get("source"),
        evidence,
    })
}

/// A conflict node, or `None` when the rule node is an ordinary claim.
fn conflict_out(node: &Node) -> Option<ConflictOut> {
    node.data.get("conflict")?.as_bool()?.then(|| {
        let get = |key: &str| {
            node.data
                .get(key)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string()
        };
        ConflictOut {
            id: node.uid.clone(),
            status: node.status,
            subject: node.name.clone(),
            documented: get("documented"),
            documented_at: get("documented_at"),
            observed: get("observed"),
            observed_at: get("observed_at"),
            resolution: get("resolution"),
        }
    })
}

fn concept_out(node: &Node) -> ConceptOut {
    let get = |key: &str| {
        node.data
            .get(key)
            .cloned()
            .unwrap_or(serde_json::Value::Null)
    };
    ConceptOut {
        id: node
            .data
            .get("concept_id")
            .and_then(|v| v.as_str())
            .unwrap_or(&node.name)
            .to_string(),
        status: node.status,
        labels: get("labels"),
        code: get("code"),
        db: get("db"),
        bridge: node
            .data
            .get("bridge")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string(),
    }
}

fn code_out(node: &Node, reason: &str) -> CodeOut {
    CodeOut {
        path: node.path.clone().unwrap_or_default(),
        symbol: node
            .data
            .get("qualified")
            .and_then(|v| v.as_str())
            .unwrap_or(&node.name)
            .to_string(),
        lines: format!("{}-{}", node.line_start, node.line_end),
        why: reason.to_string(),
        status: node.status,
        signature: node
            .data
            .get("signature")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    }
}

fn doc_out(node: &Node) -> DocOut {
    let get = |key: &str| {
        node.data
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    };
    DocOut {
        location: node.location(),
        document: get("document"),
        section: get("slug"),
        lang: node
            .data
            .get("lang")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        excerpt: get("excerpt"),
        status: node.status,
    }
}

fn history_out(node: &Node, reason: &str) -> HistoryOut {
    let get = |key: &str| {
        node.data
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    };
    HistoryOut {
        commit: node.name.clone(),
        date: get("date"),
        subject: get("subject"),
        class: get("class"),
        why_relevant: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{index, IndexOptions};

    #[test]
    fn edit_mode_returns_whole_definitions_with_the_file_header() {
        let (store, _root) = indexed();
        let task = "discount policy for strategic account orders";
        let plain = compile(&store, task, &ContextOptions::default()).unwrap();
        let edit = compile(
            &store,
            task,
            &ContextOptions {
                for_edit: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!edit.next_reads.is_empty(), "edit mode must offer regions");

        // A file's header is offered before any region inside it, because a patch
        // that adds a call needs to see what is already imported.
        let first = &edit.next_reads[0];
        assert!(first.lines.starts_with("1-"), "header first, got {first:?}");

        // Regions are padded relative to the reading plan's bare spans.
        let span_lines = |r: &NextRead| {
            let (a, b) = r.lines.split_once('-').unwrap();
            b.parse::<u32>().unwrap() - a.parse::<u32>().unwrap() + 1
        };
        let widest_read: u32 = plain.next_reads.iter().map(span_lines).max().unwrap_or(0);
        let widest_edit: u32 = edit.next_reads.iter().map(span_lines).max().unwrap_or(0);
        assert!(
            widest_edit >= widest_read,
            "edit regions ({widest_edit}) must not be narrower than read spans ({widest_read})"
        );

        // The budget is still hard in edit mode.
        let spent: u32 = edit.next_reads.iter().map(|r| r.est_tokens).sum();
        assert!(spent <= edit.budget.requested, "edit plan overspent");
    }

    #[test]
    fn rank_weights_deserialise_partially_with_defaults() {
        // An ablation file names only what it changes; everything else stays default.
        let weights: RankWeights = serde_json::from_str(r#"{"lexical_files": 0.0}"#).unwrap();
        assert_eq!(weights.lexical_files, 0.0);
        assert_eq!(weights.file_fanout, RankWeights::default().file_fanout);
        assert_eq!(weights.offer_cutoff, RankWeights::default().offer_cutoff);
    }

    #[test]
    fn content_scan_finds_words_that_live_only_in_file_bodies() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("fixtures/minierp");
        let asked: BTreeSet<String> = meaningful_words("corporate approval bypass");
        let seeds = content_seed_files(&root, &asked, 12);
        assert!(!seeds.is_empty(), "the fixture mentions these words");
        // Scores are normalised: the best file is exactly 1.0 and nothing exceeds it.
        assert!((seeds[0].1 - 1.0).abs() < 1e-6);
        assert!(seeds.iter().all(|(_, s)| *s <= 1.0 + 1e-6));
        // Paths come back repo-relative, so they can meet the store's file uids.
        assert!(seeds.iter().all(|(p, _)| !p.starts_with('/')));
    }
    use std::fs;
    use std::path::PathBuf;

    fn fixture() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "reify-ctx-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("app")).unwrap();
        fs::create_dir_all(dir.join("docs")).unwrap();
        fs::create_dir_all(dir.join("translations")).unwrap();

        fs::write(
            dir.join("app/discount.py"),
            r#"
class DiscountPolicy:
    """Computes the discount applied to an order."""

    def apply(self, order):
        if order.customer_group == 7:
            return self.strategic_rate(order)
        return 0.0

    def strategic_rate(self, order):
        return 0.15
"#,
        )
        .unwrap();
        fs::write(
            dir.join("app/strategic.py"),
            "class StrategicAccount:\n    \"\"\"An enterprise customer on the strategic tier.\"\"\"\n    def tier(self):\n        return 'S'\n",
        )
        .unwrap();
        fs::write(
            dir.join("app/unrelated.py"),
            "class TimesheetImporter:\n    def parse_row(self):\n        return None\n",
        )
        .unwrap();
        fs::write(
            dir.join("docs/pricing.md"),
            "# Pricing\n\n## Strategic Account discounts\n\nStrategic accounts receive a 15 percent discount on every order.\n\n## Timesheets\n\nUnrelated content about timesheet imports.\n",
        )
        .unwrap();
        fs::write(
            dir.join("translations/vi.csv"),
            "Strategic Account,khách hàng chiến lược\nDiscount Policy,chính sách chiết khấu\n",
        )
        .unwrap();
        dir
    }

    fn indexed() -> (crate::store::Store, PathBuf) {
        let root = fixture();
        let mut store = crate::store::Store::in_memory().unwrap();
        index(&mut store, &IndexOptions::new(&root)).unwrap();
        (store, root)
    }

    #[test]
    fn context_finds_the_relevant_code_for_a_task() {
        let (store, root) = indexed();
        let ctx = compile(
            &store,
            "Add a 15% discount for strategic enterprise customers",
            &ContextOptions::default(),
        )
        .unwrap();
        let symbols: Vec<&str> = ctx.code.iter().map(|c| c.symbol.as_str()).collect();
        assert!(
            symbols.iter().any(|s| s.contains("DiscountPolicy")),
            "got {symbols:?}"
        );
        assert!(
            symbols.iter().any(|s| s.contains("StrategicAccount")),
            "got {symbols:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn context_prefers_relevant_code_over_unrelated_code() {
        let (store, root) = indexed();
        let ctx = compile(
            &store,
            "strategic account discount",
            &ContextOptions::default(),
        )
        .unwrap();
        let positions: Vec<usize> = ctx
            .code
            .iter()
            .enumerate()
            .filter(|(_, c)| c.path.contains("unrelated"))
            .map(|(i, _)| i)
            .collect();
        let relevant = ctx
            .code
            .iter()
            .position(|c| c.symbol.contains("DiscountPolicy"));
        if let (Some(rel), Some(&unrel)) = (relevant, positions.first()) {
            assert!(rel < unrel, "relevant code must outrank unrelated code");
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn context_surfaces_the_document_that_states_the_rule() {
        let (store, root) = indexed();
        let ctx = compile(
            &store,
            "strategic account discount",
            &ContextOptions::default(),
        )
        .unwrap();
        assert!(
            ctx.documents
                .iter()
                .any(|d| d.excerpt.contains("15 percent")),
            "documents: {:?}",
            ctx.documents
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_vietnamese_task_reaches_the_same_english_code() {
        // The multilingual claim, measured as a match rather than asserted.
        let (store, root) = indexed();
        let english = compile(&store, "strategic account", &ContextOptions::default()).unwrap();
        let vietnamese =
            compile(&store, "khách hàng chiến lược", &ContextOptions::default()).unwrap();

        let vi_symbols: Vec<&str> = vietnamese.code.iter().map(|c| c.symbol.as_str()).collect();
        assert!(
            vi_symbols.iter().any(|s| s.contains("StrategicAccount")),
            "a Vietnamese query must reach English code; got {vi_symbols:?}"
        );
        assert!(!english.code.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_budget_is_never_exceeded() {
        let (store, root) = indexed();
        for budget in [50u32, 200, 800, 4000] {
            let ctx = compile(
                &store,
                "strategic account discount policy",
                &ContextOptions {
                    for_edit: false,
                    budget,
                    ..Default::default()
                },
            )
            .unwrap();
            // Conflicts are the one admitted overrun, and this fixture has none.
            assert!(
                ctx.budget.used <= budget,
                "used {} exceeded budget {budget}",
                ctx.budget.used
            );
            assert_eq!(ctx.budget.used, ctx.budget.context + ctx.budget.reads);
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn no_single_kind_of_knowledge_can_crowd_out_the_rest() {
        // A query touching common domain nouns matches many concepts, each cheap and
        // high-scoring. Without budget shares they win the whole budget and the answer
        // contains no code at all.
        let (store, root) = indexed();
        let ctx = compile(
            &store,
            "discount policy for strategic account orders",
            &ContextOptions::default(),
        )
        .unwrap();
        assert!(
            !ctx.code.is_empty(),
            "an answer with no code is not an answer"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_candidate_answering_more_of_the_question_ranks_higher() {
        let asked: BTreeSet<String> = ["discount", "strategic", "account"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let node = |name: &str, path: &str| crate::model::Node {
            id: 1,
            uid: name.into(),
            kind: NodeKind::Symbol,
            name: name.into(),
            path: Some(path.into()),
            line_start: 1,
            line_end: 2,
            lang: None,
            status: Status::Confirmed,
            confidence: 1.0,
            tokens: 10,
            data: serde_json::Value::Null,
        };
        let focused = term_coverage(&node("strategic_account_discount", "a/pricing.py"), &asked);
        let vague = term_coverage(&node("get_orders", "a/report.py"), &asked);
        assert!(focused > vague, "{focused} should exceed {vague}");
        assert!((focused - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_symbol_in_a_matching_directory_outranks_one_elsewhere() {
        let asked: BTreeSet<String> = ["timesheet", "billing", "summary"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let node = |path: &str| crate::model::Node {
            id: 1,
            uid: path.into(),
            kind: NodeKind::Symbol,
            name: "execute".into(),
            path: Some(path.into()),
            line_start: 1,
            line_end: 2,
            lang: None,
            status: Status::Confirmed,
            confidence: 1.0,
            tokens: 10,
            data: serde_json::Value::Null,
        };
        let inside = path_affinity(&node("report/timesheet_billing_summary/x.py"), &asked);
        let elsewhere = path_affinity(&node("stock/doctype/bin/bin.py"), &asked);
        assert!(inside > elsewhere);
        assert!((inside - 1.0).abs() < 1e-6);
        assert_eq!(elsewhere, 0.0);
    }

    #[test]
    fn plural_task_words_reach_singular_identifiers() {
        let asked: BTreeSet<String> = ["orders"].iter().map(|s| s.to_string()).collect();
        let node = crate::model::Node {
            id: 1,
            uid: "x".into(),
            kind: NodeKind::Symbol,
            name: "sales_order".into(),
            path: None,
            line_start: 0,
            line_end: 0,
            lang: None,
            status: Status::Confirmed,
            confidence: 1.0,
            tokens: 10,
            data: serde_json::Value::Null,
        };
        assert!(term_coverage(&node, &asked) > 0.9);
    }

    #[test]
    fn each_kind_of_knowledge_is_capped_by_count() {
        let (store, root) = indexed();
        let ctx = compile(
            &store,
            "strategic account discount policy order customer",
            &ContextOptions::default(),
        )
        .unwrap();
        assert!(ctx.concepts.len() <= max_items(NodeKind::Concept));
        assert!(ctx.documents.len() <= max_items(NodeKind::DocSection));
        assert!(ctx.data.len() <= max_items(NodeKind::DatabaseObject));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_smaller_budget_yields_a_smaller_context() {
        let (store, root) = indexed();
        let big = compile(
            &store,
            "strategic account discount",
            &ContextOptions {
                for_edit: false,
                budget: 4000,
                ..Default::default()
            },
        )
        .unwrap();
        let small = compile(
            &store,
            "strategic account discount",
            &ContextOptions {
                for_edit: false,
                budget: 120,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(small.budget.used <= big.budget.used);
        assert!(small.code.len() + small.documents.len() <= big.code.len() + big.documents.len());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_reading_plan_is_inside_the_budget_not_beside_it() {
        // An answer costing 1,400 tokens that tells the agent to read 20,000 more has
        // reduced nothing. The budget must govern the whole cost of following it.
        let (store, root) = indexed();
        for budget in [400u32, 1_000, 4_000] {
            let ctx = compile(
                &store,
                "strategic account discount policy",
                &ContextOptions {
                    for_edit: false,
                    budget,
                    ..Default::default()
                },
            )
            .unwrap();
            let plan: u32 = ctx.next_reads.iter().map(|r| r.est_tokens).sum();
            assert_eq!(plan, ctx.budget.reads);
            assert!(
                ctx.budget.context + plan <= budget,
                "context {} + reads {plan} exceeded {budget}",
                ctx.budget.context
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_answer_is_a_reading_plan_not_a_file_dump() {
        let (store, root) = indexed();
        let ctx = compile(
            &store,
            "strategic account discount",
            &ContextOptions::default(),
        )
        .unwrap();
        assert!(
            !ctx.next_reads.is_empty(),
            "an agent needs somewhere to go next"
        );
        for read in &ctx.next_reads {
            assert!(
                read.lines.contains('-'),
                "reads must be spans, not whole files"
            );
            assert!(read.est_tokens > 0);
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn excluding_a_file_frees_its_budget_for_the_next_candidate() {
        // The iteration primitive: a second call excluding the first answer's files
        // must return *different* files, not a thinner version of the same answer.
        let (store, root) = indexed();
        let first = compile(
            &store,
            "strategic account discount",
            &ContextOptions::default(),
        )
        .unwrap();
        let offered: Vec<String> = first.next_reads.iter().map(|r| r.path.clone()).collect();
        assert!(!offered.is_empty());

        let second = compile(
            &store,
            "strategic account discount",
            &ContextOptions {
                for_edit: false,
                exclude: offered.clone(),
                ..Default::default()
            },
        )
        .unwrap();
        for read in &second.next_reads {
            assert!(
                !offered.contains(&read.path),
                "{} was excluded and came back",
                read.path
            );
        }
        for item in &second.code {
            assert!(!offered.contains(&item.path), "{} in code list", item.path);
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_english_task_reaches_a_vietnamese_document_through_the_concept() {
        // Concept expansion: the task and the document share no token in any
        // language; only the concept knows both names.
        let root = fixture();
        fs::write(
            root.join("docs/chinh-sach.md"),
            "# Chính sách chiết khấu\n\n## Khách hàng chiến lược\n\nKhách hàng chiến lược được hưởng chiết khấu mười lăm phần trăm.\n",
        )
        .unwrap();
        let mut store = crate::store::Store::in_memory().unwrap();
        index(&mut store, &IndexOptions::new(&root)).unwrap();

        let ctx = compile(&store, "strategic account", &ContextOptions::default()).unwrap();
        let rendered = serde_json::to_string(&ctx).unwrap();
        assert!(
            rendered.contains("chinh-sach") || rendered.contains("chiến lược"),
            "the Vietnamese document must be reachable from the English task"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn compilation_is_deterministic() {
        let (store, root) = indexed();
        let once = serde_json::to_string(
            &compile(
                &store,
                "strategic account discount",
                &ContextOptions::default(),
            )
            .unwrap(),
        )
        .unwrap();
        let twice = serde_json::to_string(
            &compile(
                &store,
                "strategic account discount",
                &ContextOptions::default(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(once, twice);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn every_claim_carries_an_epistemic_status() {
        // The safety property: an agent must always be able to tell a parsed fact from
        // a guess, on every item in every section.
        let (store, root) = indexed();
        let ctx = compile(
            &store,
            "strategic account discount",
            &ContextOptions::default(),
        )
        .unwrap();
        let json = serde_json::to_value(&ctx).unwrap();
        for section in ["concepts", "rules", "code", "documents", "data"] {
            for item in json[section].as_array().unwrap() {
                assert!(
                    item.get("status").and_then(|v| v.as_str()).is_some(),
                    "{section} item is missing a status: {item}"
                );
            }
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_task_matching_nothing_reports_unknowns_rather_than_inventing() {
        let (store, root) = indexed();
        let ctx = compile(
            &store,
            "configure the quantum flux capacitor",
            &ContextOptions::default(),
        )
        .unwrap();
        assert!(!ctx.unknowns.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_compiled_context_stays_small() {
        // If Reify's own output is large it has reinvented the problem it solves.
        let (store, root) = indexed();
        let ctx = compile(
            &store,
            "Add a 15% discount for strategic enterprise customers",
            &ContextOptions::default(),
        )
        .unwrap();
        let rendered = serde_json::to_string(&ctx).unwrap();
        let cost = tokens::estimate(&rendered);
        assert!(cost < 4_000, "context rendered to {cost} tokens");
        let _ = fs::remove_dir_all(&root);
    }

    /// One symbol per line of a file, scored so that `flood` outranks everything.
    ///
    /// Built by hand rather than indexed from a fixture: the pathology only appears
    /// when one file holds more symbols than the whole cap, which no fixture small
    /// enough to keep in the repository would reproduce.
    #[cfg(test)]
    fn crowded(flood_file: &str, flood: usize, others: usize) -> Vec<Scored> {
        let mut out = Vec::new();
        let mut make = |id: i64, path: &str, score: f32| {
            out.push(Scored {
                node: crate::model::Node {
                    id,
                    uid: format!("{path}#{id}"),
                    kind: NodeKind::Symbol,
                    name: format!("member_{id}"),
                    path: Some(path.into()),
                    line_start: id as u32 * 10,
                    line_end: id as u32 * 10 + 5,
                    lang: None,
                    status: Status::Confirmed,
                    confidence: 1.0,
                    tokens: 10,
                    data: serde_json::Value::Null,
                },
                score,
                reason: "test".into(),
            });
        };
        for i in 0..flood {
            make(i as i64 + 1, flood_file, 1.0 - i as f32 * 0.001);
        }
        for i in 0..others {
            make(1_000 + i as i64, &format!("src/other_{i}.rs"), 0.5);
        }
        out
    }

    #[test]
    fn no_single_file_can_claim_every_symbol_slot() {
        // Relevance spreads along edges, so every member of a loosely-matched file
        // arrives holding a plausible score. Measured on Medusa before this cap, one
        // HTTP router held 8 of 20 slots for a task about discounts.
        let scored = crowded("src/router.rs", 12, 8);
        let chosen = select(&scored, 4_000);
        let mut per_file: HashMap<&str, usize> = HashMap::new();
        for item in &chosen {
            if let Some(path) = &item.node.path {
                *per_file.entry(path.as_str()).or_insert(0) += 1;
            }
        }
        assert_eq!(
            per_file.get("src/router.rs").copied().unwrap_or(0),
            MAX_SYMBOLS_PER_FILE,
            "the flooding file should be held to the cap"
        );
        assert!(
            per_file.len() > 1,
            "capping the flood must leave room for other files, saw {per_file:?}"
        );
    }

    #[test]
    fn the_reading_plan_visits_distinct_files_before_revisiting_one() {
        // A plan whose six entries all name one file points at one place and calls
        // itself a reading list. Every ranked file earns a span before any earns a
        // second, so the plan is a plan.
        let scored = crowded("src/router.rs", 12, 8);
        let chosen = select(&scored, 4_000);
        let plan = reading_plan(&chosen, MAX_NEXT_READS, 40_000, 0.0);
        assert_eq!(plan.len(), MAX_NEXT_READS, "the plan should be full");
        let distinct: BTreeSet<&str> = plan.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(
            distinct.len(),
            plan.len(),
            "plan named {} files across {} entries: {:?}",
            distinct.len(),
            plan.len(),
            plan.iter().map(|r| &r.path).collect::<Vec<_>>()
        );
    }

    #[test]
    fn an_implementation_outranks_the_test_that_exercises_it() {
        // A test names the task's vocabulary as densely as the code it exercises, so
        // it arrives scoring at least as well, and a reader cannot edit it to change
        // the behaviour. It should still be reachable — hence a penalty, not an
        // exclusion: at the default weight the test keeps half its score, not none.
        let symbol = |path: &str| crate::model::Node {
            id: 1,
            uid: path.into(),
            kind: NodeKind::Symbol,
            name: "requires_approval".into(),
            path: Some(path.into()),
            line_start: 1,
            line_end: 2,
            lang: None,
            status: Status::Confirmed,
            confidence: 1.0,
            tokens: 10,
            data: serde_json::Value::Null,
        };
        let penalty = RankWeights::default().test_path_penalty;
        let source = test_path_factor(&symbol("app/order.py"), penalty);
        let test = test_path_factor(&symbol("app/test_rules.py"), penalty);
        assert!(
            test < source,
            "a test scoring equally with its implementation must lose to it: {test} vs {source}"
        );
        assert!(test > 0.0, "a penalty must not become an exclusion");
    }

    #[test]
    fn test_detection_matches_path_segments_not_substrings() {
        // `contest` and `latest` contain "test"; neither is one.
        for path in [
            "app/tests/test_order.py",
            "src/order.test.ts",
            "pkg/order_test.go",
            "integration-tests/__fixtures__/promotion/index.ts",
            "spec/models/order_spec.rb",
            "src/__mocks__/stripe.ts",
        ] {
            assert!(is_test_path(path), "{path} should read as test material");
        }
        for path in [
            "app/contest/entry.py",
            "src/latest/version.ts",
            "src/protest/handler.go",
            "app/order.py",
        ] {
            assert!(!is_test_path(path), "{path} is not test material");
        }
    }
}

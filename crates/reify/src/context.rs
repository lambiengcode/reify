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
//! Four stages (`docs/PLAN.md` §F.3): seed, spread, select, render.

use anyhow::Result;
use serde::Serialize;
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
/// Rough tokens per line of source, for estimating the cost of a recommended read.
const TOKENS_PER_LINE: u32 = 10;

/// How much a directory path agreeing with the task lifts everything inside it.
const PATH_AFFINITY_WEIGHT: f32 = 1.0;

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

#[derive(Debug, Clone)]
pub struct ContextOptions {
    pub budget: u32,
    pub max_next_reads: usize,
}

impl Default for ContextOptions {
    fn default() -> Self {
        ContextOptions {
            budget: DEFAULT_BUDGET,
            max_next_reads: MAX_NEXT_READS,
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
    let scored = rank(store, task)?;
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

    for item in &selected {
        match item.node.kind {
            NodeKind::Concept => context.concepts.push(concept_out(&item.node)),
            NodeKind::Symbol => context.code.push(code_out(&item.node, &item.reason)),
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
    context.next_reads = reading_plan(&selected, opts.max_next_reads, read_budget);
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
fn rank(store: &Store, task: &str) -> Result<Vec<Scored>> {
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
            * (0.35 + 0.65 * coverage)
            * (1.0 + PATH_AFFINITY_WEIGHT * affinity);
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
    for word in meaningful_words(task) {
        for node in store.symbols_named(&word)? {
            bump(&mut scores, &mut nodes, node, 0.9, "named in the task");
        }
    }

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

    let mut out: Vec<Scored> = scores
        .into_iter()
        .filter_map(|(id, (score, reason))| {
            nodes.remove(&id).map(|node| Scored {
                node,
                score,
                reason,
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

/// Match singular against plural without pulling in a stemmer.
///
/// "orders" in a task must reach `sales_order`; anything more elaborate is not worth a
/// dependency for the one inflection English uses in identifiers.
fn stem_match(candidate: &str, asked: &str) -> bool {
    let strip = |w: &str| w.strip_suffix('s').unwrap_or(w).to_string();
    strip(candidate) == strip(asked)
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

/// Fill the budget, best value per token first.
///
/// Two deliberate asymmetries. A `CONFLICTED` node is admitted regardless of budget,
/// because budget pressure may drop useful context but must never drop a known
/// contradiction. And the single best concept and document are reserved, so a
/// symbol-heavy result never crowds out the two things that explain it.
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
    for item in &chosen {
        *count_by_kind.entry(item.node.kind).or_insert(0) += 1;
        *tokens_by_kind.entry(item.node.kind).or_insert(0) += item.node.tokens;
    }

    // Best value per token, subject to both caps.
    for item in &remaining {
        let kind = item.node.kind;
        let token_cap = (budget as f32 * budget_share(kind)) as u32;
        let count = *count_by_kind.get(&kind).unwrap_or(&0);
        let used = *tokens_by_kind.get(&kind).unwrap_or(&0);
        if spent + item.node.tokens > budget
            || count >= max_items(kind)
            || used + item.node.tokens > token_cap
        {
            continue;
        }
        *count_by_kind.entry(kind).or_insert(0) += 1;
        *tokens_by_kind.entry(kind).or_insert(0) += item.node.tokens;
        admit(item, &mut chosen, &mut spent, &mut taken);
    }
    // Spend budget the shares left unused on more code, which is what a change needs
    // — but never past the count cap, or the answer becomes a directory listing.
    for item in &remaining {
        let kind = item.node.kind;
        if taken.contains(&item.node.id)
            || kind != NodeKind::Symbol
            || *count_by_kind.get(&kind).unwrap_or(&0) >= max_items(kind)
            || spent + item.node.tokens > budget
        {
            continue;
        }
        *count_by_kind.entry(kind).or_insert(0) += 1;
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
fn reading_plan(selected: &[Scored], limit: usize, budget: u32) -> Vec<NextRead> {
    let mut plan: Vec<NextRead> = Vec::new();
    let mut spent = 0u32;
    for item in selected {
        if plan.len() >= limit {
            break;
        }
        if item.node.kind != NodeKind::Symbol || item.node.line_start == 0 {
            continue;
        }
        let Some(path) = &item.node.path else {
            continue;
        };
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
    plan
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
                budget: 4000,
                ..Default::default()
            },
        )
        .unwrap();
        let small = compile(
            &store,
            "strategic account discount",
            &ContextOptions {
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
}

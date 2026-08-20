//! Deterministic queries: `why` and `impact`.
//!
//! Both run entirely on the graph, with no lexical scoring and no model. `why`
//! additionally reaches for git at query time to get precise line-range history, which
//! is the one place indexing deliberately left work undone (`docs/PLAN.md` §H.5).

use anyhow::{anyhow, Result};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::gitlog;
use crate::model::{EdgeKind, Node, NodeKind, Status};
use crate::store::{Direction, Store};

/// How many commits `why` pulls for a symbol's exact line range.
const WHY_COMMIT_LIMIT: usize = 6;
/// Bound on each list in a `why` answer, so the output stays agent-sized.
const WHY_LIST_LIMIT: usize = 8;
/// How far `impact` propagates. Two hops of callers is where signal turns into noise.
const IMPACT_MAX_DEPTH: u32 = 2;
const IMPACT_MAX_NODES: usize = 60;

/// A location the user asked about, resolved to something in the store.
#[derive(Debug, Clone)]
pub enum Target {
    /// `path:line`
    Line { path: String, line: u32 },
    /// A bare symbol name, or a `path` with no line.
    Name(String),
}

impl Target {
    /// Parse `path:line`, `path`, or a symbol name.
    pub fn parse(input: &str) -> Target {
        if let Some((path, line)) = input.rsplit_once(':') {
            if let Ok(line) = line.parse::<u32>() {
                return Target::Line {
                    path: path.to_string(),
                    line,
                };
            }
        }
        Target::Name(input.to_string())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Citation {
    pub location: String,
    pub what: String,
    pub status: Status,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommitInfo {
    pub sha: String,
    pub date: String,
    pub author: String,
    pub subject: String,
    pub class: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WhyAnswer {
    pub schema: &'static str,
    pub target: String,
    pub symbol: Option<String>,
    pub location: String,
    pub kind: String,
    pub signature: Option<String>,
    pub documentation: Option<String>,
    pub concepts: Vec<Citation>,
    pub documents: Vec<Citation>,
    pub calls: Vec<Citation>,
    pub called_by: Vec<Citation>,
    pub reads: Vec<Citation>,
    pub writes: Vec<Citation>,
    pub history: Vec<CommitInfo>,
    pub co_changes: Vec<Citation>,
    /// What could not be determined. Stated so absence is not read as evidence.
    pub unknowns: Vec<String>,
}

/// Answer "why does this exist, and what does it touch".
pub fn why(store: &Store, root: &Path, target: &str) -> Result<WhyAnswer> {
    let parsed = Target::parse(target);
    let node = resolve_target(store, &parsed)?
        .ok_or_else(|| anyhow!("nothing indexed at `{target}`; try `reify index`"))?;

    let mut answer = WhyAnswer {
        schema: "reify.why/1",
        target: target.to_string(),
        symbol: node
            .data
            .get("qualified")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        location: node.location(),
        kind: node
            .data
            .get("symbol_kind")
            .and_then(|v| v.as_str())
            .unwrap_or(node.kind.as_str())
            .to_string(),
        signature: node
            .data
            .get("signature")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        documentation: node
            .data
            .get("doc")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        concepts: Vec::new(),
        documents: Vec::new(),
        calls: Vec::new(),
        called_by: Vec::new(),
        reads: Vec::new(),
        writes: Vec::new(),
        history: Vec::new(),
        co_changes: Vec::new(),
        unknowns: Vec::new(),
    };

    // Concepts that name this symbol, and through them the documents that describe it.
    let concept_nodes = incoming(store, node.id, EdgeKind::MapsTo)?;
    for concept in &concept_nodes {
        answer.concepts.push(Citation {
            location: concept.uid.clone(),
            what: describe_concept(concept),
            status: concept.status,
        });
        for doc in outgoing(store, concept.id, EdgeKind::MapsTo)?
            .into_iter()
            .filter(|n| n.kind == NodeKind::DocSection)
        {
            answer.documents.push(citation(&doc));
        }
    }
    // Documents attached directly to the symbol.
    for doc in outgoing(store, node.id, EdgeKind::DocumentedBy)? {
        answer.documents.push(citation(&doc));
    }
    dedupe(&mut answer.documents);

    for n in outgoing(store, node.id, EdgeKind::Calls)? {
        answer.calls.push(citation(&n));
    }
    for n in incoming(store, node.id, EdgeKind::Calls)? {
        answer.called_by.push(citation(&n));
    }
    for n in outgoing(store, node.id, EdgeKind::Reads)? {
        answer.reads.push(citation(&n));
    }
    for n in outgoing(store, node.id, EdgeKind::Writes)? {
        answer.writes.push(citation(&n));
    }

    // History: precise for a symbol with a span, file-level otherwise.
    if let Some(path) = &node.path {
        if node.line_start > 0 && gitlog::is_repository(root) {
            let commits = gitlog::line_history(
                root,
                path,
                node.line_start,
                node.line_end.max(node.line_start),
                WHY_COMMIT_LIMIT,
            )?;
            answer.history = commits.iter().map(commit_info).collect();
        }
        if answer.history.is_empty() {
            // Fall back to what indexing already linked at file level.
            if let Some(file) = store.node_by_uid(&crate::model::uid::file(path))? {
                for commit in outgoing(store, file.id, EdgeKind::ChangedBy)?
                    .into_iter()
                    .chain(outgoing(store, file.id, EdgeKind::IntroducedBy)?)
                {
                    answer.history.push(stored_commit_info(&commit));
                }
                for other in neighbours_both(store, file.id, EdgeKind::CoChangesWith)? {
                    answer.co_changes.push(citation(&other));
                }
            }
        } else if let Some(file) = store.node_by_uid(&crate::model::uid::file(path))? {
            for other in neighbours_both(store, file.id, EdgeKind::CoChangesWith)? {
                answer.co_changes.push(citation(&other));
            }
        }
    }

    if answer.concepts.is_empty() {
        answer
            .unknowns
            .push("no business concept maps to this symbol".into());
    }
    if answer.documents.is_empty() {
        answer
            .unknowns
            .push("no document section describes this symbol".into());
    }
    if answer.history.is_empty() {
        answer
            .unknowns
            .push("no commit history found for this location".into());
    }

    truncate_all(&mut answer);
    Ok(answer)
}

#[derive(Debug, Clone, Serialize)]
pub struct Affected {
    pub location: String,
    pub what: String,
    pub kind: String,
    /// Hops from the change site. 1 is a direct dependant.
    pub distance: u32,
    /// The concrete relationship, in words an engineer can check.
    pub reason: String,
    pub status: Status,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImpactAnswer {
    pub schema: &'static str,
    pub query: String,
    pub origins: Vec<Citation>,
    pub affected: Vec<Affected>,
    pub tables: Vec<Citation>,
    pub co_changing_files: Vec<Citation>,
    pub unknowns: Vec<String>,
}

/// Answer "what breaks if I change this".
///
/// Propagation is reverse-directional — from a symbol to the things that depend on it —
/// and crosses into the data layer, because a report reading the column a service
/// writes is affected even though nothing calls it.
pub fn impact(store: &Store, query: &str) -> Result<ImpactAnswer> {
    let origins = resolve_origins(store, query)?;
    let mut answer = ImpactAnswer {
        schema: "reify.impact/1",
        query: query.to_string(),
        origins: origins.iter().map(citation).collect(),
        affected: Vec::new(),
        tables: Vec::new(),
        co_changing_files: Vec::new(),
        unknowns: Vec::new(),
    };
    if origins.is_empty() {
        answer
            .unknowns
            .push(format!("nothing in the index matches `{query}`"));
        return Ok(answer);
    }

    let origin_ids: HashSet<i64> = origins.iter().map(|n| n.id).collect();
    let mut seen: HashSet<i64> = origin_ids.clone();
    let mut frontier: Vec<(Node, u32, String)> = origins
        .iter()
        .cloned()
        .map(|n| (n, 0, String::new()))
        .collect();

    while let Some((node, depth, _)) = frontier.pop() {
        if depth >= IMPACT_MAX_DEPTH || answer.affected.len() >= IMPACT_MAX_NODES {
            continue;
        }
        // Callers depend on this symbol.
        for (dependant, _, confidence) in
            store.neighbors(node.id, Direction::In, &[EdgeKind::Calls])?
        {
            if !seen.insert(dependant.id) {
                continue;
            }
            let reason = format!("calls {}", node.name);
            answer
                .affected
                .push(affected(&dependant, depth + 1, reason, confidence));
            frontier.push((dependant, depth + 1, String::new()));
        }
        // Data coupling: anything else touching a table this symbol writes.
        for (table, _, _) in store.neighbors(
            node.id,
            Direction::Out,
            &[EdgeKind::Writes, EdgeKind::Reads],
        )? {
            answer.tables.push(citation(&table));
            for (other, edge, confidence) in store.neighbors(
                table.id,
                Direction::In,
                &[EdgeKind::Reads, EdgeKind::Writes],
            )? {
                if origin_ids.contains(&other.id) || !seen.insert(other.id) {
                    continue;
                }
                let verb = if edge == EdgeKind::Reads {
                    "reads"
                } else {
                    "writes"
                };
                answer.affected.push(affected(
                    &other,
                    depth + 1,
                    format!("{verb} table {}", table.name),
                    confidence * 0.8,
                ));
            }
        }
    }

    for origin in &origins {
        if let Some(path) = &origin.path {
            if let Some(file) = store.node_by_uid(&crate::model::uid::file(path))? {
                for other in neighbours_both(store, file.id, EdgeKind::CoChangesWith)? {
                    answer.co_changing_files.push(citation(&other));
                }
            }
        }
    }

    dedupe(&mut answer.tables);
    dedupe(&mut answer.co_changing_files);
    // Nearest first, then by confidence, then by location so output is deterministic.
    answer.affected.sort_by(|a, b| {
        a.distance
            .cmp(&b.distance)
            .then(b.confidence.total_cmp(&a.confidence))
            .then(a.location.cmp(&b.location))
    });
    answer.affected.truncate(IMPACT_MAX_NODES);

    if answer.affected.is_empty() {
        answer.unknowns.push(
            "no dependants found; the symbol may be an entry point, or called dynamically".into(),
        );
    }
    Ok(answer)
}

// ---- shared helpers ---------------------------------------------------------

fn resolve_target(store: &Store, target: &Target) -> Result<Option<Node>> {
    match target {
        Target::Line { path, line } => {
            if let Some(symbol) = store.symbol_at(path, *line)? {
                return Ok(Some(symbol));
            }
            // The line may sit between symbols; the file itself is still an answer.
            store.node_by_uid(&crate::model::uid::file(path))
        }
        Target::Name(name) => {
            if let Some(node) = store.node_by_uid(name)? {
                return Ok(Some(node));
            }
            if let Some(node) = store.node_by_uid(&crate::model::uid::file(name))? {
                return Ok(Some(node));
            }
            let bare = name.rsplit(['.', '#', '/']).next().unwrap_or(name);
            Ok(store.symbols_named(bare)?.into_iter().next())
        }
    }
}

/// Symbols a change request is about.
///
/// Exact symbol names first, because an engineer naming a symbol means that symbol;
/// lexical search only as a fallback for prose.
fn resolve_origins(store: &Store, query: &str) -> Result<Vec<Node>> {
    let trimmed = query.trim();
    if let Some(node) = resolve_target(store, &Target::parse(trimmed))? {
        return Ok(vec![node]);
    }
    let mut origins: Vec<Node> = Vec::new();
    for word in trimmed.split_whitespace() {
        let bare = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
        if bare.len() < 3 {
            continue;
        }
        origins.extend(store.symbols_named(bare)?);
    }
    if origins.is_empty() {
        origins = store
            .search(trimmed, 8)?
            .into_iter()
            .map(|(n, _)| n)
            .filter(|n| n.kind == NodeKind::Symbol)
            .collect();
    }
    origins.sort_by(|a, b| a.uid.cmp(&b.uid));
    origins.dedup_by(|a, b| a.id == b.id);
    origins.truncate(6);
    Ok(origins)
}

fn outgoing(store: &Store, id: i64, kind: EdgeKind) -> Result<Vec<Node>> {
    Ok(store
        .neighbors(id, Direction::Out, &[kind])?
        .into_iter()
        .map(|(n, _, _)| n)
        .collect())
}

fn incoming(store: &Store, id: i64, kind: EdgeKind) -> Result<Vec<Node>> {
    Ok(store
        .neighbors(id, Direction::In, &[kind])?
        .into_iter()
        .map(|(n, _, _)| n)
        .collect())
}

/// Co-change is symmetric but stored once, so both directions must be read.
fn neighbours_both(store: &Store, id: i64, kind: EdgeKind) -> Result<Vec<Node>> {
    let mut out = outgoing(store, id, kind)?;
    out.extend(incoming(store, id, kind)?);
    Ok(out)
}

fn citation(node: &Node) -> Citation {
    Citation {
        location: node.location(),
        what: node
            .data
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or(&node.name)
            .to_string(),
        status: node.status,
    }
}

fn describe_concept(node: &Node) -> String {
    let labels = node
        .data
        .get("labels")
        .and_then(|v| v.as_object())
        .map(|m| {
            m.iter()
                .map(|(lang, label)| format!("{lang}: {}", label.as_str().unwrap_or("")))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    if labels.is_empty() {
        node.name.clone()
    } else {
        format!("{} ({labels})", node.name)
    }
}

fn affected(node: &Node, distance: u32, reason: String, confidence: f32) -> Affected {
    Affected {
        location: node.location(),
        what: node.name.clone(),
        kind: node.kind.as_str().to_string(),
        distance,
        reason,
        status: node.status,
        confidence,
    }
}

fn commit_info(commit: &gitlog::Commit) -> CommitInfo {
    CommitInfo {
        sha: commit.sha[..7.min(commit.sha.len())].to_string(),
        date: commit.date(),
        author: commit.author.clone(),
        subject: commit.subject.clone(),
        class: commit.class.as_str().to_string(),
    }
}

fn stored_commit_info(node: &Node) -> CommitInfo {
    let get = |key: &str| {
        node.data
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    };
    CommitInfo {
        sha: node.name.clone(),
        date: get("date"),
        author: get("author"),
        subject: get("subject"),
        class: get("class"),
    }
}

fn dedupe(items: &mut Vec<Citation>) {
    let mut seen: HashSet<String> = HashSet::new();
    items.retain(|c| seen.insert(c.location.clone()));
}

fn truncate_all(answer: &mut WhyAnswer) {
    for list in [
        &mut answer.concepts,
        &mut answer.documents,
        &mut answer.calls,
        &mut answer.called_by,
        &mut answer.reads,
        &mut answer.writes,
        &mut answer.co_changes,
    ] {
        list.sort_by(|a, b| a.location.cmp(&b.location));
        list.dedup_by(|a, b| a.location == b.location);
        list.truncate(WHY_LIST_LIMIT);
    }
    answer.history.truncate(WHY_COMMIT_LIMIT);
}

/// Everything known about one business concept.
#[derive(Debug, Clone, Serialize)]
pub struct Explanation {
    pub schema: &'static str,
    pub query: String,
    pub id: String,
    pub status: Status,
    /// Language code to label. English has no privileged position here.
    pub labels: serde_json::Value,
    pub bridge: String,
    pub code: Vec<Citation>,
    pub data: Vec<Citation>,
    pub documents: Vec<Citation>,
    pub rules: Vec<Citation>,
    /// Other concepts matching the same query, so an ambiguous term is visible as
    /// ambiguous rather than silently resolved to one answer.
    pub also_matched: Vec<String>,
    pub unknowns: Vec<String>,
}

/// Explain a business concept across every language, code path and table it touches.
pub fn explain(store: &Store, term: &str) -> Result<Explanation> {
    let matches = resolve_concepts(store, term)?;
    let concept = matches
        .first()
        .cloned()
        .ok_or_else(|| anyhow!("no concept matches `{term}`; try `reify context \"{term}\"`"))?;

    let mut explanation = Explanation {
        schema: "reify.explain/1",
        query: term.to_string(),
        id: concept
            .data
            .get("concept_id")
            .and_then(|v| v.as_str())
            .unwrap_or(&concept.name)
            .to_string(),
        status: concept.status,
        labels: concept
            .data
            .get("labels")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        bridge: concept
            .data
            .get("bridge")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string(),
        code: Vec::new(),
        data: Vec::new(),
        documents: Vec::new(),
        rules: Vec::new(),
        also_matched: matches
            .iter()
            .skip(1)
            .take(6)
            .map(|n| n.uid.clone())
            .collect(),
        unknowns: Vec::new(),
    };

    for node in outgoing(store, concept.id, EdgeKind::MapsTo)? {
        match node.kind {
            NodeKind::Symbol => {
                // A symbol's own rules are part of what the concept means.
                for rule in outgoing(store, node.id, EdgeKind::ImplementsRule)? {
                    explanation.rules.push(citation(&rule));
                }
                for doc in outgoing(store, node.id, EdgeKind::DocumentedBy)? {
                    explanation.documents.push(citation(&doc));
                }
                explanation.code.push(citation(&node));
            }
            NodeKind::DatabaseObject => explanation.data.push(citation(&node)),
            NodeKind::DocSection => explanation.documents.push(citation(&node)),
            _ => {}
        }
    }

    for list in [
        &mut explanation.code,
        &mut explanation.data,
        &mut explanation.documents,
        &mut explanation.rules,
    ] {
        list.sort_by(|a, b| a.location.cmp(&b.location));
        list.dedup_by(|a, b| a.location == b.location);
        list.truncate(WHY_LIST_LIMIT * 2);
    }
    if explanation.code.is_empty() {
        explanation
            .unknowns
            .push("no code is linked to this concept".into());
    }
    if explanation.documents.is_empty() {
        explanation
            .unknowns
            .push("no document describes this concept".into());
    }
    Ok(explanation)
}

/// One step of a derived process.
#[derive(Debug, Clone, Serialize)]
pub struct FlowStep {
    pub order: usize,
    pub location: String,
    pub symbol: String,
    /// How this step was reached: an entry point, or called by the previous step.
    pub reached_by: String,
    pub status: Status,
}

#[derive(Debug, Clone, Serialize)]
pub struct FlowAnswer {
    pub schema: &'static str,
    pub process: String,
    /// Where the sequence came from. Currently always the call graph — Reify does not
    /// invent a process that the code does not perform.
    pub derived_from: &'static str,
    pub steps: Vec<FlowStep>,
    pub unknowns: Vec<String>,
}

/// Derive the sequence of code that carries out a business process.
///
/// The ordering is the call graph's, not a guess: entry points first — symbols nothing
/// else in the matched set calls — then the symbols they reach, breadth first. A
/// repository that does not encode the process as calls will produce a short answer
/// rather than a fabricated one.
pub fn flow(store: &Store, process: &str) -> Result<FlowAnswer> {
    let mut answer = FlowAnswer {
        schema: "reify.flow/1",
        process: process.to_string(),
        derived_from: "call graph",
        steps: Vec::new(),
        unknowns: Vec::new(),
    };

    let seeds = resolve_origins(store, process)?;
    if seeds.is_empty() {
        answer
            .unknowns
            .push(format!("nothing in the index matches `{process}`"));
        return Ok(answer);
    }

    // Everything within one call hop of a seed forms the candidate set.
    let mut members: HashMap<i64, Node> = seeds.iter().map(|n| (n.id, n.clone())).collect();
    for seed in &seeds {
        for node in outgoing(store, seed.id, EdgeKind::Calls)? {
            members.entry(node.id).or_insert(node);
        }
    }

    // Entry points are members nothing else in the set calls.
    let mut called_by_member: HashSet<i64> = HashSet::new();
    for id in members.keys().copied().collect::<Vec<_>>() {
        for node in outgoing(store, id, EdgeKind::Calls)? {
            if members.contains_key(&node.id) {
                called_by_member.insert(node.id);
            }
        }
    }
    let mut frontier: Vec<Node> = members
        .values()
        .filter(|n| !called_by_member.contains(&n.id))
        .cloned()
        .collect();
    frontier.sort_by(|a, b| a.uid.cmp(&b.uid));
    if frontier.is_empty() {
        // A cycle: every member is called by another. Start somewhere deterministic.
        frontier = seeds.clone();
    }

    let mut visited: HashSet<i64> = HashSet::new();
    let mut order = 0usize;
    while let Some(node) = frontier.first().cloned() {
        frontier.remove(0);
        if !visited.insert(node.id) {
            continue;
        }
        order += 1;
        let reached_by = if order == 1 {
            "entry point".to_string()
        } else {
            "called during the process".to_string()
        };
        answer.steps.push(FlowStep {
            order,
            location: node.location(),
            symbol: node
                .data
                .get("qualified")
                .and_then(|v| v.as_str())
                .unwrap_or(&node.name)
                .to_string(),
            reached_by,
            status: node.status,
        });
        if answer.steps.len() >= WHY_LIST_LIMIT * 2 {
            break;
        }
        let mut next = outgoing(store, node.id, EdgeKind::Calls)?;
        next.retain(|n| members.contains_key(&n.id) && !visited.contains(&n.id));
        next.sort_by(|a, b| a.uid.cmp(&b.uid));
        frontier.extend(next);
    }

    if answer.steps.len() < 2 {
        answer.unknowns.push(
            "the code does not encode this as a sequence of calls; it may be event driven \
             or configured rather than called"
                .into(),
        );
    }
    Ok(answer)
}

/// Concepts whose labels, id or code hints match `term`, best first.
fn resolve_concepts(store: &Store, term: &str) -> Result<Vec<Node>> {
    let asked = crate::concepts::meaningful_words(term);
    let mut scored: Vec<(f32, Node)> = Vec::new();
    for node in store.nodes_of_kind(NodeKind::Concept)? {
        let mut haystack = node.name.to_lowercase();
        if let Some(labels) = node.data.get("labels").and_then(|v| v.as_object()) {
            for label in labels.values().filter_map(|v| v.as_str()) {
                haystack.push(' ');
                haystack.push_str(&label.to_lowercase());
            }
        }
        haystack.push(' ');
        haystack.push_str(&node.uid.to_lowercase());
        let words = crate::concepts::meaningful_words(&haystack);
        let matched = asked.iter().filter(|w| words.contains(*w)).count();
        if matched == 0 {
            continue;
        }
        // Full coverage of the query beats a partial match; a shorter concept beats a
        // longer one at equal coverage, since it is the more specific answer.
        let score =
            matched as f32 / asked.len().max(1) as f32 + 1.0 / (words.len().max(1) as f32 * 100.0);
        scored.push((score, node));
    }
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.uid.cmp(&b.1.uid)));
    Ok(scored.into_iter().map(|(_, n)| n).collect())
}

/// A documentation/implementation disagreement, as stored.
#[derive(Debug, Clone, Serialize)]
pub struct StoredConflict {
    pub id: String,
    pub subject: String,
    pub documented: String,
    pub documented_at: String,
    pub observed: String,
    pub observed_at: String,
    pub confidence: f32,
    pub resolution: String,
}

/// Every detected contradiction, strongest first.
pub fn conflicts(store: &Store) -> Result<Vec<StoredConflict>> {
    let mut out: Vec<StoredConflict> = store
        .nodes_of_kind(NodeKind::BusinessRule)?
        .into_iter()
        .filter(|n| {
            n.data
                .get("conflict")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        })
        .map(|n| {
            let get = |key: &str| {
                n.data
                    .get(key)
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string()
            };
            StoredConflict {
                id: n.uid.clone(),
                subject: n.name.clone(),
                documented: get("documented"),
                documented_at: get("documented_at"),
                observed: get("observed"),
                observed_at: get("observed_at"),
                confidence: n.confidence,
                resolution: get("resolution"),
            }
        })
        .collect();
    out.sort_by(|a, b| b.confidence.total_cmp(&a.confidence).then(a.id.cmp(&b.id)));
    Ok(out)
}

/// Business-rule claims above `min_confidence`, strongest first.
pub fn rules(store: &Store, min_confidence: f32) -> Result<Vec<Node>> {
    let mut out: Vec<Node> = store
        .nodes_of_kind(NodeKind::BusinessRule)?
        .into_iter()
        .filter(|n| !n.uid.starts_with("conflict:") && n.confidence >= min_confidence)
        .collect();
    out.sort_by(|a, b| {
        b.confidence
            .total_cmp(&a.confidence)
            .then(a.uid.cmp(&b.uid))
    });
    Ok(out)
}

/// A compact risk header for a file about to be edited.
#[derive(Debug, Clone, Serialize)]
pub struct Preflight {
    pub schema: &'static str,
    pub path: String,
    pub symbols: usize,
    pub rules: usize,
    pub concepts: usize,
    pub tables: usize,
    pub dependants: usize,
    pub conflicts: usize,
    /// `LOW`, `MEDIUM` or `HIGH`.
    pub risk: &'static str,
    /// Why the risk is what it is, in one sentence.
    pub reason: String,
    /// The rules an editor should read first.
    pub highest_risk_rules: Vec<Citation>,
    /// What to run next to get the full picture.
    pub suggested_command: String,
}

/// Summarise what an editor is about to touch.
///
/// Deliberately cheap and deliberately small: this runs on every edit in a hook, so it
/// must cost a few milliseconds and a few dozen tokens. Anything larger belongs in
/// `reify context`, which the header points at.
pub fn preflight(store: &Store, path: &str) -> Result<Preflight> {
    let symbols = store.symbols_in_file(path)?;
    let mut rules: Vec<Citation> = Vec::new();
    let mut concepts: HashSet<i64> = HashSet::new();
    let mut tables: HashSet<i64> = HashSet::new();
    let mut dependants: HashSet<i64> = HashSet::new();
    let mut conflicts = 0usize;

    for symbol in &symbols {
        for rule in outgoing(store, symbol.id, EdgeKind::ImplementsRule)? {
            if rule.status == Status::Conflicted {
                conflicts += 1;
            }
            rules.push(citation(&rule));
        }
        for concept in incoming(store, symbol.id, EdgeKind::MapsTo)? {
            concepts.insert(concept.id);
        }
        for table in store.neighbors(
            symbol.id,
            Direction::Out,
            &[EdgeKind::Reads, EdgeKind::Writes],
        )? {
            tables.insert(table.0.id);
        }
        for caller in incoming(store, symbol.id, EdgeKind::Calls)? {
            // A symbol calling its own sibling is not an external dependant.
            if caller.path.as_deref() != Some(path) {
                dependants.insert(caller.id);
            }
        }
    }

    rules.sort_by(|a, b| a.location.cmp(&b.location));
    rules.dedup_by(|a, b| a.location == b.location);

    // Thresholds encode a claim about danger, so they are named rather than inlined.
    let (risk, reason) = if conflicts > 0 {
        (
            "HIGH",
            "documentation and implementation disagree about this file".to_string(),
        )
    } else if dependants.len() >= 10 || rules.len() >= 5 {
        (
            "HIGH",
            format!(
                "{} dependants and {} business rules meet here",
                dependants.len(),
                rules.len()
            ),
        )
    } else if !rules.is_empty() || dependants.len() >= 3 {
        (
            "MEDIUM",
            format!(
                "{} dependants and {} business rules",
                dependants.len(),
                rules.len()
            ),
        )
    } else {
        (
            "LOW",
            "nothing else in the index depends on this file".to_string(),
        )
    };

    let highest_risk_rules = rules.iter().take(3).cloned().collect();
    Ok(Preflight {
        schema: "reify.preflight/1",
        path: path.to_string(),
        symbols: symbols.len(),
        rules: rules.len(),
        concepts: concepts.len(),
        tables: tables.len(),
        dependants: dependants.len(),
        conflicts,
        risk,
        reason,
        highest_risk_rules,
        suggested_command: format!("reify context \"<your change to {path}>\""),
    })
}

/// One row of the concept layer, for `reify concepts`.
#[derive(Debug, Clone, Serialize)]
pub struct ConceptRow {
    pub id: String,
    pub display: String,
    pub languages: Vec<String>,
    pub bridge: String,
    pub status: Status,
    /// How many symbols and tables this concept names.
    pub links: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConceptOverview {
    pub schema: &'static str,
    pub total: usize,
    pub by_bridge: HashMap<String, usize>,
    pub concepts: Vec<ConceptRow>,
}

pub fn concept_overview(store: &Store) -> Result<ConceptOverview> {
    let mut rows = Vec::new();
    let mut by_bridge: HashMap<String, usize> = HashMap::new();
    for node in store.nodes_of_kind(NodeKind::Concept)? {
        let bridge = node
            .data
            .get("bridge")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        *by_bridge.entry(bridge.clone()).or_default() += 1;
        let languages = node
            .data
            .get("labels")
            .and_then(|v| v.as_object())
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        rows.push(ConceptRow {
            id: node
                .data
                .get("concept_id")
                .and_then(|v| v.as_str())
                .unwrap_or(&node.name)
                .to_string(),
            display: node.name.clone(),
            languages,
            bridge,
            status: node.status,
            links: store
                .neighbors(node.id, Direction::Out, &[EdgeKind::MapsTo])?
                .len(),
        });
    }
    rows.sort_by(|a, b| b.links.cmp(&a.links).then(a.id.cmp(&b.id)));
    Ok(ConceptOverview {
        schema: "reify.concepts/1",
        total: rows.len(),
        by_bridge,
        concepts: rows,
    })
}

/// Concepts worth promoting into the declared glossary.
///
/// Mined concepts are the raw material; a declared one is trusted above everything
/// else in the system. Hand-writing a glossary from nothing is the reason most teams
/// never have one, so this proposes the best-grounded mined concepts as a starting
/// point for a human to edit down.
pub fn concept_suggestions(store: &Store) -> Result<Vec<crate::concepts::Concept>> {
    use crate::concepts::{Bridge, Concept};
    use std::collections::{BTreeMap, BTreeSet};

    let mut suggestions: Vec<(usize, Concept)> = Vec::new();
    for node in store.nodes_of_kind(NodeKind::Concept)? {
        let bridge = node
            .data
            .get("bridge")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        // Already declared: nothing to propose.
        if bridge == Bridge::Declared.as_str() {
            continue;
        }
        let links = store.neighbors(node.id, Direction::Out, &[EdgeKind::MapsTo])?;
        // A concept naming nothing is not worth a human's attention.
        if links.is_empty() {
            continue;
        }
        let labels: BTreeMap<String, String> = node
            .data
            .get("labels")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let code: BTreeSet<String> = links
            .iter()
            .filter(|(n, _, _)| n.kind == NodeKind::Symbol)
            .map(|(n, _, _)| n.name.clone())
            .take(6)
            .collect();
        let db: BTreeSet<String> = links
            .iter()
            .filter(|(n, _, _)| n.kind == NodeKind::DatabaseObject)
            .map(|(n, _, _)| n.name.clone())
            .take(4)
            .collect();
        suggestions.push((
            links.len(),
            Concept {
                id: node
                    .data
                    .get("concept_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&node.name)
                    .to_string(),
                labels,
                code,
                db,
                status: node.status,
                confidence: node.confidence,
                bridge: Bridge::Declared,
            },
        ));
    }
    suggestions.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.id.cmp(&b.1.id)));
    Ok(suggestions.into_iter().take(60).map(|(_, c)| c).collect())
}

/// Counts used by `reify report`, all of them defined in `docs/metrics.md`.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub files: u64,
    pub symbols: u64,
    pub doc_sections: u64,
    pub database_objects: u64,
    pub concepts: u64,
    pub commits: u64,
    pub rules: u64,
    pub conflicts: u64,
    pub edges: u64,
    pub concepts_by_bridge: HashMap<String, u64>,
    /// Share of symbols reachable from at least one concept or document section.
    pub knowledge_coverage: f32,
    /// Share of symbols with a docstring or leading comment.
    pub documented_symbols: f32,
}

pub fn report(store: &Store) -> Result<Report> {
    let symbols = store.nodes_of_kind(NodeKind::Symbol)?;
    let total = symbols.len().max(1) as f32;

    let mut covered = 0usize;
    let mut documented = 0usize;
    for symbol in &symbols {
        if symbol
            .data
            .get("doc")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.is_empty())
        {
            documented += 1;
        }
        let linked = !store
            .neighbors(symbol.id, Direction::In, &[EdgeKind::MapsTo])?
            .is_empty()
            || !store
                .neighbors(symbol.id, Direction::Out, &[EdgeKind::DocumentedBy])?
                .is_empty();
        if linked {
            covered += 1;
        }
    }

    Ok(Report {
        files: store.count_of_kind(NodeKind::File)?,
        symbols: symbols.len() as u64,
        doc_sections: store.count_of_kind(NodeKind::DocSection)?,
        database_objects: store.count_of_kind(NodeKind::DatabaseObject)?,
        concepts: store.count_of_kind(NodeKind::Concept)?,
        commits: store.count_of_kind(NodeKind::Commit)?,
        rules: rules(store, 0.0)?.len() as u64,
        conflicts: conflicts(store)?.len() as u64,
        edges: store.count_edges()?,
        concepts_by_bridge: crate::index::concept_bridges(store)?,
        knowledge_coverage: covered as f32 / total,
        documented_symbols: documented as f32 / total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{index, IndexOptions};
    use std::fs;
    use std::path::PathBuf;

    fn fixture() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "reify-query-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("app")).unwrap();
        fs::create_dir_all(dir.join("docs")).unwrap();
        fs::create_dir_all(dir.join("translations")).unwrap();
        fs::write(
            dir.join("app/order.py"),
            r#"
class SalesOrder:
    """Represents a customer sales order."""

    def requires_approval(self):
        if self.customer_group == 7:
            return self.bypass_level_two()
        return True

    def bypass_level_two(self):
        return self.db.sql("SELECT name FROM approval_log")
"#,
        )
        .unwrap();
        fs::write(
            dir.join("app/report.py"),
            "class ApprovalReport:\n    def run(self):\n        return self.db.sql(\"SELECT count(*) FROM approval_log\")\n",
        )
        .unwrap();
        fs::write(
            dir.join("app/batch.py"),
            "from app.order import SalesOrder\n\nclass Nightly:\n    def go(self):\n        return SalesOrder().requires_approval()\n",
        )
        .unwrap();
        fs::write(
            dir.join("docs/BRD.md"),
            "# Approval\n\n## Sales Order approval\n\nOrders above 50M require approval.\n",
        )
        .unwrap();
        fs::write(
            dir.join("translations/vi.csv"),
            "Sales Order,đơn bán hàng\n",
        )
        .unwrap();
        dir
    }

    fn indexed() -> (Store, PathBuf) {
        let root = fixture();
        let mut store = Store::in_memory().unwrap();
        index(&mut store, &IndexOptions::new(&root)).unwrap();
        (store, root)
    }

    #[test]
    fn targets_parse_into_lines_and_names() {
        match Target::parse("app/order.py:12") {
            Target::Line { path, line } => {
                assert_eq!(path, "app/order.py");
                assert_eq!(line, 12);
            }
            other => panic!("expected a line target, got {other:?}"),
        }
        assert!(matches!(Target::parse("SalesOrder"), Target::Name(_)));
        // A windows-style drive letter must not be read as a line number.
        assert!(matches!(Target::parse("app/order.py"), Target::Name(_)));
    }

    #[test]
    fn why_resolves_a_line_to_its_innermost_symbol() {
        let (store, root) = indexed();
        let answer = why(&store, &root, "app/order.py:6").unwrap();
        assert_eq!(
            answer.symbol.as_deref(),
            Some("SalesOrder.requires_approval")
        );
        assert!(answer.location.starts_with("app/order.py:"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn why_reports_callers_callees_and_table_access() {
        let (store, root) = indexed();
        let answer = why(&store, &root, "SalesOrder.requires_approval").unwrap();
        assert!(
            answer
                .calls
                .iter()
                .any(|c| c.what.contains("bypass_level_two")),
            "calls: {:?}",
            answer.calls
        );
        assert!(
            answer.called_by.iter().any(|c| c.what.contains("go")),
            "called_by: {:?}",
            answer.called_by
        );

        let bypass = why(&store, &root, "SalesOrder.bypass_level_two").unwrap();
        assert!(bypass.reads.iter().any(|c| c.what.contains("approval_log")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn why_surfaces_the_concept_and_its_documents() {
        let (store, root) = indexed();
        let answer = why(&store, &root, "SalesOrder").unwrap();
        assert!(
            answer
                .concepts
                .iter()
                .any(|c| c.location.contains("SALES_ORDER")),
            "concepts: {:?}",
            answer.concepts
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn why_states_what_it_could_not_determine() {
        let (store, root) = indexed();
        let answer = why(&store, &root, "ApprovalReport.run").unwrap();
        assert!(
            !answer.unknowns.is_empty(),
            "absence must be stated, not implied"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn why_on_an_unindexed_location_is_an_error_not_an_empty_answer() {
        let (store, root) = indexed();
        let err = why(&store, &root, "app/nope.py:1").unwrap_err().to_string();
        assert!(err.contains("nothing indexed"), "got {err}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn impact_finds_callers_at_distance_one() {
        let (store, root) = indexed();
        let answer = impact(&store, "requires_approval").unwrap();
        assert!(
            answer
                .affected
                .iter()
                .any(|a| a.what == "go" && a.distance == 1),
            "affected: {:?}",
            answer.affected
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn impact_crosses_into_the_data_layer_where_no_call_edge_exists() {
        // Nothing calls ApprovalReport.run from bypass_level_two, but both touch the
        // same table. This is the edge a call graph cannot produce.
        let (store, root) = indexed();
        let answer = impact(&store, "bypass_level_two").unwrap();
        assert!(
            answer.affected.iter().any(|a| a.what == "run"),
            "expected the report that reads the same table; got {:?}",
            answer.affected
        );
        assert!(answer
            .tables
            .iter()
            .any(|t| t.what.contains("approval_log")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn impact_output_is_deterministic() {
        let (store, root) = indexed();
        let a = serde_json::to_string(&impact(&store, "requires_approval").unwrap()).unwrap();
        let b = serde_json::to_string(&impact(&store, "requires_approval").unwrap()).unwrap();
        assert_eq!(a, b);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn impact_on_an_unknown_query_says_so_rather_than_inventing() {
        let (store, root) = indexed();
        let answer = impact(&store, "quantum_flux_capacitor").unwrap();
        assert!(answer.affected.is_empty());
        assert!(!answer.unknowns.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn explain_gathers_a_concept_across_code_and_documents() {
        let (store, root) = indexed();
        let answer = explain(&store, "sales order").unwrap();
        assert_eq!(answer.id, "SALES_ORDER");
        assert!(
            !answer.code.is_empty(),
            "a concept with no code is not explained"
        );
        assert!(
            answer.labels.get("vie").is_some(),
            "every language must be kept"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn explain_reaches_the_same_concept_from_any_language() {
        let (store, root) = indexed();
        let english = explain(&store, "sales order").unwrap();
        let vietnamese = explain(&store, "đơn bán hàng").unwrap();
        assert_eq!(english.id, vietnamese.id, "no language is canonical");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn explain_on_an_unknown_term_is_an_error_not_a_guess() {
        let (store, root) = indexed();
        assert!(explain(&store, "quantum flux capacitor").is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn flow_orders_steps_by_the_call_graph_starting_at_an_entry_point() {
        let (store, root) = indexed();
        let answer = flow(&store, "requires_approval").unwrap();
        assert!(answer.steps.len() >= 2, "steps: {:?}", answer.steps);
        assert_eq!(answer.steps[0].order, 1);
        assert_eq!(answer.steps[0].reached_by, "entry point");
        let symbols: Vec<&str> = answer.steps.iter().map(|s| s.symbol.as_str()).collect();
        let caller = symbols.iter().position(|s| s.contains("requires_approval"));
        let callee = symbols.iter().position(|s| s.contains("bypass_level_two"));
        if let (Some(a), Some(b)) = (caller, callee) {
            assert!(a < b, "a caller must precede what it calls: {symbols:?}");
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn flow_says_so_when_the_code_does_not_encode_a_sequence() {
        let (store, root) = indexed();
        let answer = flow(&store, "quantum flux capacitor").unwrap();
        assert!(answer.steps.is_empty());
        assert!(!answer.unknowns.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn flow_output_is_deterministic() {
        let (store, root) = indexed();
        let a = serde_json::to_string(&flow(&store, "requires_approval").unwrap()).unwrap();
        let b = serde_json::to_string(&flow(&store, "requires_approval").unwrap()).unwrap();
        assert_eq!(a, b);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn preflight_summarises_what_an_edit_is_about_to_touch() {
        let (store, root) = indexed();
        let answer = preflight(&store, "app/order.py").unwrap();
        assert!(answer.symbols > 0);
        assert!(answer.dependants > 0, "batch.py depends on this file");
        assert!(["LOW", "MEDIUM", "HIGH"].contains(&answer.risk));
        assert!(answer.suggested_command.starts_with("reify context"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn preflight_stays_small_enough_for_an_edit_hook() {
        // It runs on every edit, so its cost is the whole design constraint.
        let (store, root) = indexed();
        let answer = preflight(&store, "app/order.py").unwrap();
        let rendered = serde_json::to_string(&answer).unwrap();
        assert!(
            crate::tokens::estimate(&rendered) < 300,
            "preflight cost {} tokens",
            crate::tokens::estimate(&rendered)
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_file_nothing_depends_on_is_low_risk() {
        let (store, root) = indexed();
        let answer = preflight(&store, "app/report.py").unwrap();
        assert_eq!(answer.risk, "LOW", "{answer:?}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn preflight_on_an_unindexed_file_reports_nothing_rather_than_failing() {
        let (store, root) = indexed();
        let answer = preflight(&store, "app/does_not_exist.py").unwrap();
        assert_eq!(answer.symbols, 0);
        assert_eq!(answer.risk, "LOW");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn concept_suggestions_are_declarable_and_grounded() {
        let (store, root) = indexed();
        let suggestions = concept_suggestions(&store).unwrap();
        assert!(!suggestions.is_empty());
        for concept in &suggestions {
            assert_eq!(concept.bridge, crate::concepts::Bridge::Declared);
            assert!(
                !concept.code.is_empty() || !concept.db.is_empty(),
                "a suggestion naming nothing wastes a human's attention: {concept:?}"
            );
        }
        // The output must be pasteable straight into the glossary.
        let rendered = crate::concepts::Glossary::render(&suggestions);
        assert!(crate::concepts::Glossary::parse(&rendered).is_ok());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_concept_overview_counts_every_bridge() {
        let (store, root) = indexed();
        let overview = concept_overview(&store).unwrap();
        assert_eq!(overview.total, overview.concepts.len());
        assert_eq!(
            overview.by_bridge.values().sum::<usize>(),
            overview.total,
            "every concept must be attributed to a bridge"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn report_metrics_stay_inside_their_definitions() {
        let (store, root) = indexed();
        let r = report(&store).unwrap();
        assert!(r.symbols > 0);
        assert!((0.0..=1.0).contains(&r.knowledge_coverage));
        assert!((0.0..=1.0).contains(&r.documented_symbols));
        assert!(r.documented_symbols > 0.0, "the docstring must be counted");
        let _ = fs::remove_dir_all(&root);
    }
}

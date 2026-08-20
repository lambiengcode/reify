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

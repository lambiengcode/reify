//! Core knowledge-model types.
//!
//! Everything Reify knows is a [`Node`] or an [`Edge`]. Both carry an epistemic
//! [`Status`] and a confidence, because the product promise is that an agent can tell
//! a parsed fact from a guess. See `docs/PLAN.md` §G.

use serde::{Deserialize, Serialize};
use std::fmt;

/// How much we believe a piece of knowledge, and why.
///
/// Ordering matters: a stronger status always wins when two extractors disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Status {
    /// Explicitly unresolved. Recorded so absence is visible rather than silent.
    ///
    /// This is the `Default` on purpose: anything that forgets to state its epistemic
    /// footing must land on the status an agent is not allowed to act on.
    #[default]
    Unknown,
    /// A default applied because no evidence was available.
    Assumed,
    /// Two sources above threshold disagree.
    Conflicted,
    /// Produced by a heuristic or a language model. Verify before acting.
    Inferred,
    /// Derived deterministically from confirmed facts.
    Observed,
    /// Read directly out of a source artifact.
    Confirmed,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Unknown => "UNKNOWN",
            Status::Assumed => "ASSUMED",
            Status::Conflicted => "CONFLICTED",
            Status::Inferred => "INFERRED",
            Status::Observed => "OBSERVED",
            Status::Confirmed => "CONFIRMED",
        }
    }

    pub fn parse(s: &str) -> Status {
        match s {
            "CONFIRMED" => Status::Confirmed,
            "OBSERVED" => Status::Observed,
            "INFERRED" => Status::Inferred,
            "CONFLICTED" => Status::Conflicted,
            "ASSUMED" => Status::Assumed,
            _ => Status::Unknown,
        }
    }

    /// Whether an agent may act on this without first reading the cited evidence.
    pub fn is_actionable(self) -> bool {
        matches!(self, Status::Confirmed | Status::Observed)
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum NodeKind {
    File,
    Symbol,
    DocSection,
    Concept,
    Commit,
    DatabaseObject,
    BusinessRule,
}

impl NodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            NodeKind::File => "File",
            NodeKind::Symbol => "Symbol",
            NodeKind::DocSection => "DocSection",
            NodeKind::Concept => "Concept",
            NodeKind::Commit => "Commit",
            NodeKind::DatabaseObject => "DatabaseObject",
            NodeKind::BusinessRule => "BusinessRule",
        }
    }

    pub fn parse(s: &str) -> Option<NodeKind> {
        Some(match s {
            "File" => NodeKind::File,
            "Symbol" => NodeKind::Symbol,
            "DocSection" => NodeKind::DocSection,
            "Concept" => NodeKind::Concept,
            "Commit" => NodeKind::Commit,
            "DatabaseObject" => NodeKind::DatabaseObject,
            "BusinessRule" => NodeKind::BusinessRule,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EdgeKind {
    Calls,
    Imports,
    Inherits,
    Reads,
    Writes,
    DocumentedBy,
    MapsTo,
    IntroducedBy,
    ChangedBy,
    CoChangesWith,
    TestedBy,
    ImplementsRule,
    Contradicts,
}

impl EdgeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EdgeKind::Calls => "CALLS",
            EdgeKind::Imports => "IMPORTS",
            EdgeKind::Inherits => "INHERITS",
            EdgeKind::Reads => "READS",
            EdgeKind::Writes => "WRITES",
            EdgeKind::DocumentedBy => "DOCUMENTED_BY",
            EdgeKind::MapsTo => "MAPS_TO",
            EdgeKind::IntroducedBy => "INTRODUCED_BY",
            EdgeKind::ChangedBy => "CHANGED_BY",
            EdgeKind::CoChangesWith => "CO_CHANGES_WITH",
            EdgeKind::TestedBy => "TESTED_BY",
            EdgeKind::ImplementsRule => "IMPLEMENTS_RULE",
            EdgeKind::Contradicts => "CONTRADICTS",
        }
    }

    pub fn parse(s: &str) -> Option<EdgeKind> {
        Some(match s {
            "CALLS" => EdgeKind::Calls,
            "IMPORTS" => EdgeKind::Imports,
            "INHERITS" => EdgeKind::Inherits,
            "READS" => EdgeKind::Reads,
            "WRITES" => EdgeKind::Writes,
            "DOCUMENTED_BY" => EdgeKind::DocumentedBy,
            "MAPS_TO" => EdgeKind::MapsTo,
            "INTRODUCED_BY" => EdgeKind::IntroducedBy,
            "CHANGED_BY" => EdgeKind::ChangedBy,
            "CO_CHANGES_WITH" => EdgeKind::CoChangesWith,
            "TESTED_BY" => EdgeKind::TestedBy,
            "IMPLEMENTS_RULE" => EdgeKind::ImplementsRule,
            "CONTRADICTS" => EdgeKind::Contradicts,
            _ => return None,
        })
    }

    /// Relevance decay applied when the context compiler spreads across this edge.
    ///
    /// These weights are the tuning surface for retrieval quality; they live in one
    /// place so a benchmark regression has exactly one knob to turn.
    pub fn weight(self) -> f32 {
        match self {
            EdgeKind::ImplementsRule => 0.95,
            EdgeKind::MapsTo => 0.90,
            EdgeKind::DocumentedBy => 0.85,
            EdgeKind::Calls => 0.75,
            EdgeKind::Writes => 0.75,
            EdgeKind::Reads => 0.65,
            EdgeKind::Inherits => 0.70,
            EdgeKind::TestedBy => 0.60,
            EdgeKind::Contradicts => 0.95,
            EdgeKind::Imports => 0.35,
            EdgeKind::IntroducedBy => 0.45,
            EdgeKind::ChangedBy => 0.30,
            EdgeKind::CoChangesWith => 0.40,
        }
    }
}

/// The language a source file is written in, or the document format it uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    Python,
    TypeScript,
    JavaScript,
    Sql,
    Markdown,
    Text,
    Csv,
    Json,
    Yaml,
    Other,
}

impl Lang {
    pub fn as_str(self) -> &'static str {
        match self {
            Lang::Python => "python",
            Lang::TypeScript => "typescript",
            Lang::JavaScript => "javascript",
            Lang::Sql => "sql",
            Lang::Markdown => "markdown",
            Lang::Text => "text",
            Lang::Csv => "csv",
            Lang::Json => "json",
            Lang::Yaml => "yaml",
            Lang::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Lang {
        match s {
            "python" => Lang::Python,
            "typescript" => Lang::TypeScript,
            "javascript" => Lang::JavaScript,
            "sql" => Lang::Sql,
            "markdown" => Lang::Markdown,
            "text" => Lang::Text,
            "csv" => Lang::Csv,
            "json" => Lang::Json,
            "yaml" => Lang::Yaml,
            _ => Lang::Other,
        }
    }

    pub fn is_code(self) -> bool {
        matches!(
            self,
            Lang::Python | Lang::TypeScript | Lang::JavaScript | Lang::Sql
        )
    }

    pub fn is_doc(self) -> bool {
        matches!(self, Lang::Markdown | Lang::Text)
    }
}

/// A node staged for insertion. Ids are assigned by the store.
#[derive(Debug, Clone)]
pub struct NewNode {
    pub uid: String,
    pub kind: NodeKind,
    pub name: String,
    pub path: Option<String>,
    pub line_start: u32,
    pub line_end: u32,
    pub lang: Option<Lang>,
    pub status: Status,
    pub confidence: f32,
    /// Free text fed to the full-text index. Never rendered verbatim to an agent.
    pub search_text: String,
    /// Kind-specific payload.
    pub data: serde_json::Value,
}

impl NewNode {
    pub fn new(uid: impl Into<String>, kind: NodeKind, name: impl Into<String>) -> Self {
        NewNode {
            uid: uid.into(),
            kind,
            name: name.into(),
            path: None,
            line_start: 0,
            line_end: 0,
            lang: None,
            status: Status::Confirmed,
            confidence: 1.0,
            search_text: String::new(),
            data: serde_json::Value::Null,
        }
    }

    pub fn at(mut self, path: impl Into<String>, start: u32, end: u32) -> Self {
        self.path = Some(path.into());
        self.line_start = start;
        self.line_end = end;
        self
    }

    pub fn lang(mut self, lang: Lang) -> Self {
        self.lang = Some(lang);
        self
    }

    pub fn status(mut self, status: Status, confidence: f32) -> Self {
        self.status = status;
        self.confidence = confidence;
        self
    }

    pub fn search(mut self, text: impl Into<String>) -> Self {
        self.search_text = text.into();
        self
    }

    pub fn data(mut self, data: serde_json::Value) -> Self {
        self.data = data;
        self
    }
}

/// An edge staged for insertion, addressed by node uid rather than row id.
///
/// Resolving uids late lets extractors emit edges to symbols they have not seen yet,
/// which is the normal case for calls across files.
#[derive(Debug, Clone)]
pub struct NewEdge {
    pub src: String,
    pub dst: String,
    pub kind: EdgeKind,
    pub status: Status,
    pub confidence: f32,
}

impl NewEdge {
    pub fn new(
        src: impl Into<String>,
        dst: impl Into<String>,
        kind: EdgeKind,
        status: Status,
        confidence: f32,
    ) -> Self {
        NewEdge {
            src: src.into(),
            dst: dst.into(),
            kind,
            status,
            confidence,
        }
    }
}

/// A node as read back out of the store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: i64,
    pub uid: String,
    pub kind: NodeKind,
    pub name: String,
    pub path: Option<String>,
    pub line_start: u32,
    pub line_end: u32,
    pub lang: Option<Lang>,
    pub status: Status,
    pub confidence: f32,
    pub tokens: u32,
    pub data: serde_json::Value,
}

impl Node {
    /// `path:line` in the form editors and agents expect.
    pub fn location(&self) -> String {
        match (&self.path, self.line_start) {
            (Some(p), 0) => p.clone(),
            (Some(p), l) => format!("{p}:{l}"),
            (None, _) => self.uid.clone(),
        }
    }
}

/// Stable node identifiers.
///
/// Uids must be stable across reindexing or incremental updates lose their cache and
/// edges dangle, so they never contain line numbers.
pub mod uid {
    pub fn file(path: &str) -> String {
        format!("file:{path}")
    }

    pub fn symbol(path: &str, qualified: &str) -> String {
        format!("sym:{path}#{qualified}")
    }

    pub fn doc_section(path: &str, slug: &str) -> String {
        format!("doc:{path}#{slug}")
    }

    pub fn concept(slug: &str) -> String {
        format!("concept:{slug}")
    }

    pub fn commit(sha: &str) -> String {
        format!("commit:{}", &sha[..sha.len().min(12)])
    }

    pub fn db_object(name: &str) -> String {
        format!("db:{}", name.to_ascii_lowercase())
    }

    pub fn rule(digest: &str) -> String {
        format!("rule:{}", &digest[..digest.len().min(10)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_round_trips_through_string() {
        for s in [
            Status::Unknown,
            Status::Assumed,
            Status::Conflicted,
            Status::Inferred,
            Status::Observed,
            Status::Confirmed,
        ] {
            assert_eq!(Status::parse(s.as_str()), s);
        }
    }

    #[test]
    fn stronger_status_sorts_higher() {
        assert!(Status::Confirmed > Status::Observed);
        assert!(Status::Observed > Status::Inferred);
        assert!(Status::Inferred > Status::Conflicted);
    }

    #[test]
    fn the_default_status_is_the_one_agents_may_not_act_on() {
        assert_eq!(Status::default(), Status::Unknown);
        assert!(!Status::default().is_actionable());
    }

    #[test]
    fn only_deterministic_statuses_are_actionable() {
        assert!(Status::Confirmed.is_actionable());
        assert!(Status::Observed.is_actionable());
        assert!(!Status::Inferred.is_actionable());
        assert!(!Status::Conflicted.is_actionable());
        assert!(!Status::Assumed.is_actionable());
        assert!(!Status::Unknown.is_actionable());
    }

    #[test]
    fn node_and_edge_kinds_round_trip() {
        for k in [
            NodeKind::File,
            NodeKind::Symbol,
            NodeKind::DocSection,
            NodeKind::Concept,
            NodeKind::Commit,
            NodeKind::DatabaseObject,
            NodeKind::BusinessRule,
        ] {
            assert_eq!(NodeKind::parse(k.as_str()), Some(k));
        }
        for k in [
            EdgeKind::Calls,
            EdgeKind::Imports,
            EdgeKind::Inherits,
            EdgeKind::Reads,
            EdgeKind::Writes,
            EdgeKind::DocumentedBy,
            EdgeKind::MapsTo,
            EdgeKind::IntroducedBy,
            EdgeKind::ChangedBy,
            EdgeKind::CoChangesWith,
            EdgeKind::TestedBy,
            EdgeKind::ImplementsRule,
            EdgeKind::Contradicts,
        ] {
            assert_eq!(EdgeKind::parse(k.as_str()), Some(k));
        }
    }

    #[test]
    fn edge_weights_stay_in_unit_range() {
        for k in [EdgeKind::Calls, EdgeKind::Imports, EdgeKind::MapsTo] {
            let w = k.weight();
            assert!(w > 0.0 && w <= 1.0, "{k:?} weight {w} out of range");
        }
    }

    #[test]
    fn uids_never_embed_line_numbers() {
        // Line numbers in a uid would break incremental caching on every edit above
        // the symbol, so this is a load-bearing property rather than a style rule.
        assert_eq!(
            uid::symbol("a/b.py", "Order.total"),
            "sym:a/b.py#Order.total"
        );
        assert_eq!(uid::commit("8a31c2fdeadbeef"), "commit:8a31c2fdeadb");
    }

    #[test]
    fn node_location_formats_like_an_editor_jump() {
        let mut n = Node {
            id: 1,
            uid: "sym:a.py#f".into(),
            kind: NodeKind::Symbol,
            name: "f".into(),
            path: Some("a.py".into()),
            line_start: 12,
            line_end: 20,
            lang: Some(Lang::Python),
            status: Status::Confirmed,
            confidence: 1.0,
            tokens: 10,
            data: serde_json::Value::Null,
        };
        assert_eq!(n.location(), "a.py:12");
        n.line_start = 0;
        assert_eq!(n.location(), "a.py");
    }
}

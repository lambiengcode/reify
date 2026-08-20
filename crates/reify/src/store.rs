//! The knowledge store: one SQLite file holding every node, edge and evidence row.
//!
//! Why SQLite and not a graph database: at the scale Reify targets (roughly 10^5 nodes
//! and 10^6 edges) a covering index on `edges(src, kind)` outperforms a graph engine's
//! traversal machinery, and we get transactions, FTS5 and a single copyable file for
//! free. See `docs/PLAN.md` §P.2.

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::model::{EdgeKind, Lang, NewEdge, NewNode, Node, NodeKind, Status};
use crate::tokens;

/// Bumped whenever the schema changes shape. A store whose version differs from the
/// binary's is rebuilt rather than migrated in place; rebuilds are cheap by design.
pub const SCHEMA_VERSION: i64 = 1;

/// Edge kinds derived from a single file's content, invalidated when its hash changes.
pub const CONTENT_EDGE_KINDS: &[EdgeKind] = &[
    EdgeKind::Calls,
    EdgeKind::Imports,
    EdgeKind::Inherits,
    EdgeKind::Reads,
    EdgeKind::Writes,
    EdgeKind::DocumentedBy,
    EdgeKind::TestedBy,
    EdgeKind::ImplementsRule,
];

/// Edge kinds derived from git history, invalidated when `HEAD` moves.
pub const HISTORY_EDGE_KINDS: &[EdgeKind] = &[
    EdgeKind::IntroducedBy,
    EdgeKind::ChangedBy,
    EdgeKind::CoChangesWith,
];

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS files (
    path   TEXT PRIMARY KEY,
    lang   TEXT NOT NULL,
    hash   TEXT NOT NULL,
    bytes  INTEGER NOT NULL,
    lines  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS nodes (
    id         INTEGER PRIMARY KEY,
    uid        TEXT NOT NULL UNIQUE,
    kind       TEXT NOT NULL,
    name       TEXT NOT NULL,
    path       TEXT,
    line_start INTEGER NOT NULL DEFAULT 0,
    line_end   INTEGER NOT NULL DEFAULT 0,
    lang       TEXT,
    status     TEXT NOT NULL,
    confidence REAL NOT NULL,
    tokens     INTEGER NOT NULL,
    data       TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_nodes_kind ON nodes(kind);
CREATE INDEX IF NOT EXISTS idx_nodes_path ON nodes(path);
CREATE INDEX IF NOT EXISTS idx_nodes_name ON nodes(name);

CREATE TABLE IF NOT EXISTS edges (
    id         INTEGER PRIMARY KEY,
    src        INTEGER NOT NULL,
    dst        INTEGER NOT NULL,
    kind       TEXT NOT NULL,
    status     TEXT NOT NULL,
    confidence REAL NOT NULL,
    UNIQUE(src, dst, kind)
);
CREATE INDEX IF NOT EXISTS idx_edges_src ON edges(src, kind);
CREATE INDEX IF NOT EXISTS idx_edges_dst ON edges(dst, kind);

CREATE TABLE IF NOT EXISTS evidence (
    id      INTEGER PRIMARY KEY,
    node_id INTEGER NOT NULL,
    source  TEXT NOT NULL,
    locator TEXT NOT NULL,
    kind    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_evidence_node ON evidence(node_id);

CREATE TABLE IF NOT EXISTS refs (
    id       INTEGER PRIMARY KEY,
    from_uid TEXT NOT NULL,
    name     TEXT NOT NULL,
    path     TEXT NOT NULL,
    kind     TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_refs_path ON refs(path);
CREATE INDEX IF NOT EXISTS idx_refs_name ON refs(name);

CREATE TABLE IF NOT EXISTS facts (
    id      INTEGER PRIMARY KEY,
    kind    TEXT NOT NULL,
    path    TEXT NOT NULL,
    payload TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_facts_path ON facts(path);
CREATE INDEX IF NOT EXISTS idx_facts_kind ON facts(kind, path);

CREATE VIRTUAL TABLE IF NOT EXISTS search USING fts5(
    uid UNINDEXED,
    body,
    tokenize = "unicode61 remove_diacritics 2"
);
"#;

/// A piece of evidence supporting a node, staged for insertion.
#[derive(Debug, Clone)]
pub struct NewEvidence {
    pub node_uid: String,
    pub source: String,
    pub locator: String,
    pub kind: &'static str,
}

/// Nodes, edges and evidence accumulated by extractors before a single transaction.
///
/// Edges address nodes by uid so an extractor can emit a call to a symbol that has not
/// been parsed yet, which is the normal case for cross-file references.
#[derive(Debug, Default)]
pub struct Batch {
    pub nodes: Vec<NewNode>,
    pub edges: Vec<NewEdge>,
    pub evidence: Vec<NewEvidence>,
}

impl Batch {
    pub fn node(&mut self, node: NewNode) {
        self.nodes.push(node);
    }

    pub fn edge(&mut self, edge: NewEdge) {
        self.edges.push(edge);
    }

    pub fn evidence(&mut self, evidence: NewEvidence) {
        self.evidence.push(evidence);
    }

    pub fn absorb(&mut self, other: Batch) {
        self.nodes.extend(other.nodes);
        self.edges.extend(other.edges);
        self.evidence.extend(other.evidence);
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.edges.is_empty() && self.evidence.is_empty()
    }
}

/// What a commit actually wrote, for reporting and for tests that assert on linkage.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CommitStats {
    pub nodes: usize,
    pub edges: usize,
    /// Edges dropped because one endpoint uid was never inserted. Expected and normal
    /// for calls into third-party libraries; a sharp rise signals a resolver bug.
    pub unresolved_edges: usize,
}

/// A record of one indexed file, used to decide what incremental indexing may skip.
#[derive(Debug, Clone)]
pub struct FileRecord {
    pub path: String,
    pub lang: Lang,
    pub hash: String,
    pub bytes: u64,
    pub lines: u32,
}

pub struct Store {
    conn: Connection,
    path: PathBuf,
}

impl Store {
    /// Open an existing store, or create one if `path` does not exist.
    pub fn open(path: impl AsRef<Path>) -> Result<Store> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating store directory {}", parent.display()))?;
        }
        let conn = Connection::open(&path)
            .with_context(|| format!("opening store {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA).context("applying schema")?;

        let store = Store { conn, path };
        match store.meta("schema_version")? {
            None => store.set_meta("schema_version", &SCHEMA_VERSION.to_string())?,
            Some(v) if v != SCHEMA_VERSION.to_string() => {
                anyhow::bail!(
                    "store schema is v{v} but this binary speaks v{SCHEMA_VERSION}; \
                     run `reify index --force` to rebuild"
                )
            }
            Some(_) => {}
        }
        Ok(store)
    }

    /// Open a store held entirely in memory. Used by tests and by benchmark baselines
    /// that must not touch the working tree.
    pub fn in_memory() -> Result<Store> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        let store = Store {
            conn,
            path: PathBuf::from(":memory:"),
        };
        store.set_meta("schema_version", &SCHEMA_VERSION.to_string())?;
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    // ---- metadata -------------------------------------------------------------

    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    // ---- file tracking --------------------------------------------------------

    pub fn file_hashes(&self) -> Result<HashMap<String, String>> {
        let mut stmt = self.conn.prepare("SELECT path, hash FROM files")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = HashMap::new();
        for row in rows {
            let (p, h) = row?;
            out.insert(p, h);
        }
        Ok(out)
    }

    pub fn files(&self) -> Result<Vec<FileRecord>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, lang, hash, bytes, lines FROM files ORDER BY path")?;
        let rows = stmt.query_map([], |r| {
            Ok(FileRecord {
                path: r.get(0)?,
                lang: Lang::parse(&r.get::<_, String>(1)?),
                hash: r.get(2)?,
                bytes: r.get::<_, i64>(3)? as u64,
                lines: r.get::<_, i64>(4)? as u32,
            })
        })?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    pub fn upsert_file(&self, rec: &FileRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO files(path, lang, hash, bytes, lines) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(path) DO UPDATE SET
                lang = excluded.lang, hash = excluded.hash,
                bytes = excluded.bytes, lines = excluded.lines",
            params![
                rec.path,
                rec.lang.as_str(),
                rec.hash,
                rec.bytes as i64,
                rec.lines as i64
            ],
        )?;
        Ok(())
    }

    /// Discard everything derived from one file's *content*, keeping what other stages
    /// own.
    ///
    /// This is the incremental-update primitive, and the reason incremental indexing
    /// produces the same store as a full rebuild. Each pipeline stage owns a disjoint
    /// set of edge kinds and invalidates on its own schedule:
    ///
    /// | stage    | edge kinds                                        | invalidated when |
    /// |----------|---------------------------------------------------|------------------|
    /// | content  | CALLS IMPORTS INHERITS READS WRITES DOCUMENTED_BY TESTED_BY | the file's hash changes |
    /// | concepts | MAPS_TO                                           | every run (global rebuild) |
    /// | history  | INTRODUCED_BY CHANGED_BY CO_CHANGES_WITH          | `HEAD` moves |
    ///
    /// A file edit must not destroy history edges that the git stage will not rebuild,
    /// so the `File` node and its history edges survive here.
    pub fn forget_file_content(&self, path: &str) -> Result<()> {
        let content_kinds = CONTENT_EDGE_KINDS
            .iter()
            .map(|k| format!("'{}'", k.as_str()))
            .collect::<Vec<_>>()
            .join(",");

        // Edges touching a derived node of this file: all of them go with the node.
        self.conn.execute(
            "DELETE FROM edges WHERE
                src IN (SELECT id FROM nodes WHERE path = ?1 AND kind <> 'File')
             OR dst IN (SELECT id FROM nodes WHERE path = ?1 AND kind <> 'File')",
            params![path],
        )?;
        // Content edges anchored on the surviving File node.
        self.conn.execute(
            &format!(
                "DELETE FROM edges WHERE kind IN ({content_kinds}) AND (
                    src IN (SELECT id FROM nodes WHERE path = ?1 AND kind = 'File')
                 OR dst IN (SELECT id FROM nodes WHERE path = ?1 AND kind = 'File'))"
            ),
            params![path],
        )?;
        self.conn.execute(
            "DELETE FROM evidence WHERE node_id IN
                (SELECT id FROM nodes WHERE path = ?1 AND kind <> 'File')",
            params![path],
        )?;
        self.conn.execute(
            "DELETE FROM search WHERE uid IN
                (SELECT uid FROM nodes WHERE path = ?1 AND kind <> 'File')",
            params![path],
        )?;
        self.conn.execute(
            "DELETE FROM nodes WHERE path = ?1 AND kind <> 'File'",
            params![path],
        )?;
        self.conn
            .execute("DELETE FROM refs WHERE path = ?1", params![path])?;
        self.conn
            .execute("DELETE FROM facts WHERE path = ?1", params![path])?;
        Ok(())
    }

    /// Remove a file entirely: it is gone from the working tree.
    pub fn forget_file(&self, path: &str) -> Result<()> {
        self.forget_file_content(path)?;
        self.conn.execute(
            "DELETE FROM edges WHERE
                src IN (SELECT id FROM nodes WHERE path = ?1)
             OR dst IN (SELECT id FROM nodes WHERE path = ?1)",
            params![path],
        )?;
        self.conn.execute(
            "DELETE FROM search WHERE uid IN (SELECT uid FROM nodes WHERE path = ?1)",
            params![path],
        )?;
        self.conn
            .execute("DELETE FROM nodes WHERE path = ?1", params![path])?;
        self.conn
            .execute("DELETE FROM files WHERE path = ?1", params![path])?;
        Ok(())
    }

    /// Drop everything the concept stage owns, before it rebuilds globally.
    pub fn forget_concepts(&self) -> Result<()> {
        self.conn.execute(
            "DELETE FROM edges WHERE kind = ?1",
            params![EdgeKind::MapsTo.as_str()],
        )?;
        self.conn.execute(
            "DELETE FROM search WHERE uid IN (SELECT uid FROM nodes WHERE kind = 'Concept')",
            [],
        )?;
        self.conn
            .execute("DELETE FROM nodes WHERE kind = 'Concept'", [])?;
        Ok(())
    }

    /// Drop everything the rule stage owns, before it rebuilds globally.
    ///
    /// Rules are rebuilt whole on every run rather than patched, because corroboration
    /// and conflict detection are repository-wide: deleting a file must be able to
    /// *lower* a surviving rule's confidence, which an incremental upsert cannot do.
    pub fn forget_rules(&self) -> Result<()> {
        self.conn.execute(
            "DELETE FROM edges WHERE kind = ?1",
            params![EdgeKind::ImplementsRule.as_str()],
        )?;
        self.conn.execute(
            "DELETE FROM evidence WHERE node_id IN
                (SELECT id FROM nodes WHERE kind = 'BusinessRule')",
            [],
        )?;
        self.conn.execute(
            "DELETE FROM search WHERE uid IN
                (SELECT uid FROM nodes WHERE kind = 'BusinessRule')",
            [],
        )?;
        self.conn
            .execute("DELETE FROM nodes WHERE kind = 'BusinessRule'", [])?;
        Ok(())
    }

    /// Persist per-file facts of one kind, so a repository-wide stage can rebuild
    /// from them without re-reading or re-parsing unchanged files.
    ///
    /// This is what lets stages whose output depends on the *whole* repository —
    /// rule corroboration, concept merging, conflict detection — be rebuilt from
    /// scratch on every run while still only parsing what changed.
    pub fn put_facts(&mut self, kind: &str, path: &str, payloads: &[String]) -> Result<()> {
        if payloads.is_empty() {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        {
            let mut stmt =
                tx.prepare("INSERT INTO facts(kind, path, payload) VALUES (?1, ?2, ?3)")?;
            for payload in payloads {
                stmt.execute(params![kind, path, payload])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Every stored fact of one kind, in a stable order.
    pub fn all_facts(&self, kind: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT payload FROM facts WHERE kind = ?1 ORDER BY path, payload")?;
        let rows = stmt.query_map(params![kind], |r| r.get::<_, String>(0))?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    /// Drop everything the history stage owns, before it rebuilds from a new `HEAD`.
    pub fn forget_history(&self) -> Result<()> {
        for kind in HISTORY_EDGE_KINDS {
            self.conn
                .execute("DELETE FROM edges WHERE kind = ?1", params![kind.as_str()])?;
        }
        self.conn.execute(
            "DELETE FROM edges WHERE
                src IN (SELECT id FROM nodes WHERE kind = 'Commit')
             OR dst IN (SELECT id FROM nodes WHERE kind = 'Commit')",
            [],
        )?;
        self.conn.execute(
            "DELETE FROM search WHERE uid IN (SELECT uid FROM nodes WHERE kind = 'Commit')",
            [],
        )?;
        self.conn
            .execute("DELETE FROM nodes WHERE kind = 'Commit'", [])?;
        Ok(())
    }

    // ---- unresolved references ------------------------------------------------

    /// Persist references that could not be resolved when the file was parsed.
    ///
    /// Keeping them means an incremental run can re-resolve *every* reference in the
    /// repository against the new symbol table. Without this, editing file B would
    /// permanently lose the `A -> B` edges that unchanged file A contributed.
    pub fn put_refs(&mut self, refs: &[(String, String, String, EdgeKind)]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO refs(from_uid, name, path, kind) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for (from_uid, name, path, kind) in refs {
                stmt.execute(params![from_uid, name, path, kind.as_str()])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Every stored reference, for a repository-wide re-resolve.
    pub fn all_refs(&self) -> Result<Vec<(String, String, String, EdgeKind)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT from_uid, name, path, kind FROM refs")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                EdgeKind::parse(&r.get::<_, String>(3)?).unwrap_or(EdgeKind::Calls),
            ))
        })?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    /// `(name, uid, path, lang)` for every symbol, to rebuild the resolver index.
    pub fn symbol_triples(&self) -> Result<Vec<(String, String, String, Lang)>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, uid, COALESCE(path, ''), COALESCE(lang, 'other')
             FROM nodes WHERE kind = 'Symbol'",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                Lang::parse(&r.get::<_, String>(3)?),
            ))
        })?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    /// `(name, uid)` for every symbol and database object, to rebuild the grounding
    /// index the concept layer needs.
    pub fn groundable_names(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, uid FROM nodes WHERE kind IN ('Symbol', 'DatabaseObject')",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    /// A stable textual dump of the whole store, for equivalence testing.
    ///
    /// Excludes row ids and anything timestamp-derived, so two stores built by
    /// different routes to the same state compare equal.
    pub fn canonical_dump(&self) -> Result<String> {
        let mut out = String::new();
        let mut stmt = self.conn.prepare(
            "SELECT uid, kind, name, COALESCE(path,''), line_start, line_end,
                    status, ROUND(confidence, 4), data
             FROM nodes ORDER BY uid",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(format!(
                "N\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, f64>(7)?,
                r.get::<_, String>(8)?,
            ))
        })?;
        for row in rows {
            out.push_str(&row?);
            out.push('\n');
        }
        let mut stmt = self.conn.prepare(
            "SELECT s.uid, d.uid, e.kind, e.status, ROUND(e.confidence, 4)
             FROM edges e JOIN nodes s ON s.id = e.src JOIN nodes d ON d.id = e.dst
             ORDER BY s.uid, d.uid, e.kind",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(format!(
                "E\t{}\t{}\t{}\t{}\t{}",
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, f64>(4)?,
            ))
        })?;
        for row in rows {
            out.push_str(&row?);
            out.push('\n');
        }
        Ok(out)
    }

    // ---- writing --------------------------------------------------------------

    /// Insert a batch in one transaction, resolving edge endpoints by uid.
    ///
    /// Endpoints that do not resolve are counted, not silently dropped: a jump in
    /// `unresolved_edges` is the signal that a resolver has regressed.
    pub fn commit(&mut self, batch: Batch) -> Result<CommitStats> {
        let tx = self.conn.transaction()?;
        let mut stats = CommitStats::default();

        {
            let mut insert_node = tx.prepare(
                "INSERT INTO nodes(uid, kind, name, path, line_start, line_end,
                                   lang, status, confidence, tokens, data)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(uid) DO UPDATE SET
                    kind = excluded.kind, name = excluded.name, path = excluded.path,
                    line_start = excluded.line_start, line_end = excluded.line_end,
                    lang = excluded.lang, tokens = excluded.tokens, data = excluded.data,
                    -- keep the strongest claim we have ever had about this node
                    status = CASE WHEN excluded.confidence >= nodes.confidence
                                  THEN excluded.status ELSE nodes.status END,
                    confidence = MAX(excluded.confidence, nodes.confidence)",
            )?;
            let mut insert_search =
                tx.prepare("INSERT INTO search(uid, body) VALUES (?1, ?2)")?;
            let mut clear_search = tx.prepare("DELETE FROM search WHERE uid = ?1")?;

            for n in &batch.nodes {
                let rendered = render_cost_preview(n);
                insert_node.execute(params![
                    n.uid,
                    n.kind.as_str(),
                    n.name,
                    n.path,
                    n.line_start,
                    n.line_end,
                    n.lang.map(|l| l.as_str()),
                    n.status.as_str(),
                    n.confidence,
                    tokens::estimate(&rendered),
                    serde_json::to_string(&n.data)?,
                ])?;
                if !n.search_text.is_empty() {
                    clear_search.execute(params![n.uid])?;
                    insert_search.execute(params![n.uid, n.search_text])?;
                }
                stats.nodes += 1;
            }
        }

        {
            let mut lookup =
                tx.prepare("SELECT id FROM nodes WHERE uid = ?1")?;
            let mut insert_edge = tx.prepare(
                "INSERT INTO edges(src, dst, kind, status, confidence)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(src, dst, kind) DO UPDATE SET
                    confidence = MAX(excluded.confidence, edges.confidence)",
            )?;
            // Resolving through a local cache avoids one query per endpoint on repos
            // where a handful of symbols are referenced tens of thousands of times.
            let mut cache: HashMap<String, Option<i64>> = HashMap::new();
            let mut resolve = |uid: &str, cache: &mut HashMap<String, Option<i64>>| -> Result<Option<i64>> {
                if let Some(hit) = cache.get(uid) {
                    return Ok(*hit);
                }
                let id: Option<i64> = lookup
                    .query_row(params![uid], |r| r.get(0))
                    .optional()?;
                cache.insert(uid.to_string(), id);
                Ok(id)
            };

            for e in &batch.edges {
                let (Some(src), Some(dst)) =
                    (resolve(&e.src, &mut cache)?, resolve(&e.dst, &mut cache)?)
                else {
                    stats.unresolved_edges += 1;
                    continue;
                };
                if src == dst && e.kind != EdgeKind::Calls {
                    continue; // self-edges carry no information except for recursion
                }
                insert_edge.execute(params![
                    src,
                    dst,
                    e.kind.as_str(),
                    e.status.as_str(),
                    e.confidence
                ])?;
                stats.edges += 1;
            }

            let mut insert_ev = tx.prepare(
                "INSERT INTO evidence(node_id, source, locator, kind)
                 VALUES (?1, ?2, ?3, ?4)",
            )?;
            for ev in &batch.evidence {
                if let Some(id) = resolve(&ev.node_uid, &mut cache)? {
                    insert_ev.execute(params![id, ev.source, ev.locator, ev.kind])?;
                }
            }
        }

        tx.commit()?;
        Ok(stats)
    }

    // ---- reading --------------------------------------------------------------

    pub fn node_by_uid(&self, uid: &str) -> Result<Option<Node>> {
        Ok(self
            .conn
            .query_row(&format!("{NODE_SELECT} WHERE uid = ?1"), params![uid], row_to_node)
            .optional()?)
    }

    pub fn node_by_id(&self, id: i64) -> Result<Option<Node>> {
        Ok(self
            .conn
            .query_row(&format!("{NODE_SELECT} WHERE id = ?1"), params![id], row_to_node)
            .optional()?)
    }

    pub fn nodes_of_kind(&self, kind: NodeKind) -> Result<Vec<Node>> {
        let mut stmt = self
            .conn
            .prepare(&format!("{NODE_SELECT} WHERE kind = ?1 ORDER BY name"))?;
        let rows = stmt.query_map(params![kind.as_str()], row_to_node)?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    pub fn count_of_kind(&self, kind: NodeKind) -> Result<u64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM nodes WHERE kind = ?1",
            params![kind.as_str()],
            |r| r.get::<_, i64>(0),
        )? as u64)
    }

    pub fn count_edges(&self) -> Result<u64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get::<_, i64>(0))?
            as u64)
    }

    pub fn count_edges_of_kind(&self, kind: EdgeKind) -> Result<u64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM edges WHERE kind = ?1",
            params![kind.as_str()],
            |r| r.get::<_, i64>(0),
        )? as u64)
    }

    /// The innermost symbol whose span covers `line` in `path`.
    pub fn symbol_at(&self, path: &str, line: u32) -> Result<Option<Node>> {
        Ok(self
            .conn
            .query_row(
                &format!(
                    "{NODE_SELECT} WHERE kind = 'Symbol' AND path = ?1
                       AND line_start <= ?2 AND line_end >= ?2
                     ORDER BY (line_end - line_start) ASC LIMIT 1"
                ),
                params![path, line],
                row_to_node,
            )
            .optional()?)
    }

    pub fn symbols_in_file(&self, path: &str) -> Result<Vec<Node>> {
        let mut stmt = self.conn.prepare(&format!(
            "{NODE_SELECT} WHERE kind = 'Symbol' AND path = ?1 ORDER BY line_start"
        ))?;
        let rows = stmt.query_map(params![path], row_to_node)?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    /// Symbols whose bare name matches, across the repository.
    pub fn symbols_named(&self, name: &str) -> Result<Vec<Node>> {
        let mut stmt = self.conn.prepare(&format!(
            "{NODE_SELECT} WHERE kind = 'Symbol' AND name = ?1 LIMIT 64"
        ))?;
        let rows = stmt.query_map(params![name], row_to_node)?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    /// Neighbours of `id` along `dir`, optionally restricted to given edge kinds.
    pub fn neighbors(
        &self,
        id: i64,
        dir: Direction,
        kinds: &[EdgeKind],
    ) -> Result<Vec<(Node, EdgeKind, f32)>> {
        let (from_col, to_col) = match dir {
            Direction::Out => ("src", "dst"),
            Direction::In => ("dst", "src"),
        };
        let filter = if kinds.is_empty() {
            String::new()
        } else {
            let list = kinds
                .iter()
                .map(|k| format!("'{}'", k.as_str()))
                .collect::<Vec<_>>()
                .join(",");
            format!(" AND e.kind IN ({list})")
        };
        let sql = format!(
            "SELECT n.id, n.uid, n.kind, n.name, n.path, n.line_start, n.line_end,
                    n.lang, n.status, n.confidence, n.tokens, n.data, e.kind, e.confidence
             FROM edges e JOIN nodes n ON n.id = e.{to_col}
             WHERE e.{from_col} = ?1{filter}"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![id], |r| {
            let node = row_to_node(r)?;
            let kind = EdgeKind::parse(&r.get::<_, String>(12)?).unwrap_or(EdgeKind::Calls);
            Ok((node, kind, r.get::<_, f64>(13)? as f32))
        })?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    /// Full-text search over node bodies, returning `(node, bm25_score)` best first.
    ///
    /// FTS5 reports bm25 as a negative number where more negative is better; it is
    /// negated here so every score in Reify means "higher is more relevant".
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<(Node, f32)>> {
        let Some(expr) = fts_expression(query) else {
            return Ok(Vec::new());
        };
        let sql = format!(
            "SELECT n.id, n.uid, n.kind, n.name, n.path, n.line_start, n.line_end,
                    n.lang, n.status, n.confidence, n.tokens, n.data, bm25(search)
             FROM search JOIN nodes n ON n.uid = search.uid
             WHERE search MATCH ?1 ORDER BY bm25(search) LIMIT ?2"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![expr, limit as i64], |r| {
            let node = row_to_node(r)?;
            Ok((node, -(r.get::<_, f64>(12)? as f32)))
        })?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    pub fn evidence_for(&self, node_id: i64) -> Result<Vec<(String, String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT source, locator, kind FROM evidence WHERE node_id = ?1")?;
        let rows = stmt.query_map(params![node_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    /// Run `ANALYZE` so the planner has statistics. Called once after a full index.
    pub fn optimize(&self) -> Result<()> {
        self.conn.execute_batch("ANALYZE; PRAGMA optimize;")?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Edges where the node is the source: what it depends on.
    Out,
    /// Edges where the node is the target: what depends on it.
    In,
}

const NODE_SELECT: &str = "SELECT id, uid, kind, name, path, line_start, line_end,
                                  lang, status, confidence, tokens, data FROM nodes";

fn row_to_node(r: &rusqlite::Row<'_>) -> rusqlite::Result<Node> {
    let data: String = r.get(11)?;
    Ok(Node {
        id: r.get(0)?,
        uid: r.get(1)?,
        kind: NodeKind::parse(&r.get::<_, String>(2)?).unwrap_or(NodeKind::File),
        name: r.get(3)?,
        path: r.get(4)?,
        line_start: r.get::<_, i64>(5)? as u32,
        line_end: r.get::<_, i64>(6)? as u32,
        lang: r.get::<_, Option<String>>(7)?.map(|s| Lang::parse(&s)),
        status: Status::parse(&r.get::<_, String>(8)?),
        confidence: r.get::<_, f64>(9)? as f32,
        tokens: r.get::<_, i64>(10)? as u32,
        data: serde_json::from_str(&data).unwrap_or(serde_json::Value::Null),
    })
}

/// Turn a free-text query into an FTS5 expression.
///
/// User queries contain quotes, hyphens and punctuation that FTS5 reads as operators,
/// so every term is quoted and joined with OR. Returns `None` when nothing usable is
/// left, which the caller treats as "no lexical hits" rather than as an error.
fn fts_expression(query: &str) -> Option<String> {
    let terms: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"", t.replace('"', "")))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" OR "))
    }
}

/// Approximate what a node costs when rendered into agent-facing context.
///
/// Kept next to the store because the cost is written at insert time: the context
/// compiler must be able to run its budget knapsack without re-rendering candidates.
fn render_cost_preview(n: &NewNode) -> String {
    let mut s = String::with_capacity(96);
    s.push_str(n.kind.as_str());
    s.push(' ');
    s.push_str(&n.name);
    if let Some(p) = &n.path {
        s.push(' ');
        s.push_str(p);
    }
    if let Some(summary) = n.data.get("summary").and_then(|v| v.as_str()) {
        s.push(' ');
        s.push_str(summary);
    }
    if let Some(claim) = n.data.get("claim").and_then(|v| v.as_str()) {
        s.push(' ');
        s.push_str(claim);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::uid;

    fn sym(path: &str, name: &str) -> NewNode {
        NewNode::new(uid::symbol(path, name), NodeKind::Symbol, name)
            .at(path, 1, 10)
            .lang(Lang::Python)
            .search(name.to_string())
    }

    #[test]
    fn commit_inserts_nodes_and_resolves_edges() {
        let mut s = Store::in_memory().unwrap();
        let mut b = Batch::default();
        b.node(sym("a.py", "alpha"));
        b.node(sym("b.py", "beta"));
        b.edge(NewEdge::new(
            uid::symbol("a.py", "alpha"),
            uid::symbol("b.py", "beta"),
            EdgeKind::Calls,
            Status::Observed,
            0.9,
        ));
        let stats = s.commit(b).unwrap();
        assert_eq!(stats.nodes, 2);
        assert_eq!(stats.edges, 1);
        assert_eq!(stats.unresolved_edges, 0);
    }

    #[test]
    fn unresolved_edges_are_counted_not_silently_dropped() {
        let mut s = Store::in_memory().unwrap();
        let mut b = Batch::default();
        b.node(sym("a.py", "alpha"));
        b.edge(NewEdge::new(
            uid::symbol("a.py", "alpha"),
            uid::symbol("vendor.py", "third_party"),
            EdgeKind::Calls,
            Status::Observed,
            0.5,
        ));
        let stats = s.commit(b).unwrap();
        assert_eq!(stats.edges, 0);
        assert_eq!(stats.unresolved_edges, 1);
    }

    #[test]
    fn recommitting_the_same_node_keeps_the_strongest_claim() {
        let mut s = Store::in_memory().unwrap();
        let mut b = Batch::default();
        b.node(sym("a.py", "alpha").status(Status::Observed, 0.6));
        s.commit(b).unwrap();
        let mut b = Batch::default();
        b.node(sym("a.py", "alpha").status(Status::Inferred, 0.3));
        s.commit(b).unwrap();
        let n = s.node_by_uid(&uid::symbol("a.py", "alpha")).unwrap().unwrap();
        assert_eq!(n.status, Status::Observed);
        assert!((n.confidence - 0.6).abs() < 1e-6);
    }

    #[test]
    fn forgetting_content_keeps_history_edges_that_another_stage_owns() {
        // A file edit must not destroy edges the git stage will not rebuild, or an
        // incremental index silently diverges from a full one.
        let mut s = Store::in_memory().unwrap();
        let mut b = Batch::default();
        b.node(NewNode::new(uid::file("a.py"), NodeKind::File, "a.py").at("a.py", 0, 0));
        b.node(NewNode::new(uid::commit("abcdef123456"), NodeKind::Commit, "abcdef1"));
        b.node(sym("a.py", "alpha"));
        s.commit(b).unwrap();
        let mut b = Batch::default();
        b.edge(NewEdge::new(
            uid::file("a.py"),
            uid::commit("abcdef123456"),
            EdgeKind::ChangedBy,
            Status::Confirmed,
            1.0,
        ));
        b.edge(NewEdge::new(
            uid::file("a.py"),
            uid::symbol("a.py", "alpha"),
            EdgeKind::Imports,
            Status::Observed,
            0.9,
        ));
        s.commit(b).unwrap();
        assert_eq!(s.count_edges().unwrap(), 2);

        s.forget_file_content("a.py").unwrap();

        assert_eq!(s.count_edges_of_kind(EdgeKind::ChangedBy).unwrap(), 1, "history survives");
        assert_eq!(s.count_edges_of_kind(EdgeKind::Imports).unwrap(), 0, "content is purged");
        assert!(s.node_by_uid(&uid::file("a.py")).unwrap().is_some(), "the file node survives");
        assert!(s.node_by_uid(&uid::symbol("a.py", "alpha")).unwrap().is_none());
    }

    #[test]
    fn facts_are_scoped_by_kind_and_invalidated_with_their_file() {
        let mut s = Store::in_memory().unwrap();
        s.put_facts("rule", "a.py", &["r1".into(), "r2".into()]).unwrap();
        s.put_facts("concept", "a.py", &["c1".into()]).unwrap();
        s.put_facts("rule", "b.py", &["r3".into()]).unwrap();
        assert_eq!(s.all_facts("rule").unwrap(), vec!["r1", "r2", "r3"]);
        assert_eq!(s.all_facts("concept").unwrap(), vec!["c1"]);

        s.forget_file_content("a.py").unwrap();
        assert_eq!(s.all_facts("rule").unwrap(), vec!["r3"]);
        assert!(s.all_facts("concept").unwrap().is_empty());
    }

    #[test]
    fn refs_survive_until_their_file_changes_then_are_replaced() {
        let mut s = Store::in_memory().unwrap();
        s.put_refs(&[(
            "sym:a.py#f".into(),
            "helper".into(),
            "a.py".into(),
            EdgeKind::Calls,
        )])
        .unwrap();
        assert_eq!(s.all_refs().unwrap().len(), 1);
        s.forget_file_content("a.py").unwrap();
        assert!(s.all_refs().unwrap().is_empty());
    }

    #[test]
    fn canonical_dump_is_stable_and_order_independent() {
        let build = |order: [&str; 2]| {
            let mut s = Store::in_memory().unwrap();
            let mut b = Batch::default();
            for name in order {
                b.node(sym("a.py", name));
            }
            s.commit(b).unwrap();
            s.canonical_dump().unwrap()
        };
        assert_eq!(build(["alpha", "beta"]), build(["beta", "alpha"]));
    }

    #[test]
    fn forget_file_removes_nodes_edges_and_search_rows() {
        let mut s = Store::in_memory().unwrap();
        let mut b = Batch::default();
        b.node(sym("a.py", "alpha"));
        b.node(sym("b.py", "beta"));
        b.edge(NewEdge::new(
            uid::symbol("a.py", "alpha"),
            uid::symbol("b.py", "beta"),
            EdgeKind::Calls,
            Status::Observed,
            0.9,
        ));
        s.commit(b).unwrap();
        s.upsert_file(&FileRecord {
            path: "a.py".into(),
            lang: Lang::Python,
            hash: "h".into(),
            bytes: 1,
            lines: 1,
        })
        .unwrap();

        s.forget_file("a.py").unwrap();

        assert!(s.node_by_uid(&uid::symbol("a.py", "alpha")).unwrap().is_none());
        assert_eq!(s.count_edges().unwrap(), 0);
        assert!(s.search("alpha", 10).unwrap().is_empty());
        assert!(s.file_hashes().unwrap().is_empty());
        // The untouched file must survive.
        assert!(s.node_by_uid(&uid::symbol("b.py", "beta")).unwrap().is_some());
    }

    #[test]
    fn neighbors_respect_direction_and_kind_filter() {
        let mut s = Store::in_memory().unwrap();
        let mut b = Batch::default();
        b.node(sym("a.py", "alpha"));
        b.node(sym("b.py", "beta"));
        b.edge(NewEdge::new(
            uid::symbol("a.py", "alpha"),
            uid::symbol("b.py", "beta"),
            EdgeKind::Calls,
            Status::Observed,
            0.9,
        ));
        s.commit(b).unwrap();
        let alpha = s.node_by_uid(&uid::symbol("a.py", "alpha")).unwrap().unwrap();
        let beta = s.node_by_uid(&uid::symbol("b.py", "beta")).unwrap().unwrap();

        let out = s.neighbors(alpha.id, Direction::Out, &[]).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0.name, "beta");

        let incoming = s.neighbors(beta.id, Direction::In, &[]).unwrap();
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].0.name, "alpha");

        let filtered = s
            .neighbors(alpha.id, Direction::Out, &[EdgeKind::Imports])
            .unwrap();
        assert!(filtered.is_empty());
    }

    #[test]
    fn symbol_at_returns_the_innermost_span() {
        let mut s = Store::in_memory().unwrap();
        let mut b = Batch::default();
        b.node(
            NewNode::new(uid::symbol("a.py", "Outer"), NodeKind::Symbol, "Outer").at("a.py", 1, 100),
        );
        b.node(
            NewNode::new(uid::symbol("a.py", "Outer.inner"), NodeKind::Symbol, "inner")
                .at("a.py", 10, 20),
        );
        s.commit(b).unwrap();
        let hit = s.symbol_at("a.py", 15).unwrap().unwrap();
        assert_eq!(hit.name, "inner");
        let hit = s.symbol_at("a.py", 50).unwrap().unwrap();
        assert_eq!(hit.name, "Outer");
        assert!(s.symbol_at("a.py", 500).unwrap().is_none());
    }

    #[test]
    fn search_survives_punctuation_that_fts5_treats_as_operators() {
        let mut s = Store::in_memory().unwrap();
        let mut b = Batch::default();
        b.node(sym("a.py", "approval").search("requires L2 approval for corporate orders"));
        s.commit(b).unwrap();
        // A raw query like this is an FTS5 syntax error if passed through unescaped.
        let hits = s.search("\"approval\" - corporate (orders)", 10).unwrap();
        assert!(!hits.is_empty(), "punctuation must not break lexical search");
    }

    #[test]
    fn search_ignores_diacritics_so_vietnamese_matches_either_way() {
        let mut s = Store::in_memory().unwrap();
        let mut b = Batch::default();
        b.node(sym("a.py", "kh").search("khách hàng chiến lược"));
        s.commit(b).unwrap();
        assert!(!s.search("khach hang", 10).unwrap().is_empty());
        assert!(!s.search("khách hàng", 10).unwrap().is_empty());
    }

    #[test]
    fn search_returns_nothing_for_a_query_with_no_usable_terms() {
        let s = Store::in_memory().unwrap();
        assert!(s.search("--- !!! ---", 10).unwrap().is_empty());
    }

    #[test]
    fn schema_version_mismatch_is_reported_not_papered_over() {
        let dir = std::env::temp_dir().join(format!("reify-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let db = dir.join("db.sqlite");
        {
            let s = Store::open(&db).unwrap();
            s.set_meta("schema_version", "999").unwrap();
        }
        let err = match Store::open(&db) {
            Ok(_) => panic!("a version mismatch must not open silently"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("--force"), "unexpected error: {err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

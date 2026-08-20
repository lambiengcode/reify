//! SQL and data-layer extraction.
//!
//! Table access is the edge kind that `grep` cannot produce and that decides blast
//! radius in business systems: a report that reads the column a service writes is
//! affected by a change to that service, and nothing in the call graph says so.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use regex::Regex;

use super::FileExtract;
use crate::model::{uid, EdgeKind, Lang, NewEdge, NewNode, NodeKind, Status};

/// How a statement touches a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Access {
    Read,
    Write,
}

/// One table reference found in SQL text.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TableRef {
    pub table: String,
    pub access: Access,
}

/// One identifier: bare, or quoted with backticks, double quotes or brackets.
///
/// Quoted forms matter more than they look: Frappe and ERPNext name every table
/// `` `tabSales Order` ``, with a space, which a bare-identifier pattern misses entirely.
const IDENT: &str = r#"(?:`[^`]+`|"[^"]+"|\[[^\]]+\]|[A-Za-z_][A-Za-z0-9_$]*)"#;

fn read_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r#"(?i)\b(?:from|join)\s+({IDENT}(?:\s*\.\s*{IDENT})*)"#
        ))
        .expect("read regex is a compile-time constant")
    })
}

fn write_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r#"(?i)\b(?:insert\s+into|update|delete\s+from|create\s+table(?:\s+if\s+not\s+exists)?|alter\s+table|truncate\s+table)\s+({IDENT}(?:\s*\.\s*{IDENT})*)"#
        ))
        .expect("write regex is a compile-time constant")
    })
}

/// Words that can follow `FROM`/`JOIN` but never name a table.
///
/// The SQL keywords avoid parsing artefacts. The English determiners matter for a
/// different reason: the cheap prefilter deliberately over-accepts, so ordinary prose
/// such as "select a plan from the list" reaches this point and must produce nothing.
const NOT_A_TABLE: &[&str] = &[
    // SQL
    "select", "where", "dual", "lateral", "unnest", "values", "set", "table", "only", "distinct",
    "all", "as", "on", "using", "inner", "outer", "left", "right", "full", "cross", "natural",
    // English determiners and common prose nouns
    "the", "a", "an", "this", "that", "these", "those", "it", "its", "my", "our", "your", "their",
    "his", "her", "list", "each", "any", "some", "which", "what", "here", "there", "them", "us",
    "me", "you", "we", "they", "he", "she",
];

/// Find every table reference in a block of SQL text.
pub fn table_refs(sql: &str) -> Vec<TableRef> {
    let mut found: BTreeSet<TableRef> = BTreeSet::new();
    for caps in write_re().captures_iter(sql) {
        if let Some(name) = normalize(&caps[1]) {
            found.insert(TableRef {
                table: name,
                access: Access::Write,
            });
        }
    }
    for caps in read_re().captures_iter(sql) {
        if let Some(name) = normalize(&caps[1]) {
            let already_written = found
                .iter()
                .any(|r| r.table == name && r.access == Access::Write);
            if !already_written {
                found.insert(TableRef {
                    table: name,
                    access: Access::Read,
                });
            }
        }
    }
    found.into_iter().collect()
}

fn normalize(raw: &str) -> Option<String> {
    let bare = last_component(raw.trim())?.to_ascii_lowercase();
    if bare.len() < 2 || NOT_A_TABLE.contains(&bare.as_str()) {
        return None;
    }
    if bare.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(bare)
}

/// Take the table part of a possibly schema-qualified, possibly quoted name.
///
/// Quotes are handled from the right so a quoted identifier containing a dot — legal,
/// and present in the wild — is not split down the middle.
fn last_component(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let closing = trimmed.chars().last()?;
    let opening = match closing {
        '`' => '`',
        '"' => '"',
        ']' => '[',
        _ => {
            let bare = trimmed.rsplit('.').next().unwrap_or(trimmed).trim();
            return (!bare.is_empty()).then(|| bare.to_string());
        }
    };
    let body = &trimmed[..trimmed.len() - closing.len_utf8()];
    let start = body.rfind(opening)?;
    let inner = body[start + opening.len_utf8()..].trim();
    (!inner.is_empty()).then(|| inner.to_string())
}

/// Does this text look like SQL worth scanning?
///
/// A deliberately cheap prefilter that runs before the regexes so a repository of
/// ordinary strings does not pay regex cost on every line. It is allowed to
/// over-accept: correctness lives in [`table_refs`], which rejects what this lets past.
pub fn looks_like_sql(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    (lowered.contains("select ") && lowered.contains(" from "))
        || lowered.contains("insert into ")
        || lowered.contains("update ")
        || lowered.contains("delete from ")
        || lowered.contains("create table")
        || lowered.contains("alter table")
}

/// Extract table nodes and access edges from a standalone `.sql` file.
pub fn extract_file(path: &str, text: &str) -> FileExtract {
    let mut out = FileExtract::default();
    let file_uid = uid::file(path);
    for reference in table_refs(text) {
        stage_table(&mut out, &reference.table);
        out.batch.edge(NewEdge::new(
            file_uid.clone(),
            uid::db_object(&reference.table),
            access_edge(reference.access),
            Status::Observed,
            0.9,
        ));
    }
    out
}

/// Extract table access from SQL embedded in source code, attributed to the enclosing
/// symbol.
///
/// `owners` maps a line number to the uid of the innermost symbol covering it, so an
/// access edge points at the function that performs the query rather than at the file.
pub fn extract_embedded(text: &str, owner_at: &dyn Fn(u32) -> Option<String>) -> FileExtract {
    let mut out = FileExtract::default();
    for (idx, line) in text.lines().enumerate() {
        if !looks_like_sql(line) {
            continue;
        }
        let line_no = idx as u32 + 1;
        let Some(owner) = owner_at(line_no) else {
            continue;
        };
        for reference in table_refs(line) {
            stage_table(&mut out, &reference.table);
            out.batch.edge(NewEdge::new(
                owner.clone(),
                uid::db_object(&reference.table),
                access_edge(reference.access),
                Status::Observed,
                0.8,
            ));
        }
    }
    out
}

fn stage_table(out: &mut FileExtract, table: &str) {
    let words = crate::extract::code::split_identifier(table);
    out.vocabulary.extend(words.clone());
    out.batch.node(
        NewNode::new(uid::db_object(table), NodeKind::DatabaseObject, table)
            .lang(Lang::Sql)
            .status(Status::Observed, 0.9)
            .search(format!("{table} {}", words.join(" ")))
            .data(serde_json::json!({
                "object_kind": "table",
                "summary": format!("table {table}"),
            })),
    );
}

fn access_edge(access: Access) -> EdgeKind {
    match access {
        Access::Read => EdgeKind::Reads,
        Access::Write => EdgeKind::Writes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tables(sql: &str) -> Vec<(String, Access)> {
        table_refs(sql)
            .into_iter()
            .map(|r| (r.table, r.access))
            .collect()
    }

    #[test]
    fn select_statements_produce_reads() {
        assert_eq!(
            tables("SELECT id FROM sales_order WHERE total > 5"),
            vec![("sales_order".into(), Access::Read)]
        );
    }

    #[test]
    fn joins_are_read_accesses_too() {
        let found = tables("SELECT * FROM orders o JOIN customers c ON c.id = o.cid");
        assert!(found.contains(&("orders".into(), Access::Read)));
        assert!(found.contains(&("customers".into(), Access::Read)));
    }

    #[test]
    fn mutating_statements_produce_writes() {
        assert_eq!(
            tables("INSERT INTO approval_log (id) VALUES (1)"),
            vec![("approval_log".into(), Access::Write)]
        );
        assert_eq!(
            tables("UPDATE sales_order SET status = 'x'"),
            vec![("sales_order".into(), Access::Write)]
        );
        assert_eq!(
            tables("DELETE FROM cart_item WHERE id = 1"),
            vec![("cart_item".into(), Access::Write)]
        );
    }

    #[test]
    fn a_write_outranks_a_read_on_the_same_table() {
        // `UPDATE t ... FROM t` must report the stronger access, or impact analysis
        // understates the blast radius.
        let found = tables("UPDATE sales_order SET x = 1 FROM sales_order WHERE id = 2");
        assert_eq!(found, vec![("sales_order".into(), Access::Write)]);
    }

    #[test]
    fn schema_qualified_and_quoted_names_normalise_to_the_bare_table() {
        assert_eq!(
            tables("SELECT 1 FROM `erp`.`tabSales Order2`"),
            vec![("tabsales order2".into(), Access::Read)]
        );
        assert_eq!(
            tables(r#"SELECT 1 FROM "public"."Orders""#),
            vec![("orders".into(), Access::Read)]
        );
    }

    #[test]
    fn sql_keywords_are_not_mistaken_for_tables() {
        assert!(tables("SELECT 1 FROM DUAL").is_empty());
        assert!(tables("SELECT * FROM (SELECT 1) x").is_empty());
    }

    #[test]
    fn the_sql_prefilter_accepts_real_sql() {
        assert!(looks_like_sql("SELECT id FROM orders"));
        assert!(looks_like_sql("insert into audit_log values (1)"));
        assert!(!looks_like_sql("this sentence mentions nothing queryable"));
    }

    #[test]
    fn ordinary_prose_that_slips_past_the_prefilter_yields_no_tables() {
        // The prefilter over-accepts by design; this is the assertion that matters.
        assert!(looks_like_sql("select a plan from the list"));
        assert!(tables("select a plan from the list").is_empty());
        assert!(tables("pick one from these options").is_empty());
    }

    #[test]
    fn frappe_style_quoted_table_names_with_spaces_are_recognised() {
        // ERPNext names every table `tabSales Order`; missing this misses the repo.
        assert_eq!(
            tables("select name from `tabSales Order` where docstatus = 1"),
            vec![("tabsales order".into(), Access::Read)]
        );
    }

    #[test]
    fn a_sql_file_attributes_access_to_the_file() {
        let fx = extract_file("db/report.sql", "SELECT * FROM sales_order");
        assert_eq!(fx.batch.edges.len(), 1);
        assert_eq!(fx.batch.edges[0].src, "file:db/report.sql");
        assert_eq!(fx.batch.edges[0].dst, "db:sales_order");
        assert_eq!(fx.batch.edges[0].kind, EdgeKind::Reads);
    }

    #[test]
    fn embedded_sql_is_attributed_to_the_enclosing_symbol() {
        let src = "def load():\n    q = \"SELECT id FROM sales_order\"\n    return q\n";
        let owner = |line: u32| (1..=3).contains(&line).then(|| "sym:a.py#load".to_string());
        let fx = extract_embedded(src, &owner);
        assert_eq!(fx.batch.edges.len(), 1);
        assert_eq!(fx.batch.edges[0].src, "sym:a.py#load");
        assert_eq!(fx.batch.edges[0].dst, "db:sales_order");
    }

    #[test]
    fn embedded_sql_outside_any_symbol_is_dropped() {
        let src = "QUERY = \"SELECT id FROM sales_order\"\n";
        let fx = extract_embedded(src, &|_| None);
        assert!(fx.batch.edges.is_empty());
    }
}

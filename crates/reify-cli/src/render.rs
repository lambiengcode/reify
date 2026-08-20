//! Output rendering.
//!
//! Human output is dense, greppable and pipe-safe: no boxes that break on narrow
//! terminals, no colour when the output is redirected. Agent output is JSON against a
//! versioned schema.
//!
//! One rule governs both: an epistemic status is always shown next to a claim. A
//! renderer that prints an `INFERRED` rule as bare prose is a bug, and there is a test
//! that says so.

use anyhow::Result;
use owo_colors::OwoColorize;
use serde::Serialize;

use reify::context::Context;
use reify::discover::Discovery;
use reify::index::IndexReport;
use reify::model::{Node, Status};
use reify::query::{ImpactAnswer, Report, StoredConflict, WhyAnswer};
use reify::store::Store;

/// Print `value` as indented JSON.
fn emit_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// A short marker for a claim's epistemic status.
///
/// Rendered on every claim in every section: the whole safety story is that an agent,
/// or a human, can tell a parsed fact from a guess without reading the docs.
fn tag(status: Status) -> String {
    let text = match status {
        Status::Confirmed => "confirmed",
        Status::Observed => "observed",
        Status::Inferred => "inferred",
        Status::Conflicted => "CONFLICT",
        Status::Assumed => "assumed",
        Status::Unknown => "unknown",
    };
    if !colours_wanted() {
        return format!("[{text}]");
    }
    match status {
        Status::Confirmed | Status::Observed => format!("[{}]", text.green()),
        Status::Inferred | Status::Assumed => format!("[{}]", text.yellow()),
        Status::Conflicted => format!("[{}]", text.red().bold()),
        Status::Unknown => format!("[{}]", text.dimmed()),
    }
}

/// Colour only when a human is looking at a terminal.
fn colours_wanted() -> bool {
    std::env::var_os("NO_COLOR").is_none() && std::io::IsTerminal::is_terminal(&std::io::stdout())
}

fn heading(text: &str) {
    if colours_wanted() {
        println!("\n{}", text.bold());
    } else {
        println!("\n{text}");
    }
}

pub fn init(
    root: &std::path::Path,
    found: &Discovery,
    created_glossary: bool,
    json: bool,
) -> Result<()> {
    #[derive(Serialize)]
    struct Out<'a> {
        root: String,
        indexable: usize,
        skipped: usize,
        skip_reasons: Vec<(&'a str, usize)>,
        glossary_created: bool,
    }
    let skip_reasons = found.skip_summary();
    if json {
        return emit_json(&Out {
            root: root.display().to_string(),
            indexable: found.files.len(),
            skipped: found.skipped.len(),
            skip_reasons: skip_reasons.clone(),
            glossary_created: created_glossary,
        });
    }

    println!("Initialised reify in {}", root.join(".reify").display());
    if created_glossary {
        println!("Wrote .reify/glossary.toml — declaring your domain terms there is the");
        println!("single highest-value thing you can do for retrieval quality.");
    }
    heading("Will index");
    let mut by_lang: Vec<(&str, usize)> = Vec::new();
    for file in &found.files {
        let key = file.lang.as_str();
        match by_lang.iter_mut().find(|(k, _)| *k == key) {
            Some((_, n)) => *n += 1,
            None => by_lang.push((key, 1)),
        }
    }
    by_lang.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for (lang, count) in by_lang {
        println!("  {count:>6}  {lang}");
    }
    if !skip_reasons.is_empty() {
        heading("Will skip");
        for (reason, count) in skip_reasons {
            println!("  {count:>6}  {reason}");
        }
    }
    println!("\nNext: reify index");
    Ok(())
}

pub fn index_report(report: &IndexReport, json: bool) -> Result<()> {
    if json {
        return emit_json(report);
    }
    if report.was_noop() {
        println!(
            "Nothing changed. {} files already indexed.",
            report.files_unchanged
        );
        return Ok(());
    }
    println!(
        "Indexed {} files in {:.1}s ({} unchanged, {} removed, {} skipped)",
        report.files_parsed,
        report.elapsed_ms as f64 / 1000.0,
        report.files_unchanged,
        report.files_removed,
        report.files_skipped
    );
    println!(
        "  {} symbols · {} doc sections · {} tables · {} concepts · {} rules · {} commits · {} edges",
        report.symbols,
        report.doc_sections,
        report.database_objects,
        report.concepts,
        report.rules,
        report.commits,
        report.edges
    );
    if report.conflicts > 0 {
        println!(
            "  {} documentation/code contradiction(s) — see `reify conflicts`",
            report.conflicts
        );
    }
    if report.history_truncated {
        println!("  history walk hit its commit limit; older commits were not read");
    }
    if !report.parse_errors.is_empty() {
        println!(
            "  {} file(s) could not be parsed:",
            report.parse_errors.len()
        );
        for error in report.parse_errors.iter().take(5) {
            println!("    {error}");
        }
    }
    Ok(())
}

pub fn status(store: &Store, root: &std::path::Path, json: bool) -> Result<()> {
    #[derive(Serialize)]
    struct Out {
        root: String,
        store: String,
        indexed_at: Option<String>,
        indexed_head: Option<String>,
        current_head: Option<String>,
        stale: bool,
        files: usize,
    }
    let indexed_head = store.meta("head_sha")?;
    let current_head = reify::gitlog::head_sha(root);
    let out = Out {
        root: root.display().to_string(),
        store: store.path().display().to_string(),
        indexed_at: store.meta("indexed_at")?,
        stale: indexed_head.is_some() && indexed_head != current_head,
        indexed_head,
        current_head,
        files: store.files()?.len(),
    };
    if json {
        return emit_json(&out);
    }
    println!("root    {}", out.root);
    println!("store   {}", out.store);
    println!("files   {}", out.files);
    match (&out.indexed_head, &out.current_head) {
        (Some(indexed), Some(current)) if indexed != current => {
            println!(
                "head    {} (working tree is at {}) — run `reify index`",
                &indexed[..7.min(indexed.len())],
                &current[..7.min(current.len())]
            );
        }
        (Some(indexed), _) => println!("head    {}", &indexed[..7.min(indexed.len())]),
        _ => println!("head    not a git repository"),
    }
    Ok(())
}

pub fn context(compiled: &Context, json: bool) -> Result<()> {
    if json {
        return emit_json(compiled);
    }
    println!("TASK  {}", compiled.task);
    println!(
        "      {} of {} tokens = {} context + {} to read ({} estimate)",
        compiled.budget.used,
        compiled.budget.requested,
        compiled.budget.context,
        compiled.budget.reads,
        compiled.budget.estimator
    );

    if !compiled.conflicts.is_empty() {
        heading("Conflicts");
        for conflict in &compiled.conflicts {
            println!("  {} {}", tag(conflict.status), conflict.subject);
            println!(
                "      documented: {} ({})",
                conflict.documented, conflict.documented_at
            );
            println!(
                "      observed:   {} ({})",
                conflict.observed, conflict.observed_at
            );
        }
    }
    if !compiled.concepts.is_empty() {
        heading("Concepts");
        for concept in &compiled.concepts {
            let labels = concept
                .labels
                .as_object()
                .map(|m| {
                    m.iter()
                        .map(|(k, v)| format!("{k}: {}", v.as_str().unwrap_or("")))
                        .collect::<Vec<_>>()
                        .join(" · ")
                })
                .unwrap_or_default();
            println!("  {} {}", tag(concept.status), concept.id);
            if !labels.is_empty() {
                println!("      {labels}");
            }
        }
    }
    if !compiled.rules.is_empty() {
        heading("Business rules");
        for rule in &compiled.rules {
            println!(
                "  {} {:.2}  {}",
                tag(rule.status),
                rule.confidence,
                rule.claim
            );
            for citation in &rule.evidence {
                println!("      evidence: {citation}");
            }
        }
    }
    if !compiled.code.is_empty() {
        heading("Code");
        for item in &compiled.code {
            println!(
                "  {} {}:{}  {}",
                tag(item.status),
                item.path,
                item.lines,
                item.symbol
            );
            println!("      {}", item.why);
        }
    }
    if !compiled.documents.is_empty() {
        heading("Documents");
        for doc in &compiled.documents {
            let lang = doc.lang.as_deref().unwrap_or("?");
            println!(
                "  {} {} [{lang}]  {}",
                tag(doc.status),
                doc.location,
                doc.document
            );
            println!("      {}", doc.excerpt);
        }
    }
    if !compiled.data.is_empty() {
        heading("Data");
        for item in &compiled.data {
            println!("  {} {}  ({})", tag(item.status), item.table, item.why);
        }
    }
    if !compiled.history.is_empty() {
        heading("History");
        for commit in &compiled.history {
            println!("  {} {}  {}", commit.commit, commit.date, commit.subject);
        }
    }
    if !compiled.next_reads.is_empty() {
        heading("Read next");
        for read in &compiled.next_reads {
            println!(
                "  {}:{}  (~{} tokens)",
                read.path, read.lines, read.est_tokens
            );
        }
    }
    if !compiled.unknowns.is_empty() {
        heading("Not determined");
        for unknown in &compiled.unknowns {
            println!("  - {unknown}");
        }
    }
    Ok(())
}

pub fn why(answer: &WhyAnswer, json: bool) -> Result<()> {
    if json {
        return emit_json(answer);
    }
    println!(
        "{}  {}",
        answer.location,
        answer.symbol.as_deref().unwrap_or(&answer.target)
    );
    if let Some(signature) = &answer.signature {
        println!("  {signature}");
    }
    if let Some(doc) = &answer.documentation {
        println!("  {doc}");
    }

    let section = |title: &str, items: &[reify::query::Citation]| {
        if items.is_empty() {
            return;
        }
        heading(title);
        for item in items {
            println!("  {} {}  {}", tag(item.status), item.location, item.what);
        }
    };
    section("Concepts", &answer.concepts);
    section("Documents", &answer.documents);
    section("Calls", &answer.calls);
    section("Called by", &answer.called_by);
    section("Reads", &answer.reads);
    section("Writes", &answer.writes);
    section("Changes with", &answer.co_changes);

    if !answer.history.is_empty() {
        heading("History");
        for commit in &answer.history {
            println!(
                "  {}  {}  {:<8}  {}",
                commit.sha, commit.date, commit.class, commit.subject
            );
        }
    }
    if !answer.unknowns.is_empty() {
        heading("Not determined");
        for unknown in &answer.unknowns {
            println!("  - {unknown}");
        }
    }
    Ok(())
}

pub fn impact(answer: &ImpactAnswer, json: bool) -> Result<()> {
    if json {
        return emit_json(answer);
    }
    println!("IMPACT  {}", answer.query);
    if !answer.origins.is_empty() {
        heading("Changing");
        for origin in &answer.origins {
            println!(
                "  {} {}  {}",
                tag(origin.status),
                origin.location,
                origin.what
            );
        }
    }
    if !answer.affected.is_empty() {
        heading("Affected");
        for item in &answer.affected {
            println!(
                "  {} {}  {}  ({}, {} hop{})",
                tag(item.status),
                item.location,
                item.what,
                item.reason,
                item.distance,
                if item.distance == 1 { "" } else { "s" }
            );
        }
    }
    if !answer.tables.is_empty() {
        heading("Data touched");
        for table in &answer.tables {
            println!("  {} {}", tag(table.status), table.what);
        }
    }
    if !answer.co_changing_files.is_empty() {
        heading("Historically changes with");
        for file in &answer.co_changing_files {
            println!("  {} {}", tag(file.status), file.location);
        }
    }
    if !answer.unknowns.is_empty() {
        heading("Not determined");
        for unknown in &answer.unknowns {
            println!("  - {unknown}");
        }
    }
    Ok(())
}

pub fn conflicts(items: &[StoredConflict], json: bool) -> Result<()> {
    if json {
        return emit_json(&items);
    }
    if items.is_empty() {
        println!("No documentation/code contradictions detected.");
        println!("Detection is deliberately conservative; silence is not proof of agreement.");
        return Ok(());
    }
    for conflict in items {
        println!(
            "\n{} {}  ({:.2})",
            tag(Status::Conflicted),
            conflict.subject,
            conflict.confidence
        );
        println!("  documented  {}", conflict.documented);
        println!("              {}", conflict.documented_at);
        println!("  observed    {}", conflict.observed);
        println!("              {}", conflict.observed_at);
        println!("  status      {}", conflict.resolution);
    }
    Ok(())
}

pub fn rules(items: &[Node], json: bool) -> Result<()> {
    if json {
        return emit_json(&items);
    }
    if items.is_empty() {
        println!("No business rules mined above the confidence threshold.");
        return Ok(());
    }
    for rule in items {
        let claim = rule
            .data
            .get("claim")
            .and_then(|v| v.as_str())
            .unwrap_or(&rule.name);
        let source = rule
            .data
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        println!(
            "{} {:.2}  {}\n      {}  ({source})",
            tag(rule.status),
            rule.confidence,
            claim,
            rule.location()
        );
    }
    Ok(())
}

pub fn report(report: &Report, json: bool) -> Result<()> {
    if json {
        return emit_json(report);
    }
    let row = |label: &str, value: String| println!("  {label:<28}{value:>18}");
    println!("\n  REIFY SYSTEM REPORT");
    println!("  {}", "-".repeat(46));
    row("Files", report.files.to_string());
    row("Symbols", report.symbols.to_string());
    row("Document sections", report.doc_sections.to_string());
    row("Database objects", report.database_objects.to_string());
    row("Concepts", report.concepts.to_string());
    let mut bridges: Vec<(&String, &u64)> = report.concepts_by_bridge.iter().collect();
    bridges.sort();
    for (bridge, count) in bridges {
        row(&format!("  from {bridge}"), count.to_string());
    }
    row("Business rules", report.rules.to_string());
    row("Contradictions", report.conflicts.to_string());
    row("Commits linked", report.commits.to_string());
    row("Relationships", report.edges.to_string());
    row(
        "Documented symbols",
        format!("{:.0}%", report.documented_symbols * 100.0),
    );
    row(
        "Knowledge coverage",
        format!("{:.0}%", report.knowledge_coverage * 100.0),
    );
    println!("  {}", "-".repeat(46));
    println!("  Knowledge coverage: share of symbols reachable from a concept or document.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_status_renders_a_visible_marker() {
        // An agent must never receive a claim whose footing is unstated.
        for status in [
            Status::Confirmed,
            Status::Observed,
            Status::Inferred,
            Status::Conflicted,
            Status::Assumed,
            Status::Unknown,
        ] {
            let rendered = tag(status);
            assert!(rendered.starts_with('['), "{status:?} -> {rendered}");
            assert!(rendered.ends_with(']'), "{status:?} -> {rendered}");
            assert!(rendered.len() > 2);
        }
    }

    #[test]
    fn conflicted_is_visually_distinct_from_confirmed() {
        assert_ne!(tag(Status::Conflicted), tag(Status::Confirmed));
    }
}

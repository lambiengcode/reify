//! Output rendering.
//!
//! Human output is dense, greppable and pipe-safe: no boxes that break on narrow
//! terminals, no colour when the output is redirected. Agent output is JSON against a
//! versioned schema.
//!
//! One rule governs both: a claim is never rendered without its epistemic status. A
//! renderer that prints an `INFERRED` rule as bare prose is a bug, and there is a test
//! that says so.
//!
//! In the human output that status may be stated once for a whole section when every
//! row shares it, rather than repeated down the page. The property being protected is
//! that a reader can always tell a parsed fact from a guess — not that the badge is
//! printed a fixed number of times. Machine output always carries it per item.

use anyhow::Result;
use owo_colors::OwoColorize;
use serde::Serialize;

use reify::context::Context;
use reify::discover::Discovery;
use reify::doctor::{self, Diagnosis, Verdict};
use reify::index::IndexReport;
use reify::llm;
use reify::model::{Node, Status};
use reify::query::{
    ConceptOverview, Explanation, FlowAnswer, ImpactAnswer, Preflight, Report, StoredConflict,
    WhyAnswer,
};
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

/// The one status a whole section shares, if it shares one.
///
/// A badge repeated identically down twenty lines carries no information, and an
/// invariant `[confirmed]` invites exactly the wrong reading: it attests that the
/// symbol was *parsed from source*, never that it is the right place to change.
/// Symbols are `CONFIRMED` by construction, so the code section is nearly always
/// uniform — said once on the heading it is a fact, repeated per line it is noise.
/// The machine-readable output is untouched; every item still carries its status.
fn shared_status<T>(items: &[T], status: impl Fn(&T) -> Status) -> Option<Status> {
    let first = status(items.first()?);
    items.iter().all(|i| status(i) == first).then_some(first)
}

/// A heading that names the status its whole section shares.
fn heading_for(text: &str, shared: Option<Status>) {
    match shared {
        Some(status) => {
            let note = format!("all {}", tag(status));
            if colours_wanted() {
                println!("\n{}  {}", text.bold(), note.dimmed());
            } else {
                println!("\n{text}  {note}");
            }
        }
        None => heading(text),
    }
}

/// A progress line that overwrites itself, for a stage that runs for a minute.
///
/// Written to **stderr** so `reify index` stays pipeable, and suppressed entirely when
/// stderr is not a terminal — progress in a CI log is noise, not information.
pub fn progress_reporter() -> reify::index::ProgressFn {
    std::sync::Arc::new(|stage: &str, done: usize, total: usize| {
        if !std::io::IsTerminal::is_terminal(&std::io::stderr()) {
            return;
        }
        let line = if total > 0 {
            format!("  {stage} {done}/{total}")
        } else {
            format!("  {stage}")
        };
        // \r and a trailing clear, so the next stage overwrites this one cleanly.
        eprint!("\r{line}\x1b[K");
        let _ = std::io::Write::flush(&mut std::io::stderr());
    })
}

/// Erase the progress line before printing the result.
pub fn clear_progress() {
    if std::io::IsTerminal::is_terminal(&std::io::stderr()) {
        eprint!("\r\x1b[K");
        let _ = std::io::Write::flush(&mut std::io::stderr());
    }
}

#[allow(clippy::too_many_arguments)]
pub fn init(
    root: &std::path::Path,
    found: &Discovery,
    created_glossary: bool,
    agent_file: Option<&std::path::Path>,
    wrote_instructions: bool,
    is_git_repository: bool,
    json: bool,
) -> Result<()> {
    #[derive(Serialize)]
    struct Out<'a> {
        root: String,
        indexable: usize,
        skipped: usize,
        skip_reasons: Vec<(&'a str, usize)>,
        glossary_created: bool,
        agent_instruction_file: Option<String>,
        agent_instructions_written: bool,
        git_repository: bool,
    }
    let skip_reasons = found.skip_summary();
    if json {
        return emit_json(&Out {
            root: root.display().to_string(),
            indexable: found.files.len(),
            skipped: found.skipped.len(),
            skip_reasons: skip_reasons.clone(),
            glossary_created: created_glossary,
            agent_instruction_file: agent_file.map(|p| p.display().to_string()),
            agent_instructions_written: wrote_instructions,
            git_repository: is_git_repository,
        });
    }

    println!("Initialised reify in {}", root.join(".reify").display());

    // An empty or non-repository directory is almost always a mistake — the wrong
    // path, or a directory that was never checked out. Saying nothing lets the user
    // discover it a minute later, at the end of an index that found nothing.
    if found.files.is_empty() {
        println!("\nNothing here is indexable.");
        println!("Check the path, or that this directory contains source you expect.");
        if !skip_reasons.is_empty() {
            println!("\nEverything present was skipped:");
            for (reason, count) in &skip_reasons {
                println!("  {count:>6}  {reason}");
            }
        }
        return Ok(());
    }
    if !is_git_repository {
        println!(
            "\nThis is not a git repository, so history, blame and co-change will be \
             empty.\nEverything else works."
        );
    }
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

    heading("Tell your agent about it");
    match (agent_file, wrote_instructions) {
        (Some(path), true) => println!("  Instructions appended to {}", path.display()),
        (Some(path), false) => println!(
            "  Add Reify to {} — run `reify init --write-agent-instructions`",
            path.display()
        ),
        (None, _) => println!(
            "  No AGENTS.md or CLAUDE.md found. Create one and run\n  \
             `reify init --write-agent-instructions`, or add this yourself:\n\
             \n      Run `reify context \"<task>\"` before changing code here."
        ),
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
    if std::env::var_os("REIFY_TIMING").is_some() {
        for (stage, ms) in &report.stage_ms {
            println!("  {:>7}ms  {stage}", ms);
        }
    }
    if report.history_truncated {
        println!("  history walk hit its commit limit; older commits were not read");
    }
    if let Some(reason) = &report.history_unavailable {
        // Said plainly, with the cause: everything else indexed, and the user can
        // decide whether the missing evidence is worth completing the clone for.
        println!("  history could not be read, so `why` and blast radius lose their");
        println!("  commit evidence. Everything else indexed normally.");
        println!("    {}", reason.lines().next().unwrap_or(reason));
        if reason.contains("lazy fetching disabled") || reason.contains("promisor") {
            println!("    This is a partial clone missing objects it needs. Complete it with:");
            println!("      git fetch --refetch origin");
        }
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
        let shared = shared_status(&compiled.concepts, |c| c.status);
        heading_for("Concepts", shared);
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
            match shared {
                Some(_) => println!("  {}", concept.id),
                None => println!("  {} {}", tag(concept.status), concept.id),
            }
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
        let shared = shared_status(&compiled.code, |i| i.status);
        heading_for("Code", shared);
        for item in &compiled.code {
            match shared {
                Some(_) => println!("  {}:{}  {}", item.path, item.lines, item.symbol),
                None => println!(
                    "  {} {}:{}  {}",
                    tag(item.status),
                    item.path,
                    item.lines,
                    item.symbol
                ),
            }
            println!("      {}", item.why);
        }
    }
    if !compiled.documents.is_empty() {
        let shared = shared_status(&compiled.documents, |d| d.status);
        heading_for("Documents", shared);
        for doc in &compiled.documents {
            let lang = doc.lang.as_deref().unwrap_or("?");
            match shared {
                Some(_) => println!("  {} [{lang}]  {}", doc.location, doc.document),
                None => println!(
                    "  {} {} [{lang}]  {}",
                    tag(doc.status),
                    doc.location,
                    doc.document
                ),
            }
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
        // A file every module imports has hundreds of dependants. Printing all of them
        // spends an agent's budget to say one thing — "a lot" — so the count leads and
        // the nearest few are the evidence for it.
        let shown = answer.affected.len();
        if answer.affected_total > shown {
            heading(&format!(
                "Affected  {} total, {shown} nearest shown",
                answer.affected_total
            ));
        } else {
            heading(&format!("Affected  {shown}"));
        }
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

pub fn explain(answer: &Explanation, json: bool) -> Result<()> {
    if json {
        return emit_json(answer);
    }
    println!("{} {}", tag(answer.status), answer.id);
    if let Some(labels) = answer.labels.as_object() {
        for (lang, label) in labels {
            println!("  {lang:<6}{}", label.as_str().unwrap_or(""));
        }
    }
    println!("  source  {}", answer.bridge);

    let section = |title: &str, items: &[reify::query::Citation]| {
        if items.is_empty() {
            return;
        }
        heading(title);
        for item in items {
            println!("  {} {}  {}", tag(item.status), item.location, item.what);
        }
    };
    section("Code", &answer.code);
    section("Data", &answer.data);
    section("Documents", &answer.documents);
    section("Rules", &answer.rules);

    if !answer.also_matched.is_empty() {
        heading("Also matched");
        for other in &answer.also_matched {
            println!("  {other}");
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

pub fn flow(answer: &FlowAnswer, json: bool) -> Result<()> {
    if json {
        return emit_json(answer);
    }
    println!("FLOW  {}", answer.process);
    println!("      derived from the {}", answer.derived_from);
    for step in &answer.steps {
        println!(
            "\n  {:>2}. {} {}\n      {}  ({})",
            step.order,
            tag(step.status),
            step.symbol,
            step.location,
            step.reached_by
        );
    }
    if !answer.unknowns.is_empty() {
        heading("Not determined");
        for unknown in &answer.unknowns {
            println!("  - {unknown}");
        }
    }
    Ok(())
}

/// A single dense block, sized for injection into an editor hook.
pub fn preflight(answer: &Preflight, json: bool) -> Result<()> {
    if json {
        return emit_json(answer);
    }
    println!("PREFLIGHT  {}", answer.path);
    println!(
        "  rules {} · concepts {} · tables {} · dependants {} · conflicts {}",
        answer.rules, answer.concepts, answer.tables, answer.dependants, answer.conflicts
    );
    for rule in &answer.highest_risk_rules {
        println!("  · {}", rule.what);
    }
    let risk = if colours_wanted() {
        match answer.risk {
            "HIGH" => answer.risk.red().bold().to_string(),
            "MEDIUM" => answer.risk.yellow().to_string(),
            _ => answer.risk.green().to_string(),
        }
    } else {
        answer.risk.to_string()
    };
    println!("  RISK: {risk} — {}", answer.reason);
    println!("  next: {}", answer.suggested_command);
    Ok(())
}

/// `reify doctor`: should this repository use Reify at all?
///
/// Named signals with measured values and a plain-language verdict. Deliberately not a
/// score: `docs/metrics.md` forbids printing a number that cannot be defined, and a
/// weighted blend of four heuristics tuned on four repositories is exactly that.
pub fn doctor(answer: &Diagnosis, json: bool) -> Result<()> {
    if json {
        return emit_json(answer);
    }
    println!("DOCTOR  {}", answer.root);
    println!();

    let floor = doctor::floor_text();
    signal(
        "scale",
        &doctor::scale_text(&answer.scale),
        if answer.verdict == Verdict::TooSmall {
            format!("below the {floor} floor")
        } else {
            format!("above the {floor} floor")
        },
    );

    // Below the floor the other signals were never computed, and saying why is more
    // useful than printing three lines of zeroes.
    if answer.verdict == Verdict::TooSmall {
        verdict_line(answer);
        return Ok(());
    }

    match &answer.vocabulary {
        Some(v) => signal(
            "vocabulary",
            &format!(
                "{} of focused commits name a file they changed",
                doctor::percent(v.locality)
            ),
            format!("{} of {} commits", v.commits_local, v.commits_considered),
        ),
        None => signal(
            "vocabulary",
            "not measurable without git history",
            String::new(),
        ),
    }
    match &answer.history {
        Some(h) => {
            signal(
                "history",
                &format!(
                    "{} commits, {} focused enough to attribute",
                    h.commits_read,
                    doctor::percent(h.focus)
                ),
                format!("median commit changes {} file(s)", h.median_files_changed),
            );
            // Said only when it is not the case. On all five repositories this was
            // calibrated against it sat at 100%, so printing it always would be noise.
            if h.usable_share < 0.9 {
                signal(
                    "",
                    &format!(
                        "only {} carry a subject worth reading",
                        doctor::percent(h.usable_share)
                    ),
                    String::new(),
                );
            }
        }
        None => signal("history", "could not be read", String::new()),
    }
    signal(
        "documents",
        &format!(
            "{} document(s) only Reify can read",
            answer.documents.unreadable_by_grep
        ),
        answer.documents.examples.join(", "),
    );

    verdict_line(answer);
    Ok(())
}

/// One measured signal: name, value, and the note that puts it in context.
fn signal(name: &str, value: &str, note: String) {
    let name = if colours_wanted() {
        format!("{:<12}", name.bold())
    } else {
        format!("{name:<12}")
    };
    if note.is_empty() {
        println!("  {name}{value}");
    } else if colours_wanted() {
        println!("  {name}{value:<48}{}", note.dimmed());
    } else {
        println!("  {name}{value:<48}{note}");
    }
}

fn verdict_line(answer: &Diagnosis) {
    let text = answer.verdict.as_str();
    let painted = if colours_wanted() {
        match answer.verdict {
            Verdict::LikelyWorthIt => text.green().bold().to_string(),
            Verdict::TooSmall | Verdict::UnlikelyToHelp => text.red().bold().to_string(),
            Verdict::Marginal => text.yellow().bold().to_string(),
        }
    } else {
        text.to_string()
    };
    // The verdict word is part of the first wrapped line, so the wrap has to know how
    // wide it is — measured on the unpainted text, since colour codes take no columns.
    let lead = format!("  {text} — ");
    println!(
        "\n  {painted} — {}",
        wrap(&answer.reason, WIDTH, "  ", lead.chars().count())
    );

    if !answer.what_would_change_it.is_empty() {
        println!("\n  What would change this:");
        for item in &answer.what_would_change_it {
            println!("    - {}", wrap(item, WIDTH, "      ", 6));
        }
    }
    if let Some(c) = doctor::comparable(answer.verdict) {
        // Named by role rather than by favourability: for a yes the useful comparison
        // is the repository where Reify did worst, and for a no it is the one where it
        // did best. Calling both "least favourable" would be wrong half the time.
        let role = match answer.verdict {
            Verdict::UnlikelyToHelp => "did best",
            _ => "did worst",
        };
        println!(
            "\n  {}",
            wrap(
                &format!(
                    "For comparison, the measured repository where Reify {role}: {}, \
                     where {}. See {}.",
                    c.name, c.outcome, c.report
                ),
                WIDTH,
                "  ",
                2,
            )
        );
    }
    // The verdict is a heuristic over four repositories, and a reader could otherwise
    // mistake it for a measurement of theirs. The cost is stated for the same reason
    // the answer is: so nobody has to guess what running it will take.
    println!(
        "\n  {}",
        wrap(
            &format!(
                "This is a heuristic fitted to four measured repositories, not a \
                 measurement of this one — `reify-bench` measures this one. Read the \
                 working tree and {} in {:.1}s; no index needed, and none was used.",
                if answer.git_repository {
                    "the newest 1000 commits"
                } else {
                    "no history"
                },
                answer.elapsed_ms as f64 / 1000.0,
            ),
            WIDTH,
            "  ",
            2,
        )
    );
}

/// Terminal width the doctor output is wrapped to.
///
/// Fixed rather than read from the terminal: the verdict is the one paragraph that must
/// be readable, and it must read the same in a pipe, a CI log and a screenshot.
const WIDTH: usize = 78;

/// Wrap `text` to `width`, indenting continuation lines by `indent`.
///
/// `first_column` is how far into the line the caller has already printed, so a verdict
/// word or a bullet marker is counted against the first line's budget rather than
/// pushing it past the right edge.
fn wrap(text: &str, width: usize, indent: &str, first_column: usize) -> String {
    let mut out = String::new();
    let mut column = first_column;
    let mut fresh = true;
    for word in text.split_whitespace() {
        if !fresh && column + 1 + word.chars().count() > width {
            out.push('\n');
            out.push_str(indent);
            column = indent.chars().count();
        } else if !fresh {
            out.push(' ');
            column += 1;
        }
        out.push_str(word);
        column += word.chars().count();
        fresh = false;
    }
    out
}

pub fn concepts(overview: &ConceptOverview, json: bool) -> Result<()> {
    if json {
        return emit_json(overview);
    }
    println!("{} concepts", overview.total);
    let mut bridges: Vec<(&String, &usize)> = overview.by_bridge.iter().collect();
    bridges.sort();
    for (bridge, count) in bridges {
        println!("  {count:>6}  from {bridge}");
    }
    heading("Most connected");
    for row in overview.concepts.iter().take(25) {
        println!(
            "  {} {:<34} {:>3} links  [{}]  {}",
            tag(row.status),
            row.id,
            row.links,
            row.languages.join(","),
            row.display
        );
    }
    println!("\nRun `reify concepts --suggest` to turn these into glossary entries.");
    Ok(())
}

/// The facts a synthesis prompt is allowed to draw on.
///
/// Built from the compiled context and nothing else. The model receives retrieved
/// facts and is asked to phrase them; it is never asked what it knows.
pub fn facts_for_synthesis(compiled: &Context) -> Vec<String> {
    let mut facts = Vec::new();
    for conflict in &compiled.conflicts {
        facts.push(format!(
            "CONFLICT about {}: documentation says \"{}\" ({}), the code does \"{}\" ({})",
            conflict.subject,
            conflict.documented,
            conflict.documented_at,
            conflict.observed,
            conflict.observed_at
        ));
    }
    for rule in &compiled.rules {
        facts.push(format!(
            "RULE [{}] {} (evidence: {})",
            rule.status,
            rule.claim,
            rule.evidence.join(", ")
        ));
    }
    for concept in &compiled.concepts {
        facts.push(format!("CONCEPT {} labels={}", concept.id, concept.labels));
    }
    for item in &compiled.code {
        facts.push(format!(
            "CODE {}:{} {} — {}",
            item.path, item.lines, item.symbol, item.why
        ));
    }
    for doc in &compiled.documents {
        facts.push(format!("DOC {} — {}", doc.location, doc.excerpt));
    }
    for table in &compiled.data {
        facts.push(format!("TABLE {} — {}", table.table, table.why));
    }
    facts
}

pub fn llm_status(root: &std::path::Path, json: bool) -> Result<()> {
    let (configured, detail) = match llm::provider(root) {
        Ok(provider) => (true, provider.label),
        Err(reason) => (false, reason.explain()),
    };
    if json {
        return emit_json(&serde_json::json!({
            "configured": configured,
            "detail": detail,
            "log": llm::log_path(root).display().to_string(),
            "derived_status": llm::DERIVED_STATUS.as_str(),
        }));
    }
    if configured {
        println!("Model provider: {detail}");
        println!("Every call is logged to {}", llm::log_path(root).display());
        println!("Anything a model produces is recorded as INFERRED and must be verified.");
    } else {
        println!("{detail}");
    }
    Ok(())
}

pub fn llm_preview(prompt: &str, json: bool) -> Result<()> {
    if json {
        return emit_json(&serde_json::json!({
            "prompt": prompt,
            "bytes": prompt.len(),
            "sent": false,
        }));
    }
    println!("{prompt}");
    eprintln!(
        "\n-- {} bytes. Nothing was sent; this is exactly what a provider would receive. --",
        prompt.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthesis_facts_carry_their_status_and_citation() {
        // A model must never be handed a claim stripped of its footing.
        let compiled = Context {
            schema: "reify.context/1",
            task: "t".into(),
            budget: reify::context::BudgetInfo {
                requested: 100,
                context: 10,
                reads: 0,
                used: 10,
                unit: "tokens",
                estimator: "heuristic-v1",
            },
            concepts: vec![],
            rules: vec![reify::context::RuleOut {
                id: "rule:1".into(),
                status: Status::Inferred,
                confidence: 0.8,
                claim: "corporate orders require approval".into(),
                subject: "approval".into(),
                source: "document".into(),
                evidence: vec!["docs/BRD.md:4".into()],
            }],
            code: vec![],
            documents: vec![],
            data: vec![],
            history: vec![],
            conflicts: vec![],
            unknowns: vec![],
            next_reads: vec![],
        };
        let facts = facts_for_synthesis(&compiled);
        assert_eq!(facts.len(), 1);
        assert!(facts[0].contains("INFERRED"));
        assert!(facts[0].contains("docs/BRD.md:4"));
    }

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
    fn a_sections_status_is_hoisted_only_when_every_row_shares_it() {
        // Hoisting a status that is not actually shared would attest to a footing the
        // rows do not have, which is the one thing this renderer may never do.
        assert_eq!(
            shared_status(&[Status::Confirmed, Status::Confirmed], |s| *s),
            Some(Status::Confirmed),
            "a uniform section states its status once"
        );
        assert_eq!(
            shared_status(&[Status::Confirmed, Status::Inferred], |s| *s),
            None,
            "a mixed section must keep its per-row badges"
        );
        assert_eq!(
            shared_status(&[] as &[Status], |s| *s),
            None,
            "an empty section has no status to hoist"
        );
    }

    #[test]
    fn conflicted_is_visually_distinct_from_confirmed() {
        assert_ne!(tag(Status::Conflicted), tag(Status::Confirmed));
    }
}

/// Render a compiled context as TOON — the agent-facing format.
///
/// The measurement that forced this: for one representative task the JSON envelope
/// cost ~2.9× the tokens `budget.context` claimed, because JSON repeats every field
/// name on every record and the budget was computed from content previews, not the
/// serialization. That is a broken promise in the one number this tool exists to
/// keep. TOON states each section's columns once and then one row per record, which
/// lands within a few percent of the claimed cost — and the header it prints is the
/// *measured* estimate of the very bytes being emitted, so the claim and the payload
/// can no longer drift apart.
///
/// Format: `section[N]{col,col}: ` then one `|`-separated row per record. `status`
/// is always the first column — the safety rule that an agent can tell a parsed fact
/// from a guess survives the compression.
pub fn context_toon(compiled: &Context) -> String {
    fn cell(value: &str) -> String {
        // `|` is the delimiter and newlines are row boundaries; both are replaced,
        // never escaped, because a reader of this format is a model, not a parser.
        let mut cleaned = value.replace('|', "/").replace(['\n', '\r'], " ");
        const MAX: usize = 220;
        if cleaned.chars().count() > MAX {
            cleaned = cleaned.chars().take(MAX).collect();
        }
        cleaned
    }
    fn section(out: &mut String, name: &str, columns: &[&str], rows: Vec<Vec<String>>) {
        if rows.is_empty() {
            return;
        }
        out.push_str(&format!(
            "{name}[{}]{{{}}}:\n",
            rows.len(),
            columns.join(",")
        ));
        for row in rows {
            out.push_str("  ");
            out.push_str(
                &row.iter()
                    .map(|value| cell(value))
                    .collect::<Vec<_>>()
                    .join("|"),
            );
            out.push('\n');
        }
    }

    let mut body = String::new();
    body.push_str(&format!("task: {}\n", compiled.task));

    section(
        &mut body,
        "conflicts",
        &[
            "status",
            "subject",
            "documented",
            "documented_at",
            "observed",
            "observed_at",
        ],
        compiled
            .conflicts
            .iter()
            .map(|c| {
                vec![
                    c.status.as_str().to_string(),
                    c.subject.clone(),
                    c.documented.clone(),
                    c.documented_at.clone(),
                    c.observed.clone(),
                    c.observed_at.clone(),
                ]
            })
            .collect(),
    );
    section(
        &mut body,
        "rules",
        &["status", "confidence", "claim", "evidence"],
        compiled
            .rules
            .iter()
            .map(|r| {
                vec![
                    r.status.as_str().to_string(),
                    format!("{:.2}", r.confidence),
                    r.claim.clone(),
                    r.evidence.join("; "),
                ]
            })
            .collect(),
    );
    section(
        &mut body,
        "concepts",
        &["status", "id", "labels"],
        compiled
            .concepts
            .iter()
            .map(|c| {
                let labels = c
                    .labels
                    .as_object()
                    .map(|m| {
                        m.iter()
                            .map(|(k, v)| format!("{k}:{}", v.as_str().unwrap_or("")))
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .unwrap_or_default();
                vec![c.status.as_str().to_string(), c.id.clone(), labels]
            })
            .collect(),
    );
    section(
        &mut body,
        "code",
        &["status", "path", "lines", "symbol", "why"],
        compiled
            .code
            .iter()
            .map(|c| {
                vec![
                    c.status.as_str().to_string(),
                    c.path.clone(),
                    c.lines.clone(),
                    c.symbol.clone(),
                    c.why.clone(),
                ]
            })
            .collect(),
    );
    section(
        &mut body,
        "documents",
        &["status", "location", "lang", "excerpt"],
        compiled
            .documents
            .iter()
            .map(|d| {
                vec![
                    d.status.as_str().to_string(),
                    d.location.clone(),
                    d.lang.clone().unwrap_or_default(),
                    d.excerpt.clone(),
                ]
            })
            .collect(),
    );
    section(
        &mut body,
        "data",
        &["status", "table", "why"],
        compiled
            .data
            .iter()
            .map(|d| {
                vec![
                    d.status.as_str().to_string(),
                    d.table.clone(),
                    d.why.clone(),
                ]
            })
            .collect(),
    );
    section(
        &mut body,
        "history",
        &["commit", "date", "subject"],
        compiled
            .history
            .iter()
            .map(|h| vec![h.commit.clone(), h.date.clone(), h.subject.clone()])
            .collect(),
    );
    section(
        &mut body,
        "next_reads",
        &["path", "lines", "est_tokens"],
        compiled
            .next_reads
            .iter()
            .map(|r| vec![r.path.clone(), r.lines.clone(), r.est_tokens.to_string()])
            .collect(),
    );
    if !compiled.unknowns.is_empty() {
        body.push_str(&format!("unknowns[{}]:\n", compiled.unknowns.len()));
        for unknown in &compiled.unknowns {
            body.push_str("  ");
            body.push_str(unknown);
            body.push('\n');
        }
    }
    body.push_str("note: INFERRED claims are leads to verify against their citation, not facts\n");

    // The header carries the *measured* cost of the body being emitted, so the
    // budget line and the payload cannot disagree.
    let envelope = reify::tokens::estimate(&body);
    format!(
        "reify.context/1 toon  envelope~{envelope}tok  reads~{}tok  budget {}\n{body}",
        compiled.budget.reads, compiled.budget.requested
    )
}

#[cfg(test)]
mod toon_tests {
    use super::*;
    use reify::context::{BudgetInfo, ConflictOut, NextRead, RuleOut};

    fn sample() -> Context {
        Context {
            schema: "reify.context/1",
            task: "add a discount tier".into(),
            budget: BudgetInfo {
                requested: 4000,
                context: 100,
                reads: 700,
                used: 800,
                unit: "tokens",
                estimator: "heuristic-v1",
            },
            concepts: vec![],
            rules: vec![RuleOut {
                id: "rule:1".into(),
                status: Status::Inferred,
                confidence: 0.8,
                claim: "corporate orders | require approval".into(),
                subject: "approval".into(),
                source: "document".into(),
                evidence: vec!["docs/BRD.md:4".into()],
            }],
            code: vec![],
            documents: vec![],
            data: vec![],
            history: vec![],
            conflicts: vec![ConflictOut {
                id: "conflict:1".into(),
                status: Status::Conflicted,
                subject: "approval".into(),
                documented: "requires approval".into(),
                documented_at: "docs/BRD.md:6".into(),
                observed: "bypasses approval".into(),
                observed_at: "app/order.py:12".into(),
                resolution: "UNRESOLVED".into(),
            }],
            unknowns: vec!["no document describes this".into()],
            next_reads: vec![NextRead {
                path: "app/order.py".into(),
                lines: "10-30".into(),
                est_tokens: 210,
            }],
        }
    }

    #[test]
    fn every_row_leads_with_its_status() {
        let toon = context_toon(&sample());
        assert!(toon.contains("  INFERRED|0.80|"), "{toon}");
        assert!(toon.contains("  CONFLICTED|approval|"), "{toon}");
    }

    #[test]
    fn the_delimiter_cannot_be_forged_by_content() {
        // A claim containing `|` must not create phantom columns.
        let toon = context_toon(&sample());
        assert!(
            toon.contains("corporate orders / require approval"),
            "{toon}"
        );
    }

    #[test]
    fn the_envelope_cost_is_measured_from_the_actual_bytes() {
        let toon = context_toon(&sample());
        let header = toon.lines().next().unwrap().to_string();
        let body: String = toon.lines().skip(1).collect::<Vec<_>>().join("\n") + "\n";
        let claimed: u32 = header
            .split("envelope~")
            .nth(1)
            .and_then(|r| r.split("tok").next())
            .and_then(|n| n.parse().ok())
            .expect("header carries the envelope estimate");
        let measured = reify::tokens::estimate(&body);
        assert!(
            claimed.abs_diff(measured) <= measured / 10 + 2,
            "claimed {claimed} vs measured {measured}"
        );
    }

    #[test]
    fn unknowns_and_the_verification_note_survive_compression() {
        let toon = context_toon(&sample());
        assert!(toon.contains("no document describes this"));
        assert!(toon.contains("INFERRED claims are leads to verify"));
    }
}

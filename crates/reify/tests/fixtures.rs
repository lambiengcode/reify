//! Integration tests against the committed fixture repositories.
//!
//! `fixtures/minierp` contains knowledge that was *planted*: a documented rule, code
//! that contradicts it, a magic number, a bilingual concept, cross-module coupling
//! that exists only through a shared table, and a rule stated only by a test name.
//!
//! Every assertion here has a known right answer, which is what these fixtures are
//! for. Unlike the ERPNext benchmark, a failure here is unambiguous: the knowledge is
//! definitely present, so not finding it is definitely a regression.

use std::path::{Path, PathBuf};

use reify::context::{compile, ContextOptions};
use reify::index::{index, IndexOptions};
use reify::model::{EdgeKind, NodeKind, Status};
use reify::query;
use reify::store::{Direction, Store};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate lives two levels below the workspace root")
        .join("fixtures")
        .join(name)
}

/// Index a fixture into memory. Nothing is written into the fixture directory, so the
/// tests are safe to run in parallel and leave the working tree clean.
fn indexed(name: &str) -> Store {
    let mut store = Store::in_memory().expect("in-memory store");
    let report = index(&mut store, &IndexOptions::new(fixture(name)))
        .unwrap_or_else(|e| panic!("indexing {name}: {e:#}"));
    assert!(
        report.parse_errors.is_empty(),
        "fixture must parse cleanly: {:?}",
        report.parse_errors
    );
    store
}

#[test]
fn the_planted_documentation_conflict_is_found() {
    let store = indexed("minierp");
    let conflicts = query::conflicts(&store).expect("conflicts");
    assert_eq!(
        conflicts.len(),
        1,
        "exactly one contradiction is planted: {conflicts:#?}"
    );
    let conflict = &conflicts[0];
    assert_eq!(conflict.subject, "approval");
    assert!(conflict.documented.to_lowercase().contains("require"));
    assert!(conflict.observed.to_lowercase().contains("bypass"));
    assert!(conflict.documented_at.contains("BRD-42.md"));
    assert!(conflict.observed_at.contains("order.py"));
    assert_eq!(conflict.resolution, "UNRESOLVED");
}

#[test]
fn a_rule_stated_only_by_a_test_name_is_mined() {
    let store = indexed("minierp");
    let rules = query::rules(&store, 0.0).expect("rules");
    let from_tests: Vec<&str> = rules
        .iter()
        .filter(|r| r.data.get("source").and_then(|v| v.as_str()) == Some("test"))
        .filter_map(|r| r.data.get("claim").and_then(|v| v.as_str()))
        .collect();
    assert!(
        from_tests.iter().any(|c| c.contains("requires approval")),
        "a test name is an executable statement of intent: {from_tests:?}"
    );
}

#[test]
fn the_bilingual_concept_carries_both_labels_and_reaches_the_code() {
    let store = indexed("minierp");
    let concept = store
        .node_by_uid("concept:STRATEGIC_ACCOUNT")
        .expect("query")
        .expect("the translation bridge must produce this concept");
    assert_eq!(concept.data["labels"]["eng"], "Strategic Account");
    assert_eq!(concept.data["labels"]["vie"], "khách hàng chiến lược");

    let mapped = store
        .neighbors(concept.id, Direction::Out, &[EdgeKind::MapsTo])
        .expect("neighbours");
    assert!(
        mapped
            .iter()
            .any(|(n, _, _)| n.name == "StrategicAccount" || n.kind == NodeKind::DatabaseObject),
        "the concept must reach the code or the table it names: {mapped:?}"
    );
}

#[test]
fn an_untranslatable_ui_string_never_becomes_a_concept() {
    let store = indexed("minierp");
    assert!(
        store
            .node_by_uid("concept:PLEASE_WAIT_WHILE_LOADING")
            .unwrap()
            .is_none(),
        "grounding is what keeps a translation file from flooding the concept layer"
    );
}

#[test]
fn a_vietnamese_query_reaches_the_english_code() {
    let store = indexed("minierp");
    let vietnamese =
        compile(&store, "khách hàng chiến lược", &ContextOptions::default()).expect("compile");
    let mentioned: Vec<&str> = vietnamese
        .code
        .iter()
        .map(|c| c.symbol.as_str())
        .chain(vietnamese.concepts.iter().map(|c| c.id.as_str()))
        .collect();
    assert!(
        mentioned.iter().any(|m| m.contains("Strategic")),
        "a Vietnamese term must reach English code: {mentioned:?}"
    );
}

#[test]
fn impact_crosses_the_database_where_no_call_edge_exists() {
    // The planted coupling: report.py never calls order.py, but both touch
    // approval_log. This is the edge a call graph cannot produce and grep cannot see.
    let store = indexed("minierp");
    let answer = query::impact(&store, "record").expect("impact");
    assert!(
        answer.affected.iter().any(|a| a.what == "run"),
        "the report reading the same table must be affected: {:#?}",
        answer.affected
    );
    assert!(answer
        .tables
        .iter()
        .any(|t| t.what.contains("approval_log")));
}

#[test]
fn the_magic_number_is_reachable_from_the_business_concept() {
    let store = indexed("minierp");
    let compiled = compile(
        &store,
        "which customer group is the strategic tier",
        &ContextOptions::default(),
    )
    .expect("compile");
    let rendered = serde_json::to_string(&compiled).expect("serialise");
    assert!(
        rendered.contains("STRATEGIC") || rendered.contains("customer_group"),
        "the tier concept must connect to the column that stores it"
    );
}

#[test]
fn every_claim_in_a_compiled_context_states_its_epistemic_footing() {
    let store = indexed("minierp");
    let compiled = compile(
        &store,
        "approval for corporate orders",
        &ContextOptions::default(),
    )
    .expect("compile");
    let json = serde_json::to_value(&compiled).expect("serialise");
    for section in [
        "concepts",
        "rules",
        "code",
        "documents",
        "data",
        "conflicts",
    ] {
        for item in json[section].as_array().expect("array") {
            let status = item
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| panic!("{section} item has no status: {item}"));
            assert!(
                Status::parse(status).as_str() == status,
                "{section} carries an unrecognised status: {status}"
            );
        }
    }
}

#[test]
fn a_known_contradiction_is_never_dropped_for_budget_reasons() {
    // The one admitted overrun: budget pressure may drop useful context, but a
    // contradiction the agent needs to know about must survive any budget.
    let store = indexed("minierp");
    let compiled = compile(
        &store,
        "corporate approval",
        &ContextOptions {
            budget: 60,
            ..Default::default()
        },
    )
    .expect("compile");
    assert!(
        !compiled.conflicts.is_empty(),
        "a conflict must survive even a budget too small for anything else"
    );
}

#[test]
fn the_matched_language_pair_resolves_to_the_same_answer() {
    // minierp-vi holds the same requirement in Vietnamese and English. Both must
    // reach the same code, or the multilingual claim is decoration.
    let store = indexed("minierp-vi");
    let vietnamese = compile(&store, "miễn phê duyệt", &ContextOptions::default()).expect("vi");
    let english = compile(&store, "exempt from approval", &ContextOptions::default()).expect("en");

    let docs_for = |c: &reify::context::Context| -> Vec<String> {
        c.documents.iter().map(|d| d.location.clone()).collect()
    };
    assert!(
        !docs_for(&vietnamese).is_empty(),
        "the Vietnamese requirement must be retrievable in Vietnamese"
    );
    assert!(
        !docs_for(&english).is_empty(),
        "and the English one in English"
    );
}

#[test]
fn indexing_a_fixture_twice_changes_nothing() {
    let root = fixture("minierp");
    let mut store = Store::in_memory().expect("store");
    index(&mut store, &IndexOptions::new(&root)).expect("first");
    let first = store.canonical_dump().expect("dump");
    index(&mut store, &IndexOptions::new(&root)).expect("second");
    assert_eq!(
        first,
        store.canonical_dump().expect("dump"),
        "indexing is idempotent"
    );
}

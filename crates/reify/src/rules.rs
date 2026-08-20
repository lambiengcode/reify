//! Business-rule candidates and conflict detection.
//!
//! Rule *candidates* are generated deterministically — from guard clauses, validation
//! errors, test names, DDL constraints and modal sentences in documents. No model is
//! involved, and every candidate carries the evidence that produced it. Phrasing a
//! candidate more fluently is the one place an LLM would help, and it is optional.
//!
//! Precision is weighted far above recall throughout (`docs/PLAN.md` §N.6). A missing
//! rule costs an agent one search. A wrong rule stated confidently costs an incident.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::OnceLock;

use regex::Regex;

use crate::concepts::{meaningful_words, same_word};
use crate::model::{uid, EdgeKind, NewEdge, NewNode, NodeKind, Status};
use crate::store::{Batch, NewEvidence};

/// A business rule is one sentence an engineer can agree or disagree with.
///
/// Release-note files and long paragraphs otherwise produce single "sentences"
/// thousands of characters long that contain every word in the domain, and those match
/// everything. Length is the cheapest and most reliable filter against them.
const MAX_CLAIM_CHARS: usize = 220;
const MAX_CLAIM_WORDS: usize = 40;
const MIN_CLAIM_WORDS: usize = 4;

/// Domain words two claims must share, beyond the action they both name, before they
/// are treated as talking about the same thing.
///
/// One shared word is far too weak in a large repository: "order" alone connects
/// thousands of unrelated claims.
const MIN_SHARED_WORDS: usize = 2;

/// Lines of the guarded branch read together with an `if` condition.
const GUARD_WINDOW_LINES: usize = 4;

/// Below this, a candidate is kept in the store but not surfaced by default.
pub const MIN_SURFACED_CONFIDENCE: f32 = 0.5;

/// A conflict needs both sides to be this confident before it is reported.
///
/// Conflict detection is deliberately biased toward silence: a detector that cries
/// wolf gets disabled in week two, and its true positives are lost with it.
///
/// Calibrated against [`RuleSource::Document`], whose base strength is 0.7. A higher
/// threshold would make an uncorroborated documented claim structurally incapable of
/// raising a conflict, which would silently disable the feature rather than tighten
/// it. The conservatism lives in the five structural conditions in
/// [`detect_conflicts`], not in an unreachable number.
pub const CONFLICT_MIN_CONFIDENCE: f32 = 0.7;

/// Where a rule candidate came from, and how much that source is worth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleSource {
    /// A test name or assertion: an executable statement of intent.
    Test,
    /// A database constraint: enforced by the engine itself.
    Constraint,
    /// A guard clause or validation branch in code.
    CodeGuard,
    /// An error or exception message with domain language.
    ErrorMessage,
    /// A modal sentence in a docstring or code comment.
    ///
    /// Deliberately distinct from [`RuleSource::Document`]: a docstring lives with the
    /// code it describes, so a docstring disagreeing with nearby code is a stale
    /// comment, not the documentation/implementation contradiction Reify reports.
    CodeDoc,
    /// A modal sentence in a specification document ("must", "shall", "phải").
    Document,
}

impl RuleSource {
    /// Base confidence contributed by this source kind.
    pub fn strength(self) -> f32 {
        match self {
            RuleSource::Test => 0.9,
            RuleSource::Constraint => 0.9,
            RuleSource::CodeGuard => 0.8,
            RuleSource::ErrorMessage => 0.75,
            RuleSource::CodeDoc => 0.72,
            RuleSource::Document => 0.7,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            RuleSource::Test => "test",
            RuleSource::Constraint => "constraint",
            RuleSource::CodeGuard => "code_guard",
            RuleSource::ErrorMessage => "error_message",
            RuleSource::CodeDoc => "code_doc",
            RuleSource::Document => "document",
        }
    }
}

/// What a rule does when its condition holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Polarity {
    /// Imposes the action: require, must, block, reject.
    Require,
    /// Removes the action: bypass, skip, exempt, allow without.
    Bypass,
}

impl Polarity {
    fn opposite_of(self, other: Polarity) -> bool {
        self != other
    }
}

/// One candidate business rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleCandidate {
    pub id: String,
    /// A single sentence an engineer can agree or disagree with.
    pub claim: String,
    /// The action verb the rule governs, normalised: `approval`, `discount`, ...
    pub subject: String,
    pub polarity: Polarity,
    pub source: RuleSource,
    pub status: Status,
    pub confidence: f32,
    /// `path:line` or `doc#section`.
    pub location: String,
    /// Uid of the symbol or document section this rule was read from.
    pub anchor: String,
    /// Concept words the claim mentions, used to match rules across sources.
    pub concepts: BTreeSet<String>,
    /// The file this rule was mined from, so it is invalidated with that file.
    pub path: String,
}

impl RuleCandidate {
    fn new(
        claim: String,
        subject: String,
        polarity: Polarity,
        source: RuleSource,
        location: String,
        anchor: String,
    ) -> RuleCandidate {
        let concepts = meaningful_words(&claim);
        // The id is content-derived so the same rule keeps its identity across
        // reindexing, and so two extractions of the same rule collapse into one.
        let digest =
            blake3::hash(format!("{subject}|{polarity:?}|{}", normalize(&claim)).as_bytes())
                .to_hex()
                .to_string();
        RuleCandidate {
            id: uid::rule(&digest),
            path: location.split(':').next().unwrap_or(&location).to_string(),
            claim,
            subject,
            polarity,
            source,
            status: Status::Inferred,
            confidence: source.strength(),
            location,
            anchor,
            concepts,
        }
    }
}

/// A business action a rule can govern.
///
/// Kept explicit and small: an open-ended verb list produces open-ended false
/// positives, and every entry here is one an engineer would recognise as a business
/// action rather than a programming one.
struct Subject {
    /// Canonical name, used to match rules across sources.
    name: &'static str,
    /// Surface forms, in every language documents are written in.
    terms: &'static [&'static str],
    require: &'static [&'static str],
    bypass: &'static [&'static str],
}

const SUBJECTS: &[Subject] = &[
    Subject {
        name: "approval",
        terms: &[
            "approval",
            "approve",
            "approved",
            "phê duyệt",
            "duyệt",
            "承認",
        ],
        require: &["require", "requires", "required", "need", "needs", "must"],
        bypass: &["bypass", "skip", "exempt", "waive", "without"],
    },
    Subject {
        name: "discount",
        terms: &["discount", "chiết khấu", "giảm giá", "割引"],
        require: &["apply", "applies", "grant", "grants", "receive", "receives"],
        bypass: &["deny", "denies", "exclude", "excludes", "no"],
    },
    Subject {
        name: "validation",
        terms: &["validation", "validate", "kiểm tra", "検証"],
        require: &[
            "validate",
            "validates",
            "reject",
            "rejects",
            "block",
            "blocks",
        ],
        bypass: &["allow", "allows", "permit", "permits", "ignore"],
    },
    Subject {
        name: "credit limit",
        terms: &["credit limit", "hạn mức", "与信限度"],
        require: &["enforce", "enforces", "check", "checks", "block", "blocks"],
        bypass: &["override", "overrides", "ignore", "ignores"],
    },
];

/// Polarity words that apply to every subject, in every supported language.
///
/// Business documents in the target repositories are frequently not in English, and a
/// rule miner that only understands English modals silently mines nothing from them.
const REQUIRE_WORDS: &[&str] = &["must", "shall", "mandatory", "phải", "cần", "bắt buộc"];
const BYPASS_WORDS: &[&str] = &["exempt", "exempted", "miễn", "bỏ", "qua", "免除"];

fn modal_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Obligation markers, in every language the target documents are written in.
        //
        // Exemption language is included alongside the modals on purpose. "Strategic
        // accounts are exempt from L2 approval" states a rule as firmly as "orders
        // must be approved", but carries no modal — and exemptions are precisely the
        // clauses that contradict the general rule, so a modal-only gate is blind to
        // the most valuable half of the corpus.
        Regex::new(
            r"(?i)\b(must|shall|should|required to|may not|cannot|is required|are required|exempt|exempted|not required|no longer required|phải|không được|bắt buộc|được miễn|miễn)\b",
        )
        .expect("modal regex is a compile-time constant")
    })
}

fn test_name_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^(test_|should_|it_|test)").expect("test regex is a compile-time constant")
    })
}

/// Mine rule candidates from a symbol: its name, signature and documentation.
pub fn from_symbol(
    name: &str,
    qualified: &str,
    doc: Option<&str>,
    body: &str,
    location: &str,
    anchor: &str,
) -> Vec<RuleCandidate> {
    let mut out = Vec::new();

    // A test name is an executable statement of intent, and the highest-precision
    // rule source there is: someone wrote it down and CI keeps it true.
    if test_name_re().is_match(name) {
        let phrase = humanise(name);
        if let Some((subject, polarity)) = classify_phrase(&phrase) {
            push(
                &mut out,
                phrase,
                subject,
                polarity,
                RuleSource::Test,
                location,
                anchor,
            );
        }
    }

    // A guard clause naming a business action.
    //
    // The condition and the branch it guards are read together: the business action
    // usually lives in the branch (`if is_strategic: bypass_approval()`), so looking
    // at the condition alone finds almost nothing.
    let lines: Vec<&str> = body.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if !(trimmed.starts_with("if ")
            || trimmed.starts_with("if(")
            || trimmed.starts_with("elif "))
        {
            continue;
        }
        let window = lines[i..(i + GUARD_WINDOW_LINES).min(lines.len())].join(" ");
        let phrase = humanise(&window);
        if let Some((subject, polarity)) = classify_phrase(&phrase) {
            push(
                &mut out,
                format!("{}: {}", humanise(qualified), phrase),
                subject,
                polarity,
                RuleSource::CodeGuard,
                location,
                anchor,
            );
        }
    }

    // A docstring stating an obligation.
    if let Some(doc) = doc {
        for sentence in sentences(doc) {
            if !modal_re().is_match(&sentence) {
                continue;
            }
            if let Some((subject, polarity)) = classify_phrase(&sentence) {
                push(
                    &mut out,
                    sentence,
                    subject,
                    polarity,
                    RuleSource::CodeDoc,
                    location,
                    anchor,
                );
            }
        }
    }

    dedupe(out)
}

/// Mine rule candidates from a document section's prose.
pub fn from_document(text: &str, location: &str, anchor: &str) -> Vec<RuleCandidate> {
    let mut out = Vec::new();
    for sentence in sentences(text) {
        if !modal_re().is_match(&sentence) {
            continue;
        }
        if let Some((subject, polarity)) = classify_phrase(&sentence) {
            push(
                &mut out,
                sentence,
                subject,
                polarity,
                RuleSource::Document,
                location,
                anchor,
            );
        }
    }
    dedupe(out)
}

/// Stage a candidate, rejecting claims too long to be a single checkable rule.
#[allow(clippy::too_many_arguments)]
fn push(
    out: &mut Vec<RuleCandidate>,
    claim: String,
    subject: String,
    polarity: Polarity,
    source: RuleSource,
    location: &str,
    anchor: &str,
) {
    if !is_checkable_claim(&claim) {
        return;
    }
    out.push(RuleCandidate::new(
        claim,
        subject,
        polarity,
        source,
        location.to_string(),
        anchor.to_string(),
    ));
}

/// Is this short enough that a human could verify it against the cited evidence?
fn is_checkable_claim(claim: &str) -> bool {
    let words = claim.split_whitespace().count();
    claim.chars().count() <= MAX_CLAIM_CHARS && (MIN_CLAIM_WORDS..=MAX_CLAIM_WORDS).contains(&words)
}

/// Raise confidence when independent source kinds agree about the same rule.
///
/// Agreement across *kinds* only. Two code guards saying the same thing is one fact
/// duplicated; a document and a test agreeing is corroboration.
pub fn corroborate(candidates: &mut [RuleCandidate]) {
    let keys: Vec<(String, Polarity, RuleSource)> = candidates
        .iter()
        .map(|c| (c.subject.clone(), c.polarity, c.source))
        .collect();
    for (i, candidate) in candidates.iter_mut().enumerate() {
        let corroborating = keys
            .iter()
            .enumerate()
            .filter(|(j, (subject, polarity, source))| {
                *j != i
                    && subject == &candidate.subject
                    && *polarity == candidate.polarity
                    && *source != candidate.source
            })
            .map(|(_, (_, _, source))| *source)
            .collect::<BTreeSet<_>>();
        if !corroborating.is_empty() {
            let factor = 1.0 + 0.15 * corroborating.len() as f32;
            candidate.confidence = (candidate.confidence * factor).min(0.97);
            // Corroboration across independent source kinds is what promotes a guess
            // to an observation.
            if corroborating.len() >= 2 {
                candidate.status = Status::Observed;
            }
        }
    }
}

/// A documented claim and an implemented claim that disagree.
#[derive(Debug, Clone, Serialize)]
pub struct Conflict {
    pub id: String,
    pub subject: String,
    pub documented: String,
    pub documented_at: String,
    pub observed: String,
    pub observed_at: String,
    pub status: Status,
    pub confidence: f32,
    pub resolution: &'static str,
}

/// Detect conflicts, conservatively.
///
/// All five conditions must hold (`docs/PLAN.md` §G.6):
/// same subject, opposite polarity, both sides above threshold, different source
/// kinds, and overlapping concept vocabulary. Anything weaker is a divergence, and a
/// divergence is not reported.
pub fn detect_conflicts(candidates: &[RuleCandidate]) -> Vec<Conflict> {
    let mut out: Vec<Conflict> = Vec::new();
    for (i, a) in candidates.iter().enumerate() {
        for b in candidates.iter().skip(i + 1) {
            if a.subject != b.subject || !a.polarity.opposite_of(b.polarity) {
                continue;
            }
            if a.confidence < CONFLICT_MIN_CONFIDENCE || b.confidence < CONFLICT_MIN_CONFIDENCE {
                continue;
            }
            // Same source kind means a branch or an exception, not a contradiction.
            if a.source == b.source {
                continue;
            }
            // The two claims must be about the same thing *beyond* the action they
            // both name. Without this, "corporate customers require approval" and
            // "timesheet entries bypass approval" look like a contradiction purely
            // because both contain the word "approval".
            // Compared with inflection folded away: a document writes "corporate
            // customers require" where the code writes "customer", and treating those
            // as different words silently makes the detector unable to see the most
            // common shape of a real contradiction.
            let subject_words = subject_vocabulary(&a.subject);
            let shared = a
                .concepts
                .iter()
                .filter(|w| !subject_words.iter().any(|s| same_word(s, w)))
                .filter(|w| b.concepts.iter().any(|o| same_word(o, w)))
                .map(|w| crate::concepts::stem(w))
                .collect::<BTreeSet<&str>>()
                .len();
            if shared < MIN_SHARED_WORDS {
                continue;
            }
            let (documented, observed) = if a.source == RuleSource::Document {
                (a, b)
            } else if b.source == RuleSource::Document {
                (b, a)
            } else {
                continue; // code disagreeing with code is a branch, not a conflict
            };
            let digest = blake3::hash(format!("{}|{}", documented.id, observed.id).as_bytes())
                .to_hex()
                .to_string();
            out.push(Conflict {
                id: format!("conflict:{}", &digest[..8]),
                subject: a.subject.clone(),
                documented: documented.claim.clone(),
                documented_at: documented.location.clone(),
                observed: observed.claim.clone(),
                observed_at: observed.location.clone(),
                status: Status::Conflicted,
                confidence: documented.confidence.min(observed.confidence),
                resolution: "UNRESOLVED",
            });
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Stage rules and conflicts as nodes, linked to the code and documents they came from.
pub fn stage(candidates: &[RuleCandidate], conflicts: &[Conflict]) -> Batch {
    let mut batch = Batch::default();
    for rule in candidates {
        let line = rule
            .location
            .rsplit(':')
            .next()
            .and_then(|l| l.parse::<u32>().ok())
            .unwrap_or(0);
        batch.node(
            NewNode::new(&rule.id, NodeKind::BusinessRule, &rule.claim)
                .at(&rule.path, line, line)
                .status(rule.status, rule.confidence)
                .search(format!("{} {}", rule.claim, rule.subject))
                .data(serde_json::json!({
                    "claim": rule.claim,
                    "subject": rule.subject,
                    "polarity": rule.polarity,
                    "source": rule.source.as_str(),
                    "location": rule.location,
                    "summary": rule.claim,
                })),
        );
        batch.edge(NewEdge::new(
            rule.anchor.clone(),
            rule.id.clone(),
            EdgeKind::ImplementsRule,
            rule.status,
            rule.confidence,
        ));
        batch.evidence(NewEvidence {
            node_uid: rule.id.clone(),
            source: rule.location.clone(),
            locator: rule.location.clone(),
            kind: rule.source.as_str(),
        });
    }
    for conflict in conflicts {
        batch.node(
            NewNode::new(&conflict.id, NodeKind::BusinessRule, &conflict.subject)
                .status(Status::Conflicted, conflict.confidence)
                .search(format!("{} {}", conflict.documented, conflict.observed))
                .data(serde_json::json!({
                    "conflict": true,
                    "documented": conflict.documented,
                    "documented_at": conflict.documented_at,
                    "observed": conflict.observed,
                    "observed_at": conflict.observed_at,
                    "resolution": conflict.resolution,
                    "summary": format!("documentation and code disagree about {}", conflict.subject),
                })),
        );
    }
    batch
}

/// Every word that names a subject, so it can be excluded from shared-vocabulary tests.
fn subject_vocabulary(name: &str) -> BTreeSet<String> {
    SUBJECTS
        .iter()
        .filter(|s| s.name == name)
        .flat_map(|s| s.terms.iter())
        .flat_map(|term| meaningful_words(term))
        .collect()
}

// ---- text helpers -----------------------------------------------------------

/// Match a phrase against the subject table, returning the action and its polarity.
fn classify_phrase(phrase: &str) -> Option<(String, Polarity)> {
    let lowered = phrase.to_lowercase();
    for subject in SUBJECTS {
        if !subject.terms.iter().any(|t| lowered.contains(t)) {
            continue;
        }
        let signals = |words: &[&str], shared: &[&str]| {
            words
                .iter()
                .chain(shared.iter())
                .any(|w| contains_word(&lowered, w))
        };
        // Bypass wins ties: "requires approval unless exempt" is an exemption rule,
        // and treating it as a plain requirement is the more damaging misreading.
        if signals(subject.bypass, BYPASS_WORDS) {
            return Some((subject.name.to_string(), Polarity::Bypass));
        }
        if signals(subject.require, REQUIRE_WORDS) {
            return Some((subject.name.to_string(), Polarity::Require));
        }
    }
    None
}

/// Whole-word containment, so "no" does not match inside "notification".
///
/// Multi-word needles ("không được") fall back to substring containment, since word
/// splitting cannot express them.
fn contains_word(haystack: &str, needle: &str) -> bool {
    if needle.contains(' ') {
        return haystack.contains(needle);
    }
    haystack
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|w| w == needle)
}

/// Turn an identifier or code line into something readable.
fn humanise(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| if c == '_' { ' ' } else { c })
        .collect();
    let words: Vec<String> = cleaned
        .split_whitespace()
        .flat_map(|w| {
            if w.chars().any(|c| c.is_uppercase()) && w.chars().any(|c| c.is_lowercase()) {
                crate::extract::code::split_identifier(w)
            } else {
                vec![w
                    .trim_matches(|c: char| c == ':' || c == '(' || c == ')')
                    .to_string()]
            }
        })
        .filter(|w| !w.is_empty())
        .collect();
    normalize(&words.join(" "))
}

/// Split prose into candidate sentences.
///
/// Semicolons and newlines count as terminators, because bullet lists and release
/// notes frequently contain no full stop at all and would otherwise arrive as one
/// enormous "sentence".
fn sentences(text: &str) -> Vec<String> {
    text.split(['.', '!', '?', ';', '\n', '•'])
        .map(normalize)
        .filter(|s| is_checkable_claim(s))
        .collect()
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Collapse candidates that say the same thing about the same anchor.
///
/// Deduping on meaning — `(subject, polarity, source, anchor)` — rather than on the
/// exact claim text: a function that bypasses approval in two branches is stating one
/// rule twice, not two rules. The shortest claim is kept, being the least cluttered
/// with surrounding code.
fn dedupe(mut candidates: Vec<RuleCandidate>) -> Vec<RuleCandidate> {
    candidates.sort_by(|a, b| {
        (&a.subject, a.polarity as u8, a.source as u8, &a.anchor)
            .cmp(&(&b.subject, b.polarity as u8, b.source as u8, &b.anchor))
            .then(a.claim.len().cmp(&b.claim.len()))
            .then(a.claim.cmp(&b.claim))
    });
    candidates.dedup_by(|a, b| {
        a.subject == b.subject
            && a.polarity == b.polarity
            && a.source == b.source
            && a.anchor == b.anchor
    });
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_rule(claim: &str) -> RuleCandidate {
        let (subject, polarity) = classify_phrase(claim).expect("test claim must classify");
        RuleCandidate::new(
            claim.into(),
            subject,
            polarity,
            RuleSource::Document,
            "docs/BRD.md#4-2".into(),
            "doc:docs/BRD.md#4-2".into(),
        )
    }

    fn code_rule(claim: &str) -> RuleCandidate {
        let (subject, polarity) = classify_phrase(claim).expect("test claim must classify");
        RuleCandidate::new(
            claim.into(),
            subject,
            polarity,
            RuleSource::CodeGuard,
            "app/order.py:812".into(),
            "sym:app/order.py#f".into(),
        )
    }

    #[test]
    fn a_requirement_and_an_exemption_get_opposite_polarity() {
        assert_eq!(
            classify_phrase("corporate orders require approval"),
            Some(("approval".into(), Polarity::Require))
        );
        assert_eq!(
            classify_phrase("strategic accounts bypass approval"),
            Some(("approval".into(), Polarity::Bypass))
        );
    }

    #[test]
    fn an_exemption_wins_over_a_requirement_in_the_same_sentence() {
        // "requires approval unless exempt" is an exemption rule; reading it as a
        // plain requirement is the more damaging mistake.
        assert_eq!(
            classify_phrase("orders require approval unless the account is exempt"),
            Some(("approval".into(), Polarity::Bypass))
        );
    }

    #[test]
    fn prose_about_nothing_business_shaped_classifies_as_nothing() {
        assert_eq!(classify_phrase("the cache is warmed on startup"), None);
        assert_eq!(classify_phrase("returns a list of rows"), None);
    }

    #[test]
    fn substring_matches_do_not_trigger_a_polarity() {
        // "no" inside "notification" must not make this a bypass rule.
        assert!(!contains_word("send a discount notification", "no"));
    }

    #[test]
    fn a_test_name_becomes_a_rule_candidate() {
        let rules = from_symbol(
            "test_corporate_order_requires_approval",
            "TestOrders.test_corporate_order_requires_approval",
            None,
            "",
            "tests/test_order.py:12",
            "sym:tests/test_order.py#t",
        );
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].source, RuleSource::Test);
        assert_eq!(rules[0].polarity, Polarity::Require);
        assert!(rules[0].claim.contains("requires approval"));
        assert!(rules[0].confidence >= 0.9);
    }

    #[test]
    fn a_guard_clause_becomes_a_rule_candidate() {
        let body =
            "def check(self):\n    if self.is_strategic:\n        return self.bypass_approval()\n";
        let rules = from_symbol(
            "check",
            "Order.check",
            None,
            body,
            "app/order.py:10",
            "sym:app/order.py#Order.check",
        );
        assert!(
            rules.iter().any(|r| r.source == RuleSource::CodeGuard),
            "got {rules:?}"
        );
    }

    #[test]
    fn a_guard_reads_the_branch_it_guards_not_only_its_condition() {
        let body = "if order.is_strategic:\n    return self.bypass_approval()\n";
        let rules = from_symbol("check", "Order.check", None, body, "a.py:1", "sym:a.py#c");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].polarity, Polarity::Bypass);
    }

    #[test]
    fn vietnamese_subject_terms_are_recognised_without_english() {
        assert_eq!(
            classify_phrase("đơn hàng doanh nghiệp phải phê duyệt cấp hai"),
            Some(("approval".into(), Polarity::Require))
        );
        assert_eq!(
            classify_phrase("khách hàng chiến lược được miễn phê duyệt"),
            Some(("approval".into(), Polarity::Bypass))
        );
    }

    #[test]
    fn ordinary_control_flow_does_not_become_a_business_rule() {
        let body = "def go(self):\n    if x is None:\n        return 0\n    if len(rows) > 10:\n        return rows[:10]\n";
        let rules = from_symbol("go", "A.go", None, body, "a.py:1", "sym:a.py#A.go");
        assert!(
            rules.is_empty(),
            "false positives are the expensive failure: {rules:?}"
        );
    }

    #[test]
    fn a_modal_sentence_in_a_document_becomes_a_rule_candidate() {
        let rules = from_document(
            "Background text. Corporate orders must require approval at level two. Nothing else.",
            "docs/BRD.md#4-2",
            "doc:docs/BRD.md#4-2",
        );
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].source, RuleSource::Document);
        assert_eq!(rules[0].polarity, Polarity::Require);
    }

    #[test]
    fn a_vietnamese_obligation_sentence_is_recognised() {
        let rules = from_document(
            "Đơn hàng doanh nghiệp phải có approval cấp hai trước khi xử lý",
            "docs/BRD-vi.md#1",
            "doc:docs/BRD-vi.md#1",
        );
        assert_eq!(rules.len(), 1, "Vietnamese modals must be detected too");
    }

    #[test]
    fn a_paragraph_sized_claim_is_rejected() {
        // Release notes and changelogs produce "sentences" thousands of characters
        // long that contain every word in the domain and therefore match everything.
        let blob = "must ".to_string() + &"approval item order discount ".repeat(40);
        let rules = from_document(&blob, "docs/CHANGELOG.md#1", "doc:docs/CHANGELOG.md#1");
        assert!(rules.is_empty(), "got {rules:#?}");
    }

    #[test]
    fn claims_are_split_on_semicolons_and_bullets() {
        let text =
            "Corporate orders must require approval; strategic accounts are exempt from approval";
        let rules = from_document(text, "docs/BRD.md#1", "doc:docs/BRD.md#1");
        assert_eq!(rules.len(), 2, "got {rules:#?}");
    }

    #[test]
    fn a_docstring_is_code_documentation_not_a_specification() {
        // A stale comment next to the code it describes is not the
        // documentation/implementation contradiction Reify reports.
        let rules = from_symbol(
            "check",
            "Order.check",
            Some("Corporate customers must require approval before confirmation."),
            "",
            "app/order.py:10",
            "sym:app/order.py#Order.check",
        );
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].source, RuleSource::CodeDoc);
    }

    #[test]
    fn a_stale_docstring_does_not_raise_a_documentation_conflict() {
        let doc_comment = RuleCandidate::new(
            "corporate customers must require approval".into(),
            "approval".into(),
            Polarity::Require,
            RuleSource::CodeDoc,
            "app/order.py:1".into(),
            "sym:app/order.py#f".into(),
        );
        let code = code_rule("corporate customers bypass approval");
        assert!(detect_conflicts(&[doc_comment, code]).is_empty());
    }

    #[test]
    fn a_plural_in_the_document_still_matches_the_singular_in_the_code() {
        let rules = vec![
            doc_rule("corporate customers must require approval for every order"),
            code_rule("corporate customer order bypass approval"),
        ];
        assert_eq!(
            detect_conflicts(&rules).len(),
            1,
            "inflection must not hide a real contradiction"
        );
    }

    #[test]
    fn one_shared_word_is_not_enough_to_call_two_claims_a_conflict() {
        let rules = vec![
            doc_rule("corporate orders must require approval"),
            code_rule("draft orders bypass approval"),
        ];
        assert!(
            detect_conflicts(&rules).is_empty(),
            "only \"orders\" is shared; that connects thousands of unrelated claims"
        );
    }

    #[test]
    fn a_sentence_without_a_modal_is_not_a_rule() {
        let rules = from_document(
            "Corporate orders are processed nightly by the approval batch job.",
            "docs/BRD.md#1",
            "doc:docs/BRD.md#1",
        );
        assert!(rules.is_empty());
    }

    #[test]
    fn an_exemption_without_a_modal_is_still_a_rule() {
        // Exemptions are the clauses that contradict the general rule, so missing
        // them costs more than any other recall gap in the miner.
        let rules = from_document(
            "Strategic accounts are exempt from level two approval.",
            "docs/BRD.md#4-2",
            "doc:docs/BRD.md#4-2",
        );
        assert_eq!(rules.len(), 1, "got {rules:#?}");
        assert_eq!(rules[0].polarity, Polarity::Bypass);
    }

    #[test]
    fn identical_rules_from_one_symbol_collapse_to_one_id() {
        let body = "if strategic: bypass_approval()\nif strategic: bypass_approval()\n";
        let rules = from_symbol("f", "A.f", None, body, "a.py:1", "sym:a.py#A.f");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn two_branches_stating_one_rule_collapse_to_one_candidate() {
        let body = "if a.is_strategic:\n    return bypass_approval()\nif b.is_strategic:\n    return bypass_approval()\n";
        let rules = from_symbol("f", "A.f", None, body, "a.py:1", "sym:a.py#A.f");
        assert_eq!(rules.len(), 1, "one rule stated twice is one rule");
    }

    #[test]
    fn rule_ids_are_content_derived_and_stable() {
        let a = doc_rule("corporate orders require approval");
        let b = doc_rule("corporate  orders   require approval");
        assert_eq!(a.id, b.id, "whitespace must not change a rule's identity");
    }

    #[test]
    fn corroboration_across_source_kinds_promotes_a_guess_to_an_observation() {
        let mut rules = vec![
            doc_rule("corporate orders require approval"),
            code_rule("corporate orders require approval"),
            RuleCandidate::new(
                "corporate order requires approval".into(),
                "approval".into(),
                Polarity::Require,
                RuleSource::Test,
                "tests/t.py:1".into(),
                "sym:tests/t.py#t".into(),
            ),
        ];
        corroborate(&mut rules);
        assert!(rules.iter().all(|r| r.status == Status::Observed));
        assert!(rules.iter().all(|r| r.confidence > 0.8));
        assert!(rules.iter().all(|r| r.confidence <= 0.97));
    }

    #[test]
    fn duplicates_within_one_source_kind_do_not_corroborate() {
        let mut rules = vec![
            code_rule("corporate orders require approval"),
            RuleCandidate::new(
                "corporate orders require approval here".into(),
                "approval".into(),
                Polarity::Require,
                RuleSource::CodeGuard,
                "app/other.py:9".into(),
                "sym:app/other.py#g".into(),
            ),
        ];
        let before = rules[0].confidence;
        corroborate(&mut rules);
        assert_eq!(
            rules[0].confidence, before,
            "one fact duplicated is not corroboration"
        );
        assert_eq!(rules[0].status, Status::Inferred);
    }

    #[test]
    fn documentation_contradicting_code_is_reported() {
        let rules = vec![
            doc_rule("corporate customers require approval"),
            code_rule("corporate customers bypass approval"),
        ];
        let conflicts = detect_conflicts(&rules);
        assert_eq!(conflicts.len(), 1);
        assert!(conflicts[0].documented.contains("require"));
        assert!(conflicts[0].observed.contains("bypass"));
        assert_eq!(conflicts[0].status, Status::Conflicted);
        assert_eq!(conflicts[0].resolution, "UNRESOLVED");
    }

    #[test]
    fn code_disagreeing_with_code_is_a_branch_not_a_conflict() {
        let rules = vec![
            code_rule("corporate customers require approval"),
            RuleCandidate::new(
                "corporate customers bypass approval".into(),
                "approval".into(),
                Polarity::Bypass,
                RuleSource::CodeGuard,
                "app/other.py:9".into(),
                "sym:app/other.py#g".into(),
            ),
        ];
        assert!(detect_conflicts(&rules).is_empty());
    }

    #[test]
    fn claims_about_different_things_are_not_a_conflict() {
        let rules = vec![
            doc_rule("corporate customers require approval"),
            code_rule("timesheet entries bypass approval"),
        ];
        assert!(
            detect_conflicts(&rules).is_empty(),
            "no shared vocabulary means no contradiction"
        );
    }

    #[test]
    fn a_low_confidence_claim_cannot_raise_a_conflict() {
        let mut doc = doc_rule("corporate customers require approval");
        let code = code_rule("corporate customers bypass approval");
        doc.confidence = 0.4;
        assert!(detect_conflicts(&[doc, code]).is_empty());
    }

    #[test]
    fn conflict_detection_is_deterministic() {
        let rules = vec![
            doc_rule("corporate customers require approval"),
            code_rule("corporate customers bypass approval"),
        ];
        assert_eq!(
            serde_json::to_string(&detect_conflicts(&rules)).unwrap(),
            serde_json::to_string(&detect_conflicts(&rules)).unwrap()
        );
    }

    #[test]
    fn staging_links_each_rule_to_where_it_was_read_from() {
        let rules = vec![code_rule("corporate customers bypass approval")];
        let conflicts = detect_conflicts(&rules);
        let batch = stage(&rules, &conflicts);
        assert_eq!(batch.nodes.len(), 1);
        assert_eq!(batch.edges.len(), 1);
        assert_eq!(batch.edges[0].kind, EdgeKind::ImplementsRule);
        assert_eq!(
            batch.evidence.len(),
            1,
            "an inferred rule without evidence is not allowed"
        );
    }
}

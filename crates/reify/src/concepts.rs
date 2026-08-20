//! The concept layer: one identity per business idea, in every language it appears in.
//!
//! Concept ids are opaque (`STRATEGIC_ACCOUNT` is a symbol, not English) and every
//! label carries a language tag including English, because no language is canonical in
//! the codebases Reify targets. Cross-lingual retrieval happens in concept space, which
//! is why it is deterministic and citable rather than an embedding lookup.
//!
//! Three bridges, in precision order (`docs/PLAN.md` §K.2):
//!   1. a declared glossary — human authored, `CONFIRMED`;
//!   2. the product's own translation files — `OBSERVED`;
//!   3. co-occurrence between document headings and code identifiers — `OBSERVED`.

use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use crate::extract::code::split_identifier;
use crate::model::{uid, EdgeKind, NewEdge, NewNode, NodeKind, Status};
use crate::store::Batch;

/// A translation whose English side matches nothing in the code is a UI string, not a
/// domain concept. Grounding every mined concept in at least this many code symbols is
/// what keeps a 12,000-row translation file from becoming 12,000 useless concepts.
const MIN_GROUNDING: usize = 1;

/// Labels shorter than this match too much to be worth a concept.
const MIN_LABEL_WORD_LEN: usize = 4;

/// Cap on symbols linked to a single concept, so a generic term cannot dominate
/// retrieval.
const MAX_LINKS_PER_CONCEPT: usize = 24;

/// Words that carry no domain meaning in any of the supported languages.
const STOPWORDS: &[&str] = &[
    // English
    "the", "and", "for", "with", "from", "this", "that", "are", "was", "were", "has",
    "have", "not", "all", "any", "can", "may", "must", "shall", "will", "should", "into",
    "out", "new", "get", "set", "add", "list", "item", "data", "name", "type", "value",
    // Vietnamese
    "của", "và", "các", "cho", "một", "này", "đó", "là", "có", "không", "được", "khi",
    "với", "trong", "theo", "từ", "đến", "hoặc",
];

/// One business concept and every surface form it appears under.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Concept {
    /// Opaque stable id, e.g. `STRATEGIC_ACCOUNT`.
    pub id: String,
    /// Language code to human label, e.g. `vie` -> `khách hàng chiến lược`.
    pub labels: BTreeMap<String, String>,
    /// Code identifiers this concept is realised as.
    pub code: BTreeSet<String>,
    /// Database objects or literal value bindings, e.g. `CUSTOMER_GROUP=7`.
    pub db: BTreeSet<String>,
    pub status: Status,
    pub confidence: f32,
    /// Which bridge produced it, recorded for provenance and for `reify report`.
    pub bridge: Bridge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bridge {
    /// Declared by a human in `.reify/glossary.toml`.
    Declared,
    /// Mined from the product's own translation files.
    Translation,
    /// Mined from document headings that co-occur with code identifiers.
    #[default]
    CoOccurrence,
}

impl Bridge {
    pub fn as_str(self) -> &'static str {
        match self {
            Bridge::Declared => "declared",
            Bridge::Translation => "translation",
            Bridge::CoOccurrence => "co-occurrence",
        }
    }
}

impl Concept {
    /// One word set per language label, each de-stopworded.
    ///
    /// Per language, never unioned: grounding intersects the words of a label against
    /// code identifiers, and a set mixing `strategic account` with
    /// `khách hàng chiến lược` intersects to nothing by construction. A concept is
    /// grounded when *any one* of its surface forms lands on code.
    pub fn label_word_sets(&self) -> Vec<BTreeSet<String>> {
        let mut sets: Vec<BTreeSet<String>> = self
            .labels
            .values()
            .map(|label| meaningful_words(label))
            .filter(|set| !set.is_empty())
            .collect();
        // The id itself is a surface form: `STRATEGIC_ACCOUNT` grounds even when a
        // concept carries no English label at all.
        let from_id = meaningful_words(&self.id.replace('_', " "));
        if !from_id.is_empty() {
            sets.push(from_id);
        }
        sets
    }

    /// The label shown to a human, preferring English then whatever exists.
    pub fn display(&self) -> &str {
        self.labels
            .get("eng")
            .or_else(|| self.labels.values().next())
            .map(|s| s.as_str())
            .unwrap_or(&self.id)
    }
}

/// Maps a word to the uids of symbols and database objects that contain it.
///
/// This is the grounding index: a concept only exists if its words land here.
#[derive(Debug, Default)]
pub struct TermIndex {
    by_word: HashMap<String, Vec<String>>,
}

impl TermIndex {
    /// Register an identifier (symbol name, table name) under each of its words.
    pub fn add(&mut self, identifier: &str, node_uid: &str) {
        for word in split_identifier(identifier) {
            if word.len() < 3 || STOPWORDS.contains(&word.as_str()) {
                continue;
            }
            let bucket = self.by_word.entry(word).or_default();
            if !bucket.iter().any(|u| u == node_uid) {
                bucket.push(node_uid.to_string());
            }
        }
    }

    /// Uids containing every word in `words`.
    ///
    /// Intersection rather than union: "strategic account" must not match every symbol
    /// with "account" in its name, or the concept layer becomes noise.
    pub fn matching_all(&self, words: &BTreeSet<String>) -> Vec<String> {
        let mut iter = words.iter().filter(|w| w.len() >= 3);
        let Some(first) = iter.next() else {
            return Vec::new();
        };
        let mut current: BTreeSet<&String> = match self.by_word.get(first) {
            Some(v) => v.iter().collect(),
            None => return Vec::new(),
        };
        for word in iter {
            let Some(next) = self.by_word.get(word) else {
                return Vec::new();
            };
            let next: BTreeSet<&String> = next.iter().collect();
            current = current.intersection(&next).copied().collect();
            if current.is_empty() {
                return Vec::new();
            }
        }
        let mut out: Vec<String> = current.into_iter().cloned().collect();
        out.sort();
        out.truncate(MAX_LINKS_PER_CONCEPT);
        out
    }

    /// Like [`matching_all`](Self::matching_all), but words the codebase has never
    /// heard of do not veto the match.
    ///
    /// Document headings pad domain terms with connective nouns — "Strategic Account
    /// *handling*", "Approval *policy*" — and a strict intersection lets one such word
    /// reject an otherwise perfect match. A word absent from the index contributes no
    /// grounding evidence either way, so it is dropped rather than treated as a veto.
    /// At least `min_words` grounded words must survive, which is what keeps this from
    /// collapsing into a single-word match on a generic term.
    pub fn matching_grounded(&self, words: &BTreeSet<String>, min_words: usize) -> Vec<String> {
        let grounded: BTreeSet<String> = words
            .iter()
            .filter(|w| w.len() >= 3 && self.by_word.contains_key(*w))
            .cloned()
            .collect();
        if grounded.len() < min_words {
            return Vec::new();
        }
        self.matching_all(&grounded)
    }

    pub fn is_empty(&self) -> bool {
        self.by_word.is_empty()
    }
}

/// A human-authored glossary, the highest-precision bridge.
#[derive(Debug, Default)]
pub struct Glossary {
    pub concepts: Vec<Concept>,
}

impl Glossary {
    /// Load `.reify/glossary.toml`, or return an empty glossary if it does not exist.
    ///
    /// An absent glossary is the normal case and must never be an error; a malformed
    /// one is an error, because silently ignoring a human's curated knowledge is worse
    /// than stopping.
    pub fn load(path: &Path) -> Result<Glossary> {
        if !path.exists() {
            return Ok(Glossary::default());
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading glossary {}", path.display()))?;
        Glossary::parse(&text)
            .with_context(|| format!("parsing glossary {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Glossary> {
        #[derive(serde::Deserialize)]
        struct Raw {
            #[serde(default)]
            concept: Vec<RawConcept>,
        }
        #[derive(serde::Deserialize)]
        struct RawConcept {
            id: String,
            #[serde(default)]
            labels: BTreeMap<String, String>,
            #[serde(default)]
            code: Vec<String>,
            #[serde(default)]
            db: Vec<String>,
        }

        let raw: Raw = toml::from_str(text)?;
        let concepts = raw
            .concept
            .into_iter()
            .map(|c| Concept {
                id: normalize_id(&c.id),
                labels: c.labels,
                code: c.code.into_iter().collect(),
                db: c.db.into_iter().collect(),
                status: Status::Confirmed,
                confidence: 1.0,
                bridge: Bridge::Declared,
            })
            .collect();
        Ok(Glossary { concepts })
    }

    /// Render concepts back to glossary syntax, so `reify concepts --suggest` produces
    /// something a human can paste in and a repository can version-control.
    pub fn render(concepts: &[Concept]) -> String {
        let mut out = String::from(
            "# Reify glossary. Declared concepts are trusted above every mined one.\n\n",
        );
        for c in concepts {
            out.push_str("[[concept]]\n");
            out.push_str(&format!("id = \"{}\"\n", c.id));
            if !c.labels.is_empty() {
                let inner = c
                    .labels
                    .iter()
                    .map(|(k, v)| format!("{k} = \"{}\"", v.replace('"', "'")))
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!("labels = {{ {inner} }}\n"));
            }
            if !c.code.is_empty() {
                out.push_str(&format!("code = {:?}\n", c.code.iter().collect::<Vec<_>>()));
            }
            if !c.db.is_empty() {
                out.push_str(&format!("db = {:?}\n", c.db.iter().collect::<Vec<_>>()));
            }
            out.push('\n');
        }
        out
    }
}

/// Mine concepts from a translation file.
///
/// This is the bridge that makes multilingual work without a model: any localised
/// product already ships a professionally translated dictionary of its own domain
/// vocabulary, and the English side of each row is usually derived from an identifier.
///
/// `lang` is the target language code; rows are `(source, translation)` pairs.
pub fn from_translations(
    lang: &str,
    rows: &[(String, String)],
    grounding: &TermIndex,
) -> Vec<Concept> {
    let mut by_id: BTreeMap<String, Concept> = BTreeMap::new();
    for (source, translation) in rows {
        let words = meaningful_words(source);
        if words.is_empty() || !words.iter().any(|w| w.len() >= MIN_LABEL_WORD_LEN) {
            continue;
        }
        let grounded = grounding.matching_all(&words);
        if grounded.len() < MIN_GROUNDING {
            continue;
        }
        let id = normalize_id(source);
        let entry = by_id.entry(id.clone()).or_insert_with(|| Concept {
            id,
            status: Status::Observed,
            confidence: 0.8,
            bridge: Bridge::Translation,
            ..Concept::default()
        });
        entry.labels.insert("eng".into(), source.clone());
        entry.labels.insert(lang.to_string(), translation.clone());
    }
    by_id.into_values().collect()
}

/// Mine concepts from document headings that are also realised in code.
///
/// A heading naming something the code also names is, by construction, shared
/// vocabulary. A heading naming nothing in the code is prose.
pub fn from_headings(headings: &[String], grounding: &TermIndex) -> Vec<Concept> {
    let mut by_id: BTreeMap<String, Concept> = BTreeMap::new();
    for heading in headings {
        let words = meaningful_words(heading);
        if words.len() < 2 || !words.iter().any(|w| w.len() >= MIN_LABEL_WORD_LEN) {
            continue;
        }
        // Headings are prose, so connective words must not veto an otherwise sound
        // match; two grounded words are still required to keep precision up.
        if grounding.matching_grounded(&words, 2).len() < MIN_GROUNDING {
            continue;
        }
        let id = normalize_id(heading);
        let entry = by_id.entry(id.clone()).or_insert_with(|| Concept {
            id,
            status: Status::Observed,
            confidence: 0.65,
            bridge: Bridge::CoOccurrence,
            ..Concept::default()
        });
        entry
            .labels
            .entry("eng".into())
            .or_insert_with(|| heading.clone());
    }
    by_id.into_values().collect()
}

/// Merge concepts from several bridges, strongest bridge winning on conflict.
///
/// A declared concept always absorbs a mined one with the same id rather than being
/// overwritten by it — human curation is the highest-precision input in the system.
pub fn merge(sources: Vec<Vec<Concept>>) -> Vec<Concept> {
    let mut merged: BTreeMap<String, Concept> = BTreeMap::new();
    for batch in sources {
        for concept in batch {
            match merged.get_mut(&concept.id) {
                None => {
                    merged.insert(concept.id.clone(), concept);
                }
                Some(existing) => {
                    for (lang, label) in concept.labels {
                        existing.labels.entry(lang).or_insert(label);
                    }
                    existing.code.extend(concept.code);
                    existing.db.extend(concept.db);
                    if concept.status > existing.status {
                        existing.status = concept.status;
                        existing.bridge = concept.bridge;
                    }
                    existing.confidence = existing.confidence.max(concept.confidence);
                }
            }
        }
    }
    merged.into_values().collect()
}

/// Stage concepts as nodes and link them to the code and data they name.
pub fn stage(concepts: &[Concept], grounding: &TermIndex) -> Batch {
    let mut batch = Batch::default();
    for concept in concepts {
        let concept_uid = uid::concept(&concept.id);
        let word_sets = concept.label_word_sets();

        let mut search = String::new();
        for label in concept.labels.values() {
            search.push_str(label);
            search.push(' ');
        }
        for code in &concept.code {
            search.push_str(code);
            search.push(' ');
            search.push_str(&split_identifier(code).join(" "));
            search.push(' ');
        }
        search.push_str(&concept.id.replace('_', " ").to_lowercase());

        batch.node(
            NewNode::new(&concept_uid, NodeKind::Concept, concept.display())
                .status(concept.status, concept.confidence)
                .search(search)
                .data(serde_json::json!({
                    "concept_id": concept.id,
                    "labels": concept.labels,
                    "code": concept.code,
                    "db": concept.db,
                    "bridge": concept.bridge.as_str(),
                    "summary": format!("concept {}", concept.id),
                })),
        );

        // Link by shared vocabulary — each surface form independently — plus any
        // explicitly declared code identifier.
        let mut targets: Vec<String> = Vec::new();
        for words in &word_sets {
            targets.extend(grounding.matching_all(words));
        }
        for identifier in &concept.code {
            let exact: BTreeSet<String> = split_identifier(identifier).into_iter().collect();
            targets.extend(grounding.matching_all(&exact));
        }
        targets.sort();
        targets.dedup();
        targets.truncate(MAX_LINKS_PER_CONCEPT);

        for target in targets {
            // A declared link is as strong as the declaration; a mined one is weaker.
            let confidence = match concept.bridge {
                Bridge::Declared => 0.95,
                Bridge::Translation => 0.8,
                Bridge::CoOccurrence => 0.6,
            };
            batch.edge(NewEdge::new(
                concept_uid.clone(),
                target,
                EdgeKind::MapsTo,
                concept.status,
                confidence,
            ));
        }
    }
    batch
}

/// Parse a translation file, returning `(source, translation)` rows.
///
/// Handles the two shapes seen in the wild: two columns `source,translation`, and
/// three columns `context,source,translation` as Frappe emits.
pub fn parse_translation_csv(text: &str) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes());
    for record in reader.records().flatten() {
        let cells: Vec<&str> = record.iter().map(str::trim).collect();
        let (source, translation) = match cells.len() {
            2 => (cells[0], cells[1]),
            n if n >= 3 => (cells[1], cells[2]),
            _ => continue,
        };
        if source.is_empty() || translation.is_empty() || source == translation {
            continue;
        }
        rows.push((source.to_string(), translation.to_string()));
    }
    rows
}

/// Infer the target language of a translation file from its path.
pub fn translation_language(path: &str) -> Option<String> {
    let stem = path
        .rsplit('/')
        .next()?
        .rsplit_once('.')
        .map(|(stem, _)| stem)?;
    let code = stem.split(['-', '_']).next()?.to_ascii_lowercase();
    // ISO-639-1 codes seen on translation files, mapped to the 3-letter form used by
    // the language detector so both bridges speak one vocabulary.
    let three = match code.as_str() {
        "vi" => "vie",
        "en" => "eng",
        "ja" => "jpn",
        "ko" => "kor",
        "zh" => "cmn",
        "fr" => "fra",
        "de" => "deu",
        "es" => "spa",
        "pt" => "por",
        "th" => "tha",
        "id" => "ind",
        _ => return None,
    };
    Some(three.to_string())
}

/// Lowercase, de-stopworded, de-punctuated words of a label or identifier.
///
/// Every token goes through [`split_identifier`], so `customer_group`,
/// `customerGroup` and `Customer Group` all reduce to the same two words. Treating
/// snake_case as a single opaque token — which is what a naive split does, since `_`
/// is a word character — quietly stops business vocabulary from ever matching code.
pub fn meaningful_words(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .filter(|w| !w.is_empty())
        .flat_map(split_identifier)
        .filter(|w| w.chars().count() >= 3 && !STOPWORDS.contains(&w.as_str()))
        .collect()
}

/// Turn a label into a stable opaque concept id.
pub fn normalize_id(label: &str) -> String {
    let mut out = String::new();
    let mut last_underscore = true;
    for ch in label.chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_uppercase());
            last_underscore = false;
        } else if !last_underscore {
            out.push('_');
            last_underscore = true;
        }
    }
    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "CONCEPT".into()
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grounding() -> TermIndex {
        let mut idx = TermIndex::default();
        idx.add("StrategicAccount", "sym:crm.py#StrategicAccount");
        idx.add("bypassLevelTwoApproval", "sym:order.py#bypassLevelTwoApproval");
        idx.add("customer_group", "db:customer_group");
        idx.add("SalesOrder", "sym:order.py#SalesOrder");
        idx
    }

    #[test]
    fn identifiers_are_indexed_under_each_of_their_words() {
        let idx = grounding();
        let mut words = BTreeSet::new();
        words.insert("strategic".to_string());
        assert_eq!(idx.matching_all(&words), vec!["sym:crm.py#StrategicAccount"]);
    }

    #[test]
    fn multi_word_lookup_intersects_rather_than_unions() {
        let idx = grounding();
        let words: BTreeSet<String> = ["strategic", "account"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(idx.matching_all(&words), vec!["sym:crm.py#StrategicAccount"]);

        // "approval account" shares no single symbol, so it must match nothing.
        let words: BTreeSet<String> = ["approval", "account"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(idx.matching_all(&words).is_empty());
    }

    #[test]
    fn an_unknown_word_yields_no_match_rather_than_a_partial_one() {
        let idx = grounding();
        let words: BTreeSet<String> = ["strategic", "unicorn"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(idx.matching_all(&words).is_empty());
    }

    #[test]
    fn a_vietnamese_translation_row_becomes_a_bilingual_concept() {
        let rows = vec![(
            "Strategic Account".to_string(),
            "khách hàng chiến lược".to_string(),
        )];
        let concepts = from_translations("vie", &rows, &grounding());
        assert_eq!(concepts.len(), 1);
        let c = &concepts[0];
        assert_eq!(c.id, "STRATEGIC_ACCOUNT");
        assert_eq!(c.labels["eng"], "Strategic Account");
        assert_eq!(c.labels["vie"], "khách hàng chiến lược");
        assert_eq!(c.bridge, Bridge::Translation);
        assert_eq!(c.status, Status::Observed);
    }

    #[test]
    fn ungrounded_ui_strings_are_not_promoted_to_concepts() {
        // This is what keeps a 12k-row translation file from producing 12k concepts.
        let rows = vec![
            ("Please wait while loading".to_string(), "Vui lòng đợi".to_string()),
            ("Strategic Account".to_string(), "khách hàng chiến lược".to_string()),
        ];
        let concepts = from_translations("vie", &rows, &grounding());
        assert_eq!(concepts.len(), 1, "only the grounded row survives");
        assert_eq!(concepts[0].id, "STRATEGIC_ACCOUNT");
    }

    #[test]
    fn relaxed_matching_ignores_words_the_codebase_never_uses() {
        let idx = grounding();
        let words: BTreeSet<String> = ["strategic", "account", "handling"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(idx.matching_all(&words).is_empty(), "strict matching vetoes");
        assert_eq!(
            idx.matching_grounded(&words, 2),
            vec!["sym:crm.py#StrategicAccount"]
        );
    }

    #[test]
    fn relaxed_matching_still_requires_enough_grounded_words() {
        let idx = grounding();
        let words: BTreeSet<String> = ["account", "unicorn", "rainbow"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(idx.matching_grounded(&words, 2).is_empty());
    }

    #[test]
    fn headings_that_name_nothing_in_the_code_are_ignored() {
        let headings = vec![
            "Strategic Account handling".to_string(),
            "Introduction and background".to_string(),
        ];
        let concepts = from_headings(&headings, &grounding());
        let ids: Vec<&str> = concepts.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["STRATEGIC_ACCOUNT_HANDLING"]);
    }

    #[test]
    fn a_declared_concept_absorbs_a_mined_one_rather_than_being_overwritten() {
        let declared = vec![Concept {
            id: "STRATEGIC_ACCOUNT".into(),
            labels: BTreeMap::from([("eng".into(), "strategic account".into())]),
            code: BTreeSet::from(["StrategicAccount".to_string()]),
            db: BTreeSet::from(["CUSTOMER_GROUP=7".to_string()]),
            status: Status::Confirmed,
            confidence: 1.0,
            bridge: Bridge::Declared,
        }];
        let mined = vec![Concept {
            id: "STRATEGIC_ACCOUNT".into(),
            labels: BTreeMap::from([("vie".into(), "khách hàng chiến lược".into())]),
            status: Status::Observed,
            confidence: 0.8,
            bridge: Bridge::Translation,
            ..Concept::default()
        }];
        let merged = merge(vec![declared, mined]);
        assert_eq!(merged.len(), 1);
        let c = &merged[0];
        assert_eq!(c.status, Status::Confirmed, "human curation must win");
        assert_eq!(c.bridge, Bridge::Declared);
        assert_eq!(c.labels["vie"], "khách hàng chiến lược", "and still gain the label");
        assert!(c.db.contains("CUSTOMER_GROUP=7"));
    }

    #[test]
    fn glossary_parses_and_round_trips_through_rendering() {
        let toml_text = r#"
[[concept]]
id = "strategic account"
labels = { en = "strategic account", vi = "khách hàng chiến lược" }
code = ["StrategicAccount"]
db = ["CUSTOMER_GROUP=7"]
"#;
        let g = Glossary::parse(toml_text).unwrap();
        assert_eq!(g.concepts.len(), 1);
        assert_eq!(g.concepts[0].id, "STRATEGIC_ACCOUNT", "ids are normalised");
        assert_eq!(g.concepts[0].status, Status::Confirmed);

        let rendered = Glossary::render(&g.concepts);
        let reparsed = Glossary::parse(&rendered).unwrap();
        assert_eq!(reparsed.concepts[0].id, g.concepts[0].id);
        assert_eq!(reparsed.concepts[0].labels, g.concepts[0].labels);
    }

    #[test]
    fn a_missing_glossary_is_normal_and_a_broken_one_is_not() {
        let missing = Path::new("/nonexistent/glossary.toml");
        assert!(Glossary::load(missing).unwrap().concepts.is_empty());
        assert!(Glossary::parse("[[concept]]\nnot_an_id = 1\n").is_err());
    }

    #[test]
    fn label_word_sets_stay_separate_per_language() {
        let c = Concept {
            id: "STRATEGIC_ACCOUNT".into(),
            labels: BTreeMap::from([
                ("eng".into(), "strategic account".into()),
                ("vie".into(), "khách hàng chiến lược".into()),
            ]),
            ..Concept::default()
        };
        let sets = c.label_word_sets();
        assert!(sets.iter().any(|s| s.contains("strategic") && !s.contains("khách")));
        assert!(sets.iter().any(|s| s.contains("khách") && !s.contains("strategic")));
    }

    #[test]
    fn a_bilingual_concept_still_grounds_through_its_english_form() {
        // Unioning label words across languages would intersect to nothing, silently
        // disconnecting every translated concept from the code it names.
        let concepts = vec![Concept {
            id: "STRATEGIC_ACCOUNT".into(),
            labels: BTreeMap::from([
                ("eng".into(), "Strategic Account".into()),
                ("vie".into(), "khách hàng chiến lược".into()),
            ]),
            status: Status::Observed,
            confidence: 0.8,
            bridge: Bridge::Translation,
            ..Concept::default()
        }];
        let batch = stage(&concepts, &grounding());
        assert_eq!(batch.edges.len(), 1, "the English surface form must ground it");
        assert_eq!(batch.edges[0].dst, "sym:crm.py#StrategicAccount");
    }

    #[test]
    fn a_concept_with_no_labels_still_grounds_through_its_id() {
        let concepts = vec![Concept {
            id: "STRATEGIC_ACCOUNT".into(),
            status: Status::Observed,
            confidence: 0.8,
            ..Concept::default()
        }];
        let batch = stage(&concepts, &grounding());
        assert_eq!(batch.edges.len(), 1);
    }

    #[test]
    fn staging_links_a_concept_to_the_symbols_that_realise_it() {
        let concepts = vec![Concept {
            id: "STRATEGIC_ACCOUNT".into(),
            labels: BTreeMap::from([("eng".into(), "strategic account".into())]),
            status: Status::Confirmed,
            confidence: 1.0,
            bridge: Bridge::Declared,
            ..Concept::default()
        }];
        let batch = stage(&concepts, &grounding());
        assert_eq!(batch.nodes.len(), 1);
        assert_eq!(batch.edges.len(), 1);
        assert_eq!(batch.edges[0].src, "concept:STRATEGIC_ACCOUNT");
        assert_eq!(batch.edges[0].dst, "sym:crm.py#StrategicAccount");
        assert_eq!(batch.edges[0].kind, EdgeKind::MapsTo);
    }

    #[test]
    fn translation_csv_handles_both_two_and_three_column_shapes() {
        let two = parse_translation_csv("Strategic Account,khách hàng chiến lược\n");
        assert_eq!(two, vec![("Strategic Account".into(), "khách hàng chiến lược".into())]);

        let three = parse_translation_csv("Selling,Strategic Account,khách hàng chiến lược\n");
        assert_eq!(three, vec![("Strategic Account".into(), "khách hàng chiến lược".into())]);
    }

    #[test]
    fn untranslated_rows_are_dropped() {
        let rows = parse_translation_csv("Order,Order\n,\nSubmit,Gửi\n");
        assert_eq!(rows, vec![("Submit".into(), "Gửi".into())]);
    }

    #[test]
    fn translation_language_is_read_from_the_filename() {
        assert_eq!(translation_language("erpnext/translations/vi.csv").as_deref(), Some("vie"));
        assert_eq!(translation_language("locale/ja_JP.csv").as_deref(), Some("jpn"));
        assert_eq!(translation_language("data/customers.csv"), None);
    }

    #[test]
    fn concept_ids_are_stable_and_opaque() {
        assert_eq!(normalize_id("Strategic Account"), "STRATEGIC_ACCOUNT");
        assert_eq!(normalize_id("  L2 approval! "), "L2_APPROVAL");
        assert_eq!(normalize_id("???"), "CONCEPT");
    }

    #[test]
    fn stopwords_never_become_concept_words() {
        let words = meaningful_words("the list of all strategic accounts");
        assert!(words.contains("strategic"));
        assert!(words.contains("accounts"));
        assert!(!words.contains("the"));
        assert!(!words.contains("all"));
        assert!(!words.contains("list"));
    }

    #[test]
    fn snake_case_and_camel_case_reduce_to_the_same_words() {
        let expected: BTreeSet<String> =
            ["customer", "group"].iter().map(|s| s.to_string()).collect();
        assert_eq!(meaningful_words("customer_group"), expected);
        assert_eq!(meaningful_words("customerGroup"), expected);
        assert_eq!(meaningful_words("Customer Group"), expected);
        assert_eq!(meaningful_words("CUSTOMER_GROUP"), expected);
    }

    #[test]
    fn vietnamese_words_survive_identifier_splitting() {
        let words = meaningful_words("khách hàng chiến lược");
        assert!(words.contains("khách"));
        assert!(words.contains("chiến"));
    }

    #[test]
    fn vietnamese_stopwords_are_stripped_too() {
        let words = meaningful_words("khách hàng của công ty");
        assert!(words.contains("khách"));
        assert!(!words.contains("của"), "got {words:?}");
    }
}

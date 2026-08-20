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
/// retrieval. The strongest bridge may link the most.
const MAX_LINKS_PER_CONCEPT: usize = 24;

/// How many symbols a concept from each bridge may claim.
///
/// A concept a human declared, or that a framework declares in metadata, has earned
/// the right to name a lot of code. One inferred from the fact that two words appear
/// near each other has not: linking it as widely makes the graph denser without making
/// it more informative, and thins the relevance spread for everything else.
fn max_links(bridge: Bridge) -> usize {
    match bridge {
        Bridge::Declared => MAX_LINKS_PER_CONCEPT,
        Bridge::Translation => 16,
        Bridge::CoOccurrence => 12,
        Bridge::CodeVocabulary => 8,
    }
}

/// Words that carry no domain meaning in any of the supported languages.
const STOPWORDS: &[&str] = &[
    // English
    "the",
    "and",
    "for",
    "with",
    "from",
    "this",
    "that",
    "are",
    "was",
    "were",
    "has",
    "have",
    "not",
    "all",
    "any",
    "can",
    "may",
    "must",
    "shall",
    "will",
    "should",
    "into",
    "out",
    "new",
    "get",
    "set",
    "add",
    "list",
    "item",
    "data",
    "name",
    "type",
    "value",
    // Vietnamese
    "của",
    "và",
    "các",
    "cho",
    "một",
    "này",
    "đó",
    "là",
    "có",
    "không",
    "được",
    "khi",
    "với",
    "trong",
    "theo",
    "từ",
    "đến",
    "hoặc",
    "những",
    "đã",
    "sẽ",
    "phải",
    "cần",
    // German
    "der",
    "die",
    "das",
    "und",
    "oder",
    "für",
    "mit",
    "von",
    "den",
    "dem",
    "des",
    "ein",
    "eine",
    "ist",
    "sind",
    "wird",
    "werden",
    "nicht",
    "auch",
    "auf",
    "aus",
    "bei",
    // Thai
    "และ",
    "หรือ",
    "ของ",
    "ที่",
    "ใน",
    "จะ",
    "ได้",
    "เป็น",
    "การ",
    "ให้",
    "กับ",
    "ไม่",
    // Korean
    "그리고",
    "또는",
    "에서",
    "으로",
    "하는",
    "합니다",
    "있는",
    "없는",
    "이다",
    "위한",
    // Japanese
    "および",
    "または",
    "する",
    "した",
    "こと",
    "ため",
    "これ",
    "それ",
    "です",
    "ます",
    // Chinese
    "的",
    "和",
    "或",
    "在",
    "是",
    "为",
    "与",
    "这",
    "那",
    "有",
    "不",
    // Spanish / Portuguese / French / Italian
    "que",
    "los",
    "las",
    "del",
    "por",
    "con",
    "para",
    "una",
    "uno",
    "como",
    "más",
    "dos",
    "das",
    "não",
    "são",
    "ser",
    "est",
    "sont",
    "pour",
    "dans",
    "avec",
    "sur",
    "les",
    "des",
    "une",
    "qui",
    "sono",
    "per",
    "con",
    "nel",
    "della",
    "delle",
    // Indonesian / Malay
    "yang",
    "dan",
    "atau",
    "untuk",
    "dari",
    "pada",
    "dengan",
    "adalah",
    "akan",
    "tidak",
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
    /// Mined from the code's own identifiers, with nothing declared anywhere.
    ///
    /// The universal bridge: every repository has identifiers, so this one works
    /// regardless of framework, language or documentation. It is the weakest of the
    /// four and the only one that is always available.
    CodeVocabulary,
}

impl Bridge {
    pub fn as_str(self) -> &'static str {
        match self {
            Bridge::Declared => "declared",
            Bridge::Translation => "translation",
            Bridge::CoOccurrence => "co-occurrence",
            Bridge::CodeVocabulary => "code-vocabulary",
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
        Glossary::parse(&text).with_context(|| format!("parsing glossary {}", path.display()))
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

/// A phrase must name this many distinct symbols before it counts as vocabulary.
///
/// One symbol called `customer_group` is a name. Six symbols across four files sharing
/// the phrase is a concept the codebase keeps returning to.
const MIN_PHRASE_SYMBOLS: usize = 3;

/// A word appearing in more than this share of all symbols is structural, not domain.
///
/// `get`, `set`, `handle`, `manager`, `service`, `util` — every codebase has its own,
/// and hard-coding a list would be wrong for the next one. Measuring which words are
/// ubiquitous *in this repository* finds them without guessing.
const UBIQUITOUS_WORD_SHARE: f32 = 0.02;

/// Ubiquity cannot be established below this many occurrences, whatever the share.
///
/// Two percent of a small repository is a handful of symbols, and a genuine domain
/// term easily appears that often — so without a floor the filter deletes exactly the
/// vocabulary it exists to find. The share only starts to bind on repositories large
/// enough for it to mean something.
const UBIQUITOUS_FLOOR: usize = 25;

/// The most vocabulary concepts to mine. Enough to cover a domain, few enough that the
/// concept layer stays a vocabulary rather than a second symbol table.
const MAX_VOCABULARY_CONCEPTS: usize = 400;

/// Mine concepts from the code's own identifiers.
///
/// This is the bridge that makes Reify work on a repository that declares nothing: no
/// glossary, no translation files, no entity metadata, no documentation. It rests on
/// one observation — a multi-word phrase that recurs across many identifiers *is* the
/// domain vocabulary, because that is what naming a domain looks like.
///
/// It is a *fallback*, not a competitor. `already_named` holds the uids some stronger
/// bridge already reaches, and those are excluded: where a human, a framework or a
/// translation file has spoken, guessing from identifiers adds nothing and only thins
/// the relevance spread across a denser graph.
///
/// `symbols` is `(name, uid)` for every symbol and database object in the repository.
pub fn from_code_vocabulary(
    symbols: &[(String, String)],
    already_named: &BTreeSet<String>,
) -> Vec<Concept> {
    let symbols: Vec<(String, String)> = symbols
        .iter()
        .filter(|(_, uid)| !already_named.contains(uid))
        .cloned()
        .collect();
    if symbols.len() < MIN_PHRASE_SYMBOLS {
        return Vec::new();
    }
    let symbols = &symbols[..];

    // Which words are structural in *this* repository rather than domain vocabulary.
    let mut word_frequency: BTreeMap<String, usize> = BTreeMap::new();
    let split: Vec<(Vec<String>, &String)> = symbols
        .iter()
        .map(|(name, uid)| (split_identifier(name), uid))
        .collect();
    for (words, _) in &split {
        for word in words.iter().collect::<BTreeSet<_>>() {
            *word_frequency.entry(word.clone()).or_default() += 1;
        }
    }
    let ubiquitous_at =
        (((symbols.len() as f32) * UBIQUITOUS_WORD_SHARE) as usize).max(UBIQUITOUS_FLOOR);

    // Adjacent word pairs are the unit: `customer_group`, `approval_policy`. Longer
    // phrases fragment, and single words are rarely a concept on their own.
    let mut phrases: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    for (words, uid) in &split {
        let meaningful: Vec<&String> = words
            .iter()
            .filter(|w| w.len() >= 3 && !STOPWORDS.contains(&w.as_str()))
            .collect();
        for pair in meaningful.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            // A pair of two ubiquitous words is boilerplate; one is acceptable, since
            // `service` in `approval_service` still points at the domain.
            let generic = |w: &String| word_frequency.get(w).copied().unwrap_or(0) > ubiquitous_at;
            if generic(a) && generic(b) {
                continue;
            }
            phrases
                .entry((a.clone(), b.clone()))
                .or_default()
                .insert((*uid).clone());
        }
    }

    let mut ranked: Vec<((String, String), BTreeSet<String>)> = phrases
        .into_iter()
        .filter(|(_, uids)| uids.len() >= MIN_PHRASE_SYMBOLS)
        .collect();
    // Most-used first, then alphabetically so truncation is deterministic.
    ranked.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
    ranked.truncate(MAX_VOCABULARY_CONCEPTS);

    ranked
        .into_iter()
        .map(|((a, b), uids)| {
            let label = format!("{a} {b}");
            Concept {
                id: normalize_id(&label),
                labels: BTreeMap::from([("eng".to_string(), label)]),
                code: uids
                    .iter()
                    .filter_map(|u| u.rsplit(['#', ':']).next().map(str::to_string))
                    .take(6)
                    .collect(),
                db: BTreeSet::new(),
                // Read off the code and nothing else: real, but weaker evidence than a
                // human or a framework declaring the same thing.
                status: Status::Observed,
                confidence: 0.55,
                bridge: Bridge::CodeVocabulary,
            }
        })
        .collect()
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
        targets.truncate(max_links(concept.bridge));

        for target in targets {
            // A declared link is as strong as the declaration; a mined one is weaker.
            let confidence = match concept.bridge {
                Bridge::Declared => 0.95,
                Bridge::Translation => 0.8,
                Bridge::CoOccurrence => 0.6,
                Bridge::CodeVocabulary => 0.5,
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

/// Parse a Java/Spring `.properties` message bundle into `(key, value)` pairs.
///
/// These are the translation bridge for JVM stacks, and they differ from a CSV in one
/// important way: the source and the translation live in *different files*, joined by
/// key. So this returns keys, and the join happens once every bundle has been read.
pub fn parse_properties(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut pending = String::new();
    for raw in text.lines() {
        let line = if pending.is_empty() {
            raw.trim_start()
        } else {
            raw.trim()
        };
        if pending.is_empty() && (line.starts_with('#') || line.starts_with('!') || line.is_empty())
        {
            continue;
        }
        // A trailing backslash continues the value onto the next line.
        if let Some(head) = line.strip_suffix('\\') {
            pending.push_str(head);
            continue;
        }
        let full = if pending.is_empty() {
            line.to_string()
        } else {
            std::mem::take(&mut pending) + line
        };
        let Some((key, value)) = full.split_once(['=', ':']) else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if key.is_empty() || value.is_empty() {
            continue;
        }
        out.push((key.to_string(), value.to_string()));
    }
    out
}

/// The locale a message bundle is written in, or `None` when it is the base bundle.
///
/// `messages.properties` is the base — conventionally English — and
/// `messages_fr.properties` is its French translation. The locale is the *last*
/// underscore-separated segment, which is the opposite of the CSV convention where the
/// whole stem is the language.
pub fn bundle_locale(path: &str) -> Option<String> {
    let stem = path.rsplit('/').next()?.strip_suffix(".properties")?;
    // `messages_fr` and `messages_pt_BR`: drop the bundle's base name, and the first
    // segment that remains is the language. Taking the *last* segment instead reads
    // `pt_BR` as the region and finds no language at all.
    let mut segments = stem.split('_');
    let _base = segments.next()?;
    let code = segments.next()?.split('-').next()?.to_ascii_lowercase();
    three_letter_code(&code)
}

/// Infer the target language of a translation file from its path.
pub fn translation_language(path: &str) -> Option<String> {
    let stem = path
        .rsplit('/')
        .next()?
        .rsplit_once('.')
        .map(|(stem, _)| stem)?;
    let code = stem.split(['-', '_']).next()?.to_ascii_lowercase();
    three_letter_code(&code)
}

/// ISO-639-1 to the three-letter form the language detector uses, so every bridge
/// speaks one vocabulary.
fn three_letter_code(code: &str) -> Option<String> {
    let three = match code {
        // The languages business documentation actually arrives in. Kept as an
        // explicit table rather than a crate: the mapping is small, stable, and the
        // failure mode of a wrong guess is a concept grounded on the wrong language.
        "vi" => "vie",
        "en" => "eng",
        "th" => "tha",
        "ko" => "kor",
        "ja" => "jpn",
        "zh" | "cmn" => "cmn",
        "de" => "deu",
        "fr" => "fra",
        "es" => "spa",
        "pt" => "por",
        "it" => "ita",
        "nl" => "nld",
        "ru" => "rus",
        "pl" => "pol",
        "tr" => "tur",
        "sv" => "swe",
        "da" => "dan",
        "nb" | "no" => "nob",
        "fi" => "fin",
        "cs" => "ces",
        "sk" => "slk",
        "hu" => "hun",
        "ro" => "ron",
        "bg" => "bul",
        "uk" => "ukr",
        "el" => "ell",
        "he" => "heb",
        "ar" => "arb",
        "fa" => "pes",
        "hi" => "hin",
        "bn" => "ben",
        "ta" => "tam",
        "te" => "tel",
        "mr" => "mar",
        "gu" => "guj",
        "pa" => "pan",
        "ur" => "urd",
        "id" => "ind",
        "ms" => "zsm",
        "tl" => "tgl",
        "km" => "khm",
        "lo" => "lao",
        "my" => "mya",
        "ne" => "nep",
        "si" => "sin",
        "sw" => "swh",
        "af" => "afr",
        "ca" => "cat",
        "hr" => "hrv",
        "sr" => "srp",
        "sl" => "slv",
        "lt" => "lit",
        "lv" => "lav",
        "et" => "est",
        "az" => "aze",
        "ka" => "kat",
        "hy" => "hye",
        "kk" => "kaz",
        "uz" => "uzb",
        "mn" => "mon",
        "am" => "amh",
        "yo" => "yor",
        "zu" => "zul",
        "eo" => "epo",
        "jv" => "jav",
        "mk" => "mkd",
        "be" => "bel",
        _ => return None,
    };
    Some(three.to_string())
}

/// Join base-locale and translated message bundles into `(source, translation)` rows.
///
/// `bundles` is `(locale, key, value)`, where `locale` is `None` for the base bundle.
/// A key present in a translation but absent from the base yields nothing: without the
/// source string there is no pair, and inventing one would ground a concept on a term
/// the codebase never uses.
pub fn join_message_bundles(
    bundles: &[(Option<String>, String, String)],
) -> BTreeMap<String, Vec<(String, String)>> {
    let base: BTreeMap<&str, &str> = bundles
        .iter()
        .filter(|(locale, _, _)| locale.is_none())
        .map(|(_, key, value)| (key.as_str(), value.as_str()))
        .collect();

    let mut by_language: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for (locale, key, value) in bundles {
        let Some(locale) = locale else { continue };
        let Some(source) = base.get(key.as_str()) else {
            continue;
        };
        if *source == value {
            continue; // untranslated
        }
        by_language
            .entry(locale.clone())
            .or_default()
            .push((source.to_string(), value.clone()));
    }
    by_language
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

/// Reduce a word to a form that compares across simple English inflection.
///
/// Deliberately not a stemmer. It folds the one inflection that matters for matching
/// business vocabulary against code — the plural `s` — and nothing else. Treating
/// "customers" and "customer" as unrelated words is a bug; aggressively stemming
/// "billing" to "bill" would be a different and worse one.
pub fn stem(word: &str) -> &str {
    if word.len() > 3 && word.ends_with('s') && !word.ends_with("ss") {
        &word[..word.len() - 1]
    } else {
        word
    }
}

/// Do two words match once simple inflection is folded away?
pub fn same_word(a: &str, b: &str) -> bool {
    a == b || stem(a) == stem(b)
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
        idx.add(
            "bypassLevelTwoApproval",
            "sym:order.py#bypassLevelTwoApproval",
        );
        idx.add("customer_group", "db:customer_group");
        idx.add("SalesOrder", "sym:order.py#SalesOrder");
        idx
    }

    #[test]
    fn identifiers_are_indexed_under_each_of_their_words() {
        let idx = grounding();
        let mut words = BTreeSet::new();
        words.insert("strategic".to_string());
        assert_eq!(
            idx.matching_all(&words),
            vec!["sym:crm.py#StrategicAccount"]
        );
    }

    #[test]
    fn multi_word_lookup_intersects_rather_than_unions() {
        let idx = grounding();
        let words: BTreeSet<String> = ["strategic", "account"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            idx.matching_all(&words),
            vec!["sym:crm.py#StrategicAccount"]
        );

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
            (
                "Please wait while loading".to_string(),
                "Vui lòng đợi".to_string(),
            ),
            (
                "Strategic Account".to_string(),
                "khách hàng chiến lược".to_string(),
            ),
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
        assert!(
            idx.matching_all(&words).is_empty(),
            "strict matching vetoes"
        );
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

    /// Nothing covered yet: the repository declares no vocabulary at all.
    fn nothing_named() -> BTreeSet<String> {
        BTreeSet::new()
    }

    #[test]
    fn the_universal_bridge_skips_symbols_a_stronger_bridge_already_reaches() {
        // It fills gaps. Where a glossary, framework or translation file has already
        // named something, guessing from identifiers adds noise and no knowledge.
        let symbols: Vec<(String, String)> = (0..6)
            .map(|i| {
                (
                    format!("patient_encounter_{i}"),
                    format!("sym:p{i}.java#p{i}"),
                )
            })
            .collect();
        assert_eq!(from_code_vocabulary(&symbols, &nothing_named()).len(), 1);

        let covered: BTreeSet<String> = symbols.iter().map(|(_, uid)| uid.clone()).collect();
        assert!(
            from_code_vocabulary(&symbols, &covered).is_empty(),
            "already-named symbols must not be mined again"
        );
    }

    #[test]
    fn vocabulary_concepts_are_mined_from_identifiers_alone() {
        // The universal case: no glossary, no translations, no metadata, no docs.
        let symbols: Vec<(String, String)> = [
            (
                "customer_group_for_order",
                "sym:a.py#customer_group_for_order",
            ),
            ("CustomerGroupPolicy", "sym:b.py#CustomerGroupPolicy"),
            ("resolveCustomerGroup", "sym:c.ts#resolveCustomerGroup"),
            ("unrelated_helper", "sym:d.py#unrelated_helper"),
        ]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();

        let concepts = from_code_vocabulary(&symbols, &nothing_named());
        let ids: Vec<&str> = concepts.iter().map(|c| c.id.as_str()).collect();
        assert!(ids.contains(&"CUSTOMER_GROUP"), "got {ids:?}");
        let found = concepts.iter().find(|c| c.id == "CUSTOMER_GROUP").unwrap();
        assert_eq!(found.bridge, Bridge::CodeVocabulary);
        assert_eq!(found.status, Status::Observed);
        assert!(found.confidence < 0.8, "weaker than anything declared");
    }

    #[test]
    fn a_phrase_used_once_is_a_name_not_a_concept() {
        let symbols: Vec<(String, String)> = [
            ("customer_group", "sym:a.py#customer_group"),
            ("wildly_different_thing", "sym:b.py#wildly_different_thing"),
            ("another_unique_name", "sym:c.py#another_unique_name"),
            (
                "yet_more_distinct_words",
                "sym:d.py#yet_more_distinct_words",
            ),
        ]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
        assert!(from_code_vocabulary(&symbols, &nothing_named()).is_empty());
    }

    #[test]
    fn structural_boilerplate_is_measured_out_rather_than_hard_coded() {
        // Every codebase has its own boilerplate vocabulary. Ubiquity within *this*
        // repository identifies it without a curated list that fits only one stack.
        let mut symbols: Vec<(String, String)> = Vec::new();
        for i in 0..200 {
            symbols.push((
                format!("abstract_factory_{i}"),
                format!("sym:f{i}.java#a{i}"),
            ));
        }
        for i in 0..5 {
            symbols.push((
                format!("patient_encounter_{i}"),
                format!("sym:p{i}.java#p{i}"),
            ));
        }
        let ids: Vec<String> = from_code_vocabulary(&symbols, &nothing_named())
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert!(
            ids.contains(&"PATIENT_ENCOUNTER".to_string()),
            "got {ids:?}"
        );
        assert!(
            !ids.contains(&"ABSTRACT_FACTORY".to_string()),
            "a phrase in 200 of 205 symbols is boilerplate: {ids:?}"
        );
    }

    #[test]
    fn a_domain_term_survives_the_boilerplate_filter_on_a_small_repository() {
        // Two percent of a small repository is a handful of symbols. Without a floor,
        // the ubiquity filter deletes the very vocabulary it exists to find.
        let symbols: Vec<(String, String)> = (0..6)
            .map(|i| {
                (
                    format!("patient_encounter_{i}"),
                    format!("sym:p{i}.java#p{i}"),
                )
            })
            .collect();
        let ids: Vec<String> = from_code_vocabulary(&symbols, &nothing_named())
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(ids, vec!["PATIENT_ENCOUNTER"], "got {ids:?}");
    }

    #[test]
    fn vocabulary_mining_is_deterministic_and_bounded() {
        let symbols: Vec<(String, String)> = (0..2000)
            .map(|i| {
                (
                    format!("domain_term_{}", i % 300),
                    format!("sym:f{i}.py#s{i}"),
                )
            })
            .collect();
        let first = from_code_vocabulary(&symbols, &nothing_named());
        let second = from_code_vocabulary(&symbols, &nothing_named());
        assert_eq!(
            first.iter().map(|c| &c.id).collect::<Vec<_>>(),
            second.iter().map(|c| &c.id).collect::<Vec<_>>()
        );
        assert!(first.len() <= MAX_VOCABULARY_CONCEPTS);
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
        assert_eq!(
            c.labels["vie"], "khách hàng chiến lược",
            "and still gain the label"
        );
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
        assert!(sets
            .iter()
            .any(|s| s.contains("strategic") && !s.contains("khách")));
        assert!(sets
            .iter()
            .any(|s| s.contains("khách") && !s.contains("strategic")));
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
        assert_eq!(
            batch.edges.len(),
            1,
            "the English surface form must ground it"
        );
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
    fn a_weaker_bridge_links_less_widely() {
        let mut idx = TermIndex::default();
        for i in 0..30 {
            idx.add(
                &format!("strategic_account_{i}"),
                &format!("sym:f{i}.py#strategic_account_{i}"),
            );
        }
        let concept_for = |bridge: Bridge| Concept {
            id: "STRATEGIC_ACCOUNT".into(),
            labels: BTreeMap::from([("eng".into(), "strategic account".into())]),
            status: Status::Observed,
            confidence: 0.8,
            bridge,
            ..Concept::default()
        };
        let links = |bridge| stage(&[concept_for(bridge)], &idx).edges.len();
        assert!(
            links(Bridge::Declared) > links(Bridge::CodeVocabulary),
            "evidence strength must bound how much code a concept claims"
        );
        assert_eq!(
            links(Bridge::CodeVocabulary),
            max_links(Bridge::CodeVocabulary)
        );
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
        assert_eq!(
            two,
            vec![("Strategic Account".into(), "khách hàng chiến lược".into())]
        );

        let three = parse_translation_csv("Selling,Strategic Account,khách hàng chiến lược\n");
        assert_eq!(
            three,
            vec![("Strategic Account".into(), "khách hàng chiến lược".into())]
        );
    }

    #[test]
    fn untranslated_rows_are_dropped() {
        let rows = parse_translation_csv("Order,Order\n,\nSubmit,Gửi\n");
        assert_eq!(rows, vec![("Submit".into(), "Gửi".into())]);
    }

    #[test]
    fn properties_bundles_parse_including_continuations_and_comments() {
        let text = "# a comment\n!another\n\nlogin.title=OpenMRS - Login\n\
                    long.key=first part \\\n  second part\nempty.key=\n";
        let rows = parse_properties(text);
        assert_eq!(rows[0], ("login.title".into(), "OpenMRS - Login".into()));
        assert_eq!(
            rows[1],
            ("long.key".into(), "first part second part".into())
        );
        assert_eq!(rows.len(), 2, "empty values are not vocabulary: {rows:?}");
    }

    #[test]
    fn a_bundle_locale_is_the_suffix_not_the_stem() {
        // The opposite of the CSV convention, where the whole stem is the language.
        assert_eq!(
            bundle_locale("resources/messages_fr.properties").as_deref(),
            Some("fra")
        );
        assert_eq!(
            bundle_locale("resources/messages_pt_BR.properties").as_deref(),
            Some("por")
        );
        assert_eq!(
            bundle_locale("resources/messages.properties"),
            None,
            "the base bundle"
        );
        assert_eq!(bundle_locale("config/application.properties"), None);
    }

    #[test]
    fn message_bundles_join_across_locales_by_key() {
        let bundles = vec![
            (None, "alert.text".to_string(), "Alert text".to_string()),
            (None, "alert.read".to_string(), "Alert read".to_string()),
            (
                Some("fra".to_string()),
                "alert.text".to_string(),
                "Texte alerte".to_string(),
            ),
            // No base entry, so no pair can be made.
            (
                Some("fra".to_string()),
                "orphan".to_string(),
                "Orphelin".to_string(),
            ),
            // Identical to the base: not actually translated.
            (
                Some("fra".to_string()),
                "alert.read".to_string(),
                "Alert read".to_string(),
            ),
        ];
        let joined = join_message_bundles(&bundles);
        assert_eq!(
            joined["fra"],
            vec![("Alert text".to_string(), "Texte alerte".to_string())]
        );
    }

    #[test]
    fn translation_language_is_read_from_the_filename() {
        assert_eq!(
            translation_language("erpnext/translations/vi.csv").as_deref(),
            Some("vie")
        );
        assert_eq!(
            translation_language("locale/ja_JP.csv").as_deref(),
            Some("jpn")
        );
        assert_eq!(translation_language("data/customers.csv"), None);
    }

    #[test]
    fn concept_ids_are_stable_and_opaque() {
        assert_eq!(normalize_id("Strategic Account"), "STRATEGIC_ACCOUNT");
        assert_eq!(normalize_id("  L2 approval! "), "L2_APPROVAL");
        assert_eq!(normalize_id("???"), "CONCEPT");
    }

    #[test]
    fn inflection_folding_matches_plurals_without_over_stemming() {
        assert!(same_word("customers", "customer"));
        assert!(same_word("requires", "require"));
        assert!(same_word("order", "orders"));
        // Must not fold words that merely end in s, or collapse unrelated stems.
        assert!(!same_word("class", "clas"));
        assert!(!same_word("billing", "bill"));
        assert!(!same_word("approval", "approve"));
        assert_eq!(stem("is"), "is", "short words are left alone");
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
        let expected: BTreeSet<String> = ["customer", "group"]
            .iter()
            .map(|s| s.to_string())
            .collect();
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

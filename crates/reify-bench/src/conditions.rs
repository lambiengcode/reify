//! Benchmark conditions: what the agent has to work with.
//!
//! Every condition answers the same question — *which files should I open for this
//! task, in what order* — and is charged the same way: the tokens an agent would spend
//! consuming the answer plus the files it recommends.
//!
//! The baselines are steel-manned on purpose. A weak baseline produces a flattering
//! result and teaches nothing, so the lexical baseline ranks the way a competent
//! engineer greps: distinct query terms first, raw frequency only as a tie-break.

use anyhow::Result;
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};

use reify::concepts::meaningful_words;
use reify::context::{self, ContextOptions};
use reify::store::Store;
use reify::tokens;

/// A condition's answer: an ordered list of files, and what it cost to produce.
#[derive(Debug, Clone, Serialize)]
pub struct Answer {
    /// Files in the order the agent would look at them.
    pub files: Vec<String>,
    /// Tokens the tool's own output costs the agent.
    pub answer_tokens: u32,
    /// Estimated tokens to read each recommended file or span, aligned with `files`.
    pub read_tokens: Vec<u32>,
    pub elapsed_ms: u128,
}

impl Answer {
    /// Total tokens to consume the answer and read everything it recommends.
    pub fn total_tokens(&self) -> u32 {
        self.answer_tokens + self.read_tokens.iter().sum::<u32>()
    }
}

/// The repository's text, loaded once and shared by every lexical condition.
pub struct Corpus {
    /// `path -> (lowercased content, estimated tokens to read the whole file)`
    files: Vec<(String, String, u32)>,
}

impl Corpus {
    pub fn load(root: &std::path::Path) -> Result<Corpus> {
        let found = reify::discover::discover(root)?;
        let mut files = Vec::with_capacity(found.files.len());
        for file in &found.files {
            if !file.lang.is_code() {
                continue;
            }
            let Ok(text) = file.read() else { continue };
            let cost = tokens::estimate(&text);
            files.push((file.path.clone(), text.to_lowercase(), cost));
        }
        Ok(Corpus { files })
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }
}

/// Baseline A: content search, ranked as a competent engineer would rank it.
///
/// Files are ordered by how many *distinct* task terms they contain, then by total
/// occurrences. Files are taken until the token budget is exhausted, so the baseline
/// is charged exactly the budget Reify is charged.
pub fn content_search(corpus: &Corpus, prompt: &str, budget: u32) -> Answer {
    let started = std::time::Instant::now();
    let terms: Vec<String> = meaningful_words(prompt).into_iter().collect();
    let mut scored: Vec<(&str, usize, usize, u32)> = Vec::new();
    for (path, text, cost) in &corpus.files {
        let mut distinct = 0usize;
        let mut total = 0usize;
        for term in &terms {
            let n = text.matches(term.as_str()).count();
            if n > 0 {
                distinct += 1;
                total += n;
            }
        }
        if distinct > 0 {
            scored.push((path.as_str(), distinct, total, *cost));
        }
    }
    scored.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then(b.2.cmp(&a.2))
            .then(a.3.cmp(&b.3)) // cheaper file first at equal relevance
            .then(a.0.cmp(b.0))
    });

    take_within_budget(scored, budget, started)
}

/// Baseline B: path search — what an agent does first, and often all it does.
pub fn path_search(corpus: &Corpus, prompt: &str, budget: u32) -> Answer {
    let started = std::time::Instant::now();
    let terms: Vec<String> = meaningful_words(prompt).into_iter().collect();
    let mut scored: Vec<(&str, usize, usize, u32)> = Vec::new();
    for (path, _, cost) in &corpus.files {
        let lowered = path.to_lowercase();
        let distinct = terms
            .iter()
            .filter(|t| lowered.contains(t.as_str()))
            .count();
        if distinct > 0 {
            scored.push((path.as_str(), distinct, 0, *cost));
        }
    }
    scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.3.cmp(&b.3)).then(a.0.cmp(b.0)));
    take_within_budget(scored, budget, started)
}

fn take_within_budget(
    scored: Vec<(&str, usize, usize, u32)>,
    budget: u32,
    started: std::time::Instant,
) -> Answer {
    let mut files = Vec::new();
    let mut read_tokens = Vec::new();
    let mut spent = 0u32;
    for (path, _, _, cost) in scored {
        if spent + cost > budget {
            continue;
        }
        spent += cost;
        files.push(path.to_string());
        read_tokens.push(cost);
    }
    Answer {
        files,
        // A grep result costs the agent almost nothing to read; the cost is the files.
        answer_tokens: 0,
        read_tokens,
        elapsed_ms: started.elapsed().as_millis(),
    }
}

/// Reify: the compiled context, plus the spans it says to read next.
///
/// Charged for both halves — the context output itself *and* the reads it recommends —
/// so the comparison against a baseline that only pays for file reads is not rigged.
pub fn reify_context(store: &Store, prompt: &str, budget: u32) -> Result<Answer> {
    let started = std::time::Instant::now();
    let compiled = context::compile(
        store,
        prompt,
        &ContextOptions {
            budget,
            max_next_reads: 12,
        },
    )?;

    // Order: the reading plan first, then any other file the context names. A span is
    // charged for the span, not for the whole file — that is the point of the plan.
    let mut files: Vec<String> = Vec::new();
    let mut read_tokens: Vec<u32> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut per_file: HashMap<String, u32> = HashMap::new();

    for read in &compiled.next_reads {
        *per_file.entry(read.path.clone()).or_insert(0) += read.est_tokens;
    }
    for read in &compiled.next_reads {
        if seen.insert(read.path.clone()) {
            files.push(read.path.clone());
            read_tokens.push(*per_file.get(&read.path).unwrap_or(&0));
        }
    }
    // Files the context names but the reading plan did not fund. The agent is told
    // about them but is not told to read them, so they cost nothing extra beyond the
    // context line already charged.
    for item in &compiled.code {
        if item.path.is_empty() || !seen.insert(item.path.clone()) {
            continue;
        }
        files.push(item.path.clone());
        read_tokens.push(0);
    }

    Ok(Answer {
        files,
        // Charged for the context output only; the reads are itemised above, exactly
        // as the budget contract splits them.
        answer_tokens: compiled.budget.context,
        read_tokens,
        elapsed_ms: started.elapsed().as_millis(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corpus() -> Corpus {
        Corpus {
            files: vec![
                (
                    "app/pricing.py".into(),
                    "def apply_discount(customer):\n    return strategic_rate(customer)".into(),
                    20,
                ),
                (
                    "app/report.py".into(),
                    "def report():\n    return list_of_orders()".into(),
                    15,
                ),
                (
                    "app/huge.py".into(),
                    "discount customer strategic ".repeat(50),
                    5_000,
                ),
            ],
        }
    }

    #[test]
    fn content_search_prefers_files_matching_more_distinct_terms() {
        // pricing.py contains all three task terms; huge.py contains two of them many
        // times over. Distinct coverage must beat raw frequency, which is how a
        // competent engineer greps and what makes this a fair baseline.
        let answer = content_search(&corpus(), "apply a discount for a customer", 10_000);
        assert_eq!(
            answer.files.first().map(String::as_str),
            Some("app/pricing.py")
        );
        assert!(answer.files.contains(&"app/huge.py".to_string()));
        assert!(
            !answer.files.contains(&"app/report.py".to_string()),
            "a file matching no term must not be offered at all"
        );
    }

    #[test]
    fn the_baseline_is_charged_the_same_budget() {
        let answer = content_search(&corpus(), "discount customer strategic", 100);
        assert!(
            answer.total_tokens() <= 100,
            "spent {}",
            answer.total_tokens()
        );
        assert!(
            !answer.files.contains(&"app/huge.py".to_string()),
            "a file that does not fit must be skipped, not truncated"
        );
    }

    #[test]
    fn path_search_finds_files_named_after_the_task() {
        let answer = path_search(&corpus(), "fix the pricing calculation", 10_000);
        assert_eq!(answer.files, vec!["app/pricing.py"]);
    }

    #[test]
    fn a_task_matching_nothing_yields_an_empty_answer() {
        let answer = content_search(&corpus(), "quantum flux capacitor", 10_000);
        assert!(answer.files.is_empty());
        assert_eq!(answer.total_tokens(), 0);
    }
}

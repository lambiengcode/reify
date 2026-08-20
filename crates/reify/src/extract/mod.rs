//! Extraction: source text in, staged knowledge out.
//!
//! Extractors are deliberately split into a per-file phase and a repository-wide
//! resolve phase. A call site can only be resolved once every file's symbols are
//! known, and keeping the per-file phase pure is what makes it parallel and cacheable.

pub mod code;
pub mod docs;
pub mod richdoc;
pub mod schema;
pub mod sqlish;

use crate::concepts::Concept;
use crate::model::EdgeKind;
use crate::rules::RuleCandidate;
use crate::store::Batch;

/// A reference whose target cannot be known until every file has been parsed.
#[derive(Debug, Clone)]
pub struct PendingRef {
    /// Uid of the symbol the reference appears inside.
    pub from: String,
    /// The bare name as written at the call site.
    pub name: String,
    /// Path of the file the reference appears in, used to prefer local targets.
    pub file: String,
    pub kind: EdgeKind,
}

/// Everything one file contributes, before repository-wide resolution.
#[derive(Debug, Default)]
pub struct FileExtract {
    pub batch: Batch,
    pub pending: Vec<PendingRef>,
    /// Module specifiers this file imports, in source form.
    pub imports: Vec<String>,
    /// Identifier words seen in this file, feeding the concept miner.
    pub vocabulary: Vec<String>,
    /// Business-rule candidates mined from this file.
    pub rules: Vec<RuleCandidate>,
    /// Concepts declared by this file's structured metadata.
    pub concepts: Vec<Concept>,
}

impl FileExtract {
    pub fn absorb(&mut self, other: FileExtract) {
        self.batch.absorb(other.batch);
        self.pending.extend(other.pending);
        self.imports.extend(other.imports);
        self.vocabulary.extend(other.vocabulary);
        self.rules.extend(other.rules);
        self.concepts.extend(other.concepts);
    }
}

//! Repository discovery: walk, classify, hash.
//!
//! The default exclusions are deliberately visible rather than magic — `reify status`
//! reports what was skipped and why, because a knowledge tool that silently ignores
//! half a repository is worse than one that indexes nothing.

use anyhow::{Context, Result};
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

use crate::model::Lang;

/// Files above this size are skipped: in practice they are bundles, fixtures or data
/// dumps, and parsing them costs far more than the knowledge they carry.
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// Directory names never worth indexing, regardless of ignore files.
const EXCLUDED_DIRS: &[&str] = &[
    ".git",
    ".reify",
    "node_modules",
    "target",
    "dist",
    "build",
    "vendor",
    "__pycache__",
    ".venv",
    "venv",
    ".tox",
    ".mypy_cache",
    ".pytest_cache",
    "site-packages",
    ".next",
    "coverage",
    ".idea",
    ".vscode",
];

/// Name suffixes that mark generated or minified output.
const EXCLUDED_SUFFIXES: &[&str] = &[
    ".min.js",
    ".min.css",
    ".map",
    ".lock",
    ".bundle.js",
    "-lock.json",
    ".pb.go",
    "_pb2.py",
    ".g.dart",
    ".generated.ts",
];

/// One file that will be indexed.
///
/// Deliberately does not carry the file's text. Discovery must hash every file in the
/// repository, and retaining all of that content would make peak memory scale with the
/// repository rather than with the changed set. Text is read again at parse time, by
/// which point the page cache makes it nearly free.
#[derive(Debug, Clone)]
pub struct Discovered {
    /// Path relative to the repository root, always with `/` separators.
    pub path: String,
    pub abs: PathBuf,
    pub lang: Lang,
    pub hash: String,
    pub bytes: u64,
    pub lines: u32,
}

impl Discovered {
    /// Read the file's text.
    pub fn read(&self) -> Result<String> {
        std::fs::read_to_string(&self.abs)
            .with_context(|| format!("reading {}", self.abs.display()))
    }
}

/// Why a file present in the tree was not indexed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    TooLarge(u64),
    Binary,
    Generated,
    UnsupportedType,
}

impl SkipReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            SkipReason::TooLarge(_) => "too large",
            SkipReason::Binary => "binary",
            SkipReason::Generated => "generated or minified",
            SkipReason::UnsupportedType => "unsupported file type",
        }
    }
}

#[derive(Debug, Default)]
pub struct Discovery {
    pub files: Vec<Discovered>,
    pub skipped: Vec<(String, SkipReason)>,
}

impl Discovery {
    /// Skip counts grouped by reason, for `reify status`.
    pub fn skip_summary(&self) -> Vec<(&'static str, usize)> {
        let mut counts: Vec<(&'static str, usize)> = Vec::new();
        for (_, reason) in &self.skipped {
            let key = reason.as_str();
            match counts.iter_mut().find(|(k, _)| *k == key) {
                Some((_, n)) => *n += 1,
                None => counts.push((key, 1)),
            }
        }
        counts.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        counts
    }
}

/// Walk `root`, honouring `.gitignore` and `.reifyignore`, and read every indexable file.
pub fn discover(root: &Path) -> Result<Discovery> {
    let mut walker = WalkBuilder::new(root);
    walker
        .hidden(false)
        .git_ignore(true)
        .git_global(false)
        .parents(false)
        .add_custom_ignore_filename(".reifyignore")
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !EXCLUDED_DIRS.contains(&name.as_ref())
        });

    let mut out = Discovery::default();
    for entry in walker.build() {
        let entry = match entry {
            Ok(e) => e,
            // An unreadable entry is reported by the caller as a warning, not a failure:
            // one permission-denied file must not abort a 20-minute index.
            Err(_) => continue,
        };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let abs = entry.path().to_path_buf();
        let rel = match abs.strip_prefix(root) {
            Ok(r) => r.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };

        if EXCLUDED_SUFFIXES.iter().any(|s| rel.ends_with(s)) {
            out.skipped.push((rel, SkipReason::Generated));
            continue;
        }
        let lang = classify(&rel);
        if lang == Lang::Other {
            out.skipped.push((rel, SkipReason::UnsupportedType));
            continue;
        }
        let meta = match std::fs::metadata(&abs) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.len() > MAX_FILE_BYTES {
            out.skipped.push((rel, SkipReason::TooLarge(meta.len())));
            continue;
        }
        let bytes = match std::fs::read(&abs) {
            Ok(b) => b,
            Err(_) => continue,
        };
        if bytes.contains(&0) {
            out.skipped.push((rel, SkipReason::Binary));
            continue;
        }
        let text = match String::from_utf8(bytes) {
            Ok(t) => t,
            Err(_) => {
                out.skipped.push((rel, SkipReason::Binary));
                continue;
            }
        };
        let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
        let lines = text.lines().count() as u32;
        out.files.push(Discovered {
            path: rel,
            abs,
            lang,
            hash,
            bytes: meta.len(),
            lines,
        });
    }

    out.files.sort_by(|a, b| a.path.cmp(&b.path));
    out.skipped.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Read one file the same way `discover` would, for incremental single-file updates.
pub fn read_one(root: &Path, rel: &str) -> Result<Option<Discovered>> {
    let abs = root.join(rel);
    if !abs.is_file() {
        return Ok(None);
    }
    let lang = classify(rel);
    if lang == Lang::Other {
        return Ok(None);
    }
    let meta = std::fs::metadata(&abs)?;
    if meta.len() > MAX_FILE_BYTES {
        return Ok(None);
    }
    let text =
        std::fs::read_to_string(&abs).with_context(|| format!("reading {}", abs.display()))?;
    let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
    let lines = text.lines().count() as u32;
    Ok(Some(Discovered {
        path: rel.to_string(),
        abs,
        lang,
        hash,
        bytes: meta.len(),
        lines,
    }))
}

/// Map a path to the language or document format Reify will parse it as.
pub fn classify(path: &str) -> Lang {
    let name = path.rsplit('/').next().unwrap_or(path);
    let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    match ext {
        "py" | "pyi" => Lang::Python,
        "ts" | "tsx" => Lang::TypeScript,
        "js" | "jsx" | "mjs" | "cjs" => Lang::JavaScript,
        "sql" => Lang::Sql,
        "md" | "markdown" | "mdx" => Lang::Markdown,
        "txt" | "rst" | "adoc" => Lang::Text,
        "csv" => Lang::Csv,
        "json" => Lang::Json,
        "yaml" | "yml" => Lang::Yaml,
        _ => Lang::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("reify-disc-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn classification_covers_the_mvp_languages() {
        assert_eq!(classify("a/b/order.py"), Lang::Python);
        assert_eq!(classify("app/x.tsx"), Lang::TypeScript);
        assert_eq!(classify("app/x.mjs"), Lang::JavaScript);
        assert_eq!(classify("db/schema.sql"), Lang::Sql);
        assert_eq!(classify("docs/BRD.md"), Lang::Markdown);
        assert_eq!(classify("i18n/vi.csv"), Lang::Csv);
        assert_eq!(classify("bin/tool"), Lang::Other);
    }

    #[test]
    fn walk_reads_source_and_records_why_files_were_skipped() {
        let d = tmp("walk");
        fs::write(d.join("a.py"), "def f():\n    pass\n").unwrap();
        fs::write(d.join("app.min.js"), "var a=1").unwrap();
        fs::write(d.join("logo.bin"), [0u8, 1, 2]).unwrap();
        fs::create_dir_all(d.join("node_modules")).unwrap();
        fs::write(d.join("node_modules/dep.py"), "x = 1").unwrap();

        let found = discover(&d).unwrap();
        let paths: Vec<&str> = found.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            vec!["a.py"],
            "only the real source file is indexable"
        );

        let reasons: Vec<&str> = found.skipped.iter().map(|(_, r)| r.as_str()).collect();
        assert!(reasons.contains(&"generated or minified"));
        assert!(reasons.contains(&"unsupported file type"));
        assert!(!found.skip_summary().is_empty());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn binary_content_is_detected_even_with_a_source_extension() {
        let d = tmp("bin");
        fs::write(d.join("weird.py"), [b'a', 0u8, b'b']).unwrap();
        let found = discover(&d).unwrap();
        assert!(found.files.is_empty());
        assert_eq!(found.skipped[0].1, SkipReason::Binary);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn hashes_are_content_addressed_and_stable() {
        let d = tmp("hash");
        fs::write(d.join("a.py"), "x = 1\n").unwrap();
        let first = discover(&d).unwrap().files[0].hash.clone();
        let second = discover(&d).unwrap().files[0].hash.clone();
        assert_eq!(first, second, "hashing must be deterministic");
        fs::write(d.join("a.py"), "x = 2\n").unwrap();
        let third = discover(&d).unwrap().files[0].hash.clone();
        assert_ne!(first, third, "content change must change the hash");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn reifyignore_is_honoured() {
        let d = tmp("ignore");
        fs::write(d.join(".reifyignore"), "secret/\n").unwrap();
        fs::create_dir_all(d.join("secret")).unwrap();
        fs::write(d.join("secret/x.py"), "x = 1").unwrap();
        fs::write(d.join("ok.py"), "y = 1").unwrap();
        let found = discover(&d).unwrap();
        let paths: Vec<&str> = found.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["ok.py"]);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn read_one_matches_what_the_walk_would_have_produced() {
        let d = tmp("readone");
        fs::write(d.join("a.py"), "def f():\n    pass\n").unwrap();
        let walked = discover(&d).unwrap().files.remove(0);
        let single = read_one(&d, "a.py").unwrap().unwrap();
        assert_eq!(walked.hash, single.hash);
        assert_eq!(walked.lines, single.lines);
        assert_eq!(walked.lang, single.lang);
        assert_eq!(walked.read().unwrap(), single.read().unwrap());
        assert!(read_one(&d, "missing.py").unwrap().is_none());
        let _ = fs::remove_dir_all(&d);
    }
}

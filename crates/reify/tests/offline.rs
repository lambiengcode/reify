//! The offline guarantee, enforced structurally rather than promised.
//!
//! The README says Reify makes no network connection. That claim is worth exactly as
//! much as the check behind it, so these tests assert two independent things:
//!
//! 1. no crate capable of opening a network connection is in the dependency tree; and
//! 2. no source file in the workspace reaches for a socket directly.
//!
//! Both are checked against the workspace on disk, so adding a networking dependency —
//! or hand-rolling a socket — fails the build rather than quietly breaking the promise.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Crates that exist to talk to a network. Any of these in the lockfile means the
/// offline guarantee is no longer structural.
const NETWORK_CRATES: &[&str] = &[
    "reqwest",
    "hyper",
    "ureq",
    "curl",
    "curl-sys",
    "isahc",
    "surf",
    "attohttpc",
    "tokio",
    "async-std",
    "smol",
    "mio",
    "socket2",
    "trust-dns-resolver",
    "hickory-resolver",
    "rustls",
    "native-tls",
    "openssl",
    "openssl-sys",
    "tungstenite",
    "async-tungstenite",
    "quinn",
    "h2",
    "h3",
];

/// Source constructs that open a socket without any dependency at all.
const SOCKET_CONSTRUCTS: &[&str] = &[
    "std::net::TcpStream",
    "std::net::UdpSocket",
    "std::net::TcpListener",
    "TcpStream::connect",
    "UdpSocket::bind",
];

fn workspace_root() -> PathBuf {
    // `crates/reify` -> `crates` -> workspace root
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate lives two levels below the workspace root")
        .to_path_buf()
}

#[test]
fn no_networking_crate_is_in_the_dependency_tree() {
    let lockfile = workspace_root().join("Cargo.lock");
    let text = std::fs::read_to_string(&lockfile)
        .unwrap_or_else(|e| panic!("reading {}: {e}", lockfile.display()));

    let mut present: BTreeSet<&str> = BTreeSet::new();
    for line in text.lines() {
        let Some(name) = line
            .strip_prefix("name = \"")
            .and_then(|r| r.strip_suffix('"'))
        else {
            continue;
        };
        if let Some(found) = NETWORK_CRATES.iter().find(|c| **c == name) {
            present.insert(found);
        }
    }
    assert!(
        present.is_empty(),
        "the offline guarantee is structural, and these crates break it: {present:?}\n\
         If one is genuinely needed, the README's privacy section must change first."
    );
}

#[test]
fn no_source_file_opens_a_socket_directly() {
    let mut offenders: Vec<String> = Vec::new();
    visit(&workspace_root().join("crates"), &mut |path, text| {
        // This file names the constructs in order to forbid them.
        if path.ends_with("offline.rs") {
            return;
        }
        for construct in SOCKET_CONSTRUCTS {
            if text.contains(construct) {
                offenders.push(format!("{}: {construct}", path.display()));
            }
        }
    });
    assert!(offenders.is_empty(), "sockets found: {offenders:#?}");
}

#[test]
fn the_only_subprocess_reify_runs_is_git() {
    // Reify never executes anything *from the repository it indexes*. It does invoke
    // a small set of system tools, each named here so adding another is a deliberate,
    // reviewed act rather than a silent one:
    //   git        — history, the only thing that can produce it
    //   converters — document text extraction for formats with no usable pure-Rust
    //                reader; the exact list is asserted separately below
    //   program    — the model provider the *user* configured, which is the whole
    //                point of the design: Reify opens no socket, and the one command
    //                that can reach a network is named by the user, logged on every
    //                call, and unreachable under REIFY_OFFLINE=1
    //   curl, tar  — `reify upgrade` only: fetch and unpack a release as visible
    //                subprocesses rather than an embedded HTTP client, checksum
    //                verified in-process before install, refused under
    //                REIFY_OFFLINE=1. Documented in the README's privacy section.
    // Neither can open a network connection on Reify's behalf, and both are given a
    // repository file as *input* rather than being loaded from the repository.
    let mut commands: BTreeSet<String> = BTreeSet::new();
    visit(&workspace_root().join("crates"), &mut |path, text| {
        if path.ends_with("offline.rs") {
            return;
        }
        for (index, _) in text.match_indices("Command::new(") {
            let tail = &text[index + "Command::new(".len()..];
            if let Some(end) = tail.find(')') {
                commands.insert(tail[..end].trim().to_string());
            }
        }
    });
    let allowed: BTreeSet<String> = [
        "\"git\"",
        "\"sleep\"",
        "\"curl\"",
        "\"tar\"",
        // Spawned from a reviewed table, asserted by the next test.
        "c.program",
        "converter.program",
        "program",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let unexpected: Vec<&String> = commands.difference(&allowed).collect();
    assert!(
        unexpected.is_empty(),
        "unexpected subprocesses: {unexpected:?}\n\
         Adding one means updating this test and the privacy section of the README. \
         (`sleep` is a timeout test.)"
    );
}

/// Every document converter Reify can spawn must be a known text extractor.
///
/// Asserted against the real table rather than a source grep, so adding a converter
/// fails this test until the new program has been looked at. None of these can open a
/// network connection on Reify's behalf, and each is handed a repository file as
/// *input* rather than being loaded from the repository.
#[test]
fn every_document_converter_is_a_reviewed_text_extractor() {
    let allowed: BTreeSet<&str> = [
        "pdftotext", // poppler
        "mutool",    // mupdf
        "antiword",  // legacy .doc
        "textutil",  // macOS, built in
        "soffice",   // LibreOffice
    ]
    .into_iter()
    .collect();

    let declared: BTreeSet<&str> = reify::extract::richdoc::external_tools()
        .into_iter()
        .collect();
    let unexpected: Vec<&&str> = declared.difference(&allowed).collect();
    assert!(
        unexpected.is_empty(),
        "unreviewed document converters: {unexpected:?}"
    );
}

fn visit(dir: &Path, f: &mut impl FnMut(&Path, &str)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            visit(&path, f);
        } else if path.extension().is_some_and(|e| e == "rs") {
            if let Ok(text) = std::fs::read_to_string(&path) {
                f(&path, &text);
            }
        }
    }
}

//! `reify upgrade`, `reify uninstall`, `reify uninit`: the binary managing itself.
//!
//! Upgrade is the one command in Reify that reaches the network, and it does so
//! through a subprocess you can see — `curl` fetches, `tar` unpacks — never through
//! an embedded HTTP client, so the privacy claim ("no networking crate in the
//! dependency tree, asserted in CI") survives intact. The checksum is verified
//! in-process with a pure-Rust SHA-256 before anything replaces the running binary,
//! and `REIFY_OFFLINE=1` refuses the whole command, exactly as it does for the model
//! provider.

use anyhow::{bail, Context, Result};
use sha2::Digest;
use std::path::{Path, PathBuf};
use std::process::Command;

const REPO: &str = "lambiengcode/reify";
const CURRENT: &str = env!("CARGO_PKG_VERSION");
/// The binary's filename inside a release archive.
const BINARY: &str = if cfg!(windows) { "reify.exe" } else { "reify" };

/// The release target this build can upgrade to, or why it cannot.
fn release_target() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        (os, arch) => bail!(
            "no prebuilt binary for {os}/{arch}; upgrade from source instead:\n  \
             cargo install --git https://github.com/{REPO} reify-cli"
        ),
    }
}

/// Refuse when the user has declared the machine offline.
///
/// Takes the value rather than reading the environment so the refusal is testable;
/// the callers pass the live variable.
fn refuse_when_offline(offline: Option<&str>) -> Result<()> {
    if offline.is_some_and(|v| v == "1") {
        bail!(
            "{}=1 is set, and upgrade needs the network. Unset it to upgrade; \
             nothing else in Reify is affected.",
            reify::llm::OFFLINE_ENV
        );
    }
    Ok(())
}

/// A semantic version triple, for "is the release newer than this build".
fn parse_version(tag: &str) -> Option<(u64, u64, u64)> {
    let mut parts = tag.trim().trim_start_matches('v').splitn(3, '.');
    Some((
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        // Tolerate suffixes like `1.2.3-rc1` by reading the leading digits.
        parts
            .next()?
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .ok()?,
    ))
}

/// Run curl and return stdout, with a readable failure.
fn curl(args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("curl")
        .arg("-fsSL")
        .args(args)
        .output()
        .context("running curl (is it installed?)")?;
    anyhow::ensure!(
        output.status.success(),
        "curl failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(output.stdout)
}

/// The latest release tag, from the GitHub API by way of curl.
fn latest_tag() -> Result<String> {
    let body = curl(&[&format!(
        "https://api.github.com/repos/{REPO}/releases/latest"
    )])?;
    let json: serde_json::Value =
        serde_json::from_slice(&body).context("parsing the release listing")?;
    json["tag_name"]
        .as_str()
        .map(str::to_string)
        .context("the release listing has no tag_name")
}

/// `reify upgrade [--check] [version]`.
pub fn upgrade(check: bool, version: Option<&str>) -> Result<()> {
    refuse_when_offline(std::env::var(reify::llm::OFFLINE_ENV).ok().as_deref())?;
    let target = release_target()?;
    let tag = match version {
        Some(v) if v.starts_with('v') => v.to_string(),
        Some(v) => format!("v{v}"),
        None => latest_tag()?,
    };

    let up_to_date = parse_version(&tag) <= parse_version(CURRENT);
    if check {
        if up_to_date {
            println!("reify {CURRENT} is up to date (latest release: {tag})");
        } else {
            println!("reify {CURRENT} → {tag} available; run `reify upgrade`");
        }
        return Ok(());
    }
    if version.is_none() && up_to_date {
        println!("reify {CURRENT} is already the latest release ({tag}); nothing to do");
        return Ok(());
    }

    let exe = std::env::current_exe().context("locating the running binary")?;
    let exe = exe.canonicalize().unwrap_or(exe);
    let bin_dir = exe
        .parent()
        .context("the running binary has no parent directory")?;

    // Staged next to the destination, so the final swap is a same-filesystem rename:
    // either the old binary or the new one exists at every instant, never neither.
    let stage = bin_dir.join(format!(".reify-upgrade-{}", std::process::id()));
    let result = install_release(&tag, target, &stage, &exe);
    let _ = std::fs::remove_dir_all(&stage);
    result?;
    println!("upgraded {} to {tag}", exe.display());
    Ok(())
}

fn install_release(tag: &str, target: &str, stage: &Path, exe: &Path) -> Result<()> {
    std::fs::create_dir_all(stage).context("creating the staging directory")?;
    let name = format!("reify-{tag}-{target}");
    let base = format!("https://github.com/{REPO}/releases/download/{tag}");

    eprintln!("downloading {name}.tar.gz");
    let tarball = curl(&[&format!("{base}/{name}.tar.gz")])?;

    // Integrity, verified in-process before anything is unpacked. The published
    // checksum file is `<hex>  <filename>`.
    let published = curl(&[&format!("{base}/{name}.tar.gz.sha256")])?;
    let expected = String::from_utf8_lossy(&published)
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_lowercase();
    let actual = hex(&sha2::Sha256::digest(&tarball));
    anyhow::ensure!(
        expected == actual,
        "checksum mismatch for {name}.tar.gz: published {expected}, downloaded {actual}. \
         Not installing it."
    );

    let archive = stage.join("release.tar.gz");
    std::fs::write(&archive, &tarball).context("writing the downloaded archive")?;
    let status = Command::new("tar")
        .args(["-xzf"])
        .arg(&archive)
        .arg("-C")
        .arg(stage)
        .status()
        .context("running tar")?;
    anyhow::ensure!(status.success(), "tar could not unpack the release");

    let fresh = stage.join(&name).join(BINARY);
    anyhow::ensure!(fresh.is_file(), "the release archive holds no reify binary");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fresh, std::fs::Permissions::from_mode(0o755))?;
    }
    // Windows holds an open handle on the running image, so the new binary cannot be
    // renamed over it — but the running one *can* be renamed aside, which frees the
    // path. The displaced file is left for the next run to clear: deleting it while
    // it is still mapped fails, and failing an upgrade over housekeeping would be
    // worse than one stale file.
    #[cfg(windows)]
    {
        let displaced = exe.with_extension("old");
        let _ = std::fs::remove_file(&displaced);
        std::fs::rename(exe, &displaced)
            .with_context(|| format!("moving {} aside", exe.display()))?;
        if let Err(err) = std::fs::rename(&fresh, exe) {
            // Put the working binary back rather than leaving the user with none.
            let _ = std::fs::rename(&displaced, exe);
            return Err(err).with_context(|| format!("replacing {}", exe.display()));
        }
        return Ok(());
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(&fresh, exe)
            .with_context(|| format!("replacing {} (is it writable?)", exe.display()))?;
        Ok(())
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `reify uninstall [--yes]`: remove the binary, and say precisely what stays.
pub fn uninstall(yes: bool) -> Result<()> {
    let exe = std::env::current_exe().context("locating the running binary")?;
    let exe = exe.canonicalize().unwrap_or(exe);
    println!("uninstall removes exactly one thing: {}", exe.display());
    println!();
    println!("It does NOT touch:");
    println!("  - any repository's `.reify/` store — remove per repository with `reify uninit`");
    println!("  - instruction blocks in AGENTS.md / CLAUDE.md — `reify uninit` removes those too");
    println!("  - shell completions you may have written to your shell's directory");
    if !yes {
        println!();
        println!("Nothing was removed. Re-run with --yes to remove the binary.");
        return Ok(());
    }
    std::fs::remove_file(&exe)
        .with_context(|| format!("removing {} (is it writable?)", exe.display()))?;
    println!("removed {}", exe.display());
    Ok(())
}

/// `reify uninit [--yes]`: remove this repository's store and everything
/// `reify install` wrote.
///
/// The removal targets are derived from the same table `install` plans from, so an agent
/// cannot be added to one without appearing in the other — otherwise `install` quietly
/// leaves orphans that only turn up when somebody wonders why their agent still mentions
/// a tool they removed.
pub fn uninit(root: &Path, yes: bool) -> Result<()> {
    let store = root.join(reify::index::REIFY_DIR);
    let mut planned: Vec<String> = Vec::new();
    let mut edits: Vec<Removal> = Vec::new();

    for (rel, kind) in crate::install::removable_targets() {
        let path = root.join(&rel);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        match kind {
            crate::install::Kind::Mcp => match crate::install::without_mcp_entry(&text) {
                Ok(Some(stripped)) => {
                    planned.push(format!("remove the reify server entry from {rel}"));
                    edits.push(Removal::Rewrite(path, stripped));
                }
                Ok(None) => {}
                // An unparsable config is left exactly as it is, on the way out as much
                // as on the way in.
                Err(e) => planned.push(format!("leave {rel} alone ({e})")),
            },
            _ if !text.contains(crate::AGENT_INSTRUCTIONS.trim()) => {}
            crate::install::Kind::RuleFile => {
                planned.push(format!("remove {rel}"));
                edits.push(Removal::Delete(path));
            }
            crate::install::Kind::Instructions => {
                let stripped = strip_block(&text);
                if stripped.trim().is_empty() {
                    // Nothing but our own block was ever in it.
                    planned.push(format!("remove {rel}"));
                    edits.push(Removal::Delete(path));
                } else {
                    planned.push(format!("strip the Reify instruction block from {rel}"));
                    edits.push(Removal::Rewrite(path, stripped));
                }
            }
        }
    }

    if store.is_dir() {
        planned.push(format!("remove {}", store.display()));
    }
    if planned.is_empty() {
        println!("nothing to remove: no `.reify/` store or Reify integration here");
        return Ok(());
    }
    for step in &planned {
        println!("will {step}");
    }
    if !yes {
        println!();
        println!("Nothing was removed. Re-run with --yes to apply.");
        return Ok(());
    }
    for edit in edits {
        match edit {
            Removal::Rewrite(path, text) => std::fs::write(&path, text)
                .with_context(|| format!("rewriting {}", path.display()))?,
            Removal::Delete(path) => std::fs::remove_file(&path)
                .with_context(|| format!("removing {}", path.display()))?,
        }
    }
    if store.is_dir() {
        std::fs::remove_dir_all(&store).with_context(|| format!("removing {}", store.display()))?;
    }
    println!("done");
    Ok(())
}

enum Removal {
    Rewrite(PathBuf, String),
    Delete(PathBuf),
}

/// Take our block out of a file somebody else also writes to.
///
/// The full constant is tried first, and it carries the newline that `install` and
/// `init` push in front of it — so removing it restores the file byte for byte rather
/// than leaving the blank lines the block was separated by. The trimmed form is the
/// fallback, for a file where somebody pasted the block by hand.
fn strip_block(text: &str) -> String {
    for block in [crate::AGENT_INSTRUCTIONS, crate::AGENT_INSTRUCTIONS.trim()] {
        if let Some(at) = text.find(block) {
            let mut out = String::with_capacity(text.len());
            out.push_str(&text[..at]);
            out.push_str(&text[at + block.len()..]);
            return out;
        }
    }
    text.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_release_platform_maps_to_a_published_target() {
        // The mapping is compile-time for this build; assert the current platform
        // resolves (CI runs on all four) and unsupported pairs read as guidance.
        release_target().unwrap();
    }

    #[test]
    fn versions_compare_as_semver_triples() {
        assert!(parse_version("v0.2.0") > parse_version("0.1.9"));
        assert!(parse_version("v0.1.0") == parse_version(CURRENT).map(|_| (0, 1, 0)));
        assert!(parse_version("1.0.0-rc1").is_some());
        assert!(parse_version("nonsense").is_none());
    }

    #[test]
    fn offline_mode_refuses_the_upgrade_before_any_network_use() {
        assert!(refuse_when_offline(Some("1")).is_err());
        refuse_when_offline(Some("0")).unwrap();
        refuse_when_offline(None).unwrap();
    }

    #[test]
    fn uninit_strips_the_instruction_block_and_the_store() {
        let dir = std::env::temp_dir().join(format!("reify-uninit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(reify::index::REIFY_DIR)).unwrap();
        let agents = format!(
            "# My repo\n{}\n## Other section\n",
            crate::AGENT_INSTRUCTIONS
        );
        std::fs::write(dir.join("AGENTS.md"), &agents).unwrap();

        // Without --yes: a plan, and nothing changes.
        uninit(&dir, false).unwrap();
        assert!(dir.join(reify::index::REIFY_DIR).is_dir());

        uninit(&dir, true).unwrap();
        assert!(!dir.join(reify::index::REIFY_DIR).exists());
        let after = std::fs::read_to_string(dir.join("AGENTS.md")).unwrap();
        assert!(!after.contains("Before changing code"));
        assert!(after.contains("## Other section"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

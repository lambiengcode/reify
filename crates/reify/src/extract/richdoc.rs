//! HTML, DOCX and PDF ingestion.
//!
//! All three reduce to the same thing: text with headings. Each is converted to a
//! Markdown-shaped intermediate and handed to the section splitter, so a Confluence
//! export and a `.md` file produce citations of the same shape.
//!
//! PDF is the weak link and says so. There is no good pure-Rust PDF text extractor, so
//! Reify shells out to `pdftotext` when it is installed and **reports loudly** when it
//! is not. Silently indexing nothing would be worse than indexing nothing loudly.

use anyhow::{anyhow, Result};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

/// Bound on the external text extractor. A malformed PDF must not hang an index.
const EXTRACTOR_TIMEOUT: Duration = Duration::from_secs(20);

/// Convert an HTML document to the Markdown-shaped intermediate.
///
/// Hand-rolled rather than delegated to a parser: the job is to keep heading structure
/// and drop everything else, which is a scan, not a parse. A full DOM would cost a
/// dependency and buy nothing Reify uses.
pub fn html_to_markdown(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let bytes: Vec<char> = html.chars().collect();
    let lowered = html.to_ascii_lowercase();
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] != '<' {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        // Skip elements whose content is never prose.
        if let Some(skipped) = skip_element(&lowered, i, &["script", "style", "head", "svg"]) {
            i = skipped;
            continue;
        }
        let Some(close) = lowered[byte_index(&bytes, i)..].find('>') else {
            break;
        };
        let tag_start = byte_index(&bytes, i);
        let tag = &lowered[tag_start + 1..tag_start + close];
        let name: String = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();

        match name.as_str() {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" if !tag.starts_with('/') => {
                let level: usize = name[1..].parse().unwrap_or(1);
                out.push('\n');
                out.push_str(&"#".repeat(level));
                out.push(' ');
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => out.push('\n'),
            "p" | "div" | "br" | "li" | "tr" | "section" | "article" => out.push('\n'),
            _ => out.push(' '),
        }
        // Advance past the tag, in characters.
        i = char_index_after(&bytes, tag_start + close + 1);
    }

    decode_entities(&out)
}

/// Extract a DOCX document as the Markdown-shaped intermediate.
///
/// A `.docx` is a zip of XML. Paragraph style names carry the heading level, which is
/// what makes a Word document citable by section rather than as one wall of text.
pub fn docx_to_markdown(bytes: &[u8]) -> Result<String> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|e| anyhow!("not a readable .docx: {e}"))?;
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .map_err(|e| anyhow!("no word/document.xml in the archive: {e}"))?
        .read_to_string(&mut xml)
        .map_err(|e| anyhow!("reading word/document.xml: {e}"))?;
    Ok(wordml_to_markdown(&xml))
}

/// Convert WordprocessingML to the Markdown-shaped intermediate.
fn wordml_to_markdown(xml: &str) -> String {
    use quick_xml::events::Event;

    let mut reader = quick_xml::Reader::from_str(xml);
    let mut out = String::new();
    let mut buffer = Vec::new();
    let mut paragraph = String::new();
    let mut heading_level: Option<usize> = None;
    // Only text inside a `<w:t>` run is document content. Whitespace *between*
    // elements is formatting: a pretty-printed document would otherwise inject
    // newlines into the middle of every sentence, and a sentence Word split across
    // three runs would never match anything.
    let mut in_text_run = false;

    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = local_name(e.name().as_ref());
                if name == "pStyle" {
                    if let Some(style) = attribute(&e, "val") {
                        heading_level = heading_from_style(&style);
                    }
                }
                if name == "t" {
                    in_text_run = true;
                }
            }
            Ok(Event::Text(e)) if in_text_run => {
                if let Ok(text) = e.decode() {
                    paragraph.push_str(&text);
                }
            }
            Ok(Event::End(e)) => {
                let name = local_name(e.name().as_ref());
                if name == "t" {
                    in_text_run = false;
                }
                if name == "p" {
                    let trimmed = paragraph.trim();
                    if !trimmed.is_empty() {
                        match heading_level {
                            Some(level) => {
                                out.push('\n');
                                out.push_str(&"#".repeat(level.clamp(1, 6)));
                                out.push(' ');
                                out.push_str(trimmed);
                                out.push('\n');
                            }
                            None => {
                                out.push_str(trimmed);
                                out.push('\n');
                            }
                        }
                    }
                    paragraph.clear();
                    heading_level = None;
                } else if name == "tab" || name == "br" {
                    paragraph.push(' ');
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buffer.clear();
    }
    out
}

fn heading_from_style(style: &str) -> Option<usize> {
    let lowered = style.to_ascii_lowercase().replace(['-', ' '], "");
    let digits: String = lowered
        .strip_prefix("heading")?
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok().or(Some(1))
}

fn local_name(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    text.rsplit(':').next().unwrap_or(&text).to_string()
}

fn attribute(event: &quick_xml::events::BytesStart<'_>, wanted: &str) -> Option<String> {
    event.attributes().flatten().find_map(|attr| {
        (local_name(attr.key.as_ref()) == wanted)
            .then(|| String::from_utf8_lossy(&attr.value).to_string())
    })
}

/// Whether a PDF text extractor is available on this machine.
pub fn pdf_extractor_available() -> bool {
    Command::new("pdftotext")
        .arg("-v")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

/// Extract a PDF's text, or explain precisely why it could not be done.
///
/// The error message names the missing tool and how to install it, because a user
/// whose BRDs are all PDFs needs to know that Reify indexed none of them.
pub fn pdf_to_text(path: &Path) -> Result<String> {
    if !pdf_extractor_available() {
        return Err(anyhow!(
            "pdftotext is not installed, so PDF documents cannot be indexed \
             (macOS: `brew install poppler`, Debian: `apt install poppler-utils`)"
        ));
    }
    let mut command = Command::new("pdftotext");
    command
        .arg("-layout")
        .arg("-enc")
        .arg("UTF-8")
        .arg(path)
        .arg("-")
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = command
        .spawn()
        .map_err(|e| anyhow!("spawning pdftotext: {e}"))?;
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if !status.success() => {
                return Err(anyhow!("pdftotext could not read this file"))
            }
            Ok(Some(_)) => {
                let output = child
                    .wait_with_output()
                    .map_err(|e| anyhow!("reading pdftotext output: {e}"))?;
                return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
            }
            Ok(None) if started.elapsed() >= EXTRACTOR_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(anyhow!("pdftotext exceeded its time budget on this file"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(anyhow!("waiting for pdftotext: {e}")),
        }
    }
}

// ---- small helpers ----------------------------------------------------------

fn byte_index(chars: &[char], char_index: usize) -> usize {
    chars[..char_index].iter().map(|c| c.len_utf8()).sum()
}

fn char_index_after(chars: &[char], byte_index: usize) -> usize {
    let mut bytes = 0usize;
    for (i, c) in chars.iter().enumerate() {
        if bytes >= byte_index {
            return i;
        }
        bytes += c.len_utf8();
    }
    chars.len()
}

/// Skip an element and its content entirely, returning the character index after it.
fn skip_element(lowered: &str, char_index: usize, names: &[&str]) -> Option<usize> {
    let chars: Vec<char> = lowered.chars().collect();
    let start = byte_index(&chars, char_index);
    for name in names {
        let open = format!("<{name}");
        if lowered[start..].starts_with(&open) {
            let close = format!("</{name}>");
            let after = lowered[start..]
                .find(&close)
                .map(|i| start + i + close.len())
                .unwrap_or(lowered.len());
            return Some(char_index_after(&chars, after));
        }
    }
    None
}

fn decode_entities(text: &str) -> String {
    text.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_headings_become_markdown_headings() {
        let html = "<html><body><h1>Approval</h1><p>Corporate orders need approval.</p>\
                    <h2>Exceptions</h2><p>Strategic accounts are exempt.</p></body></html>";
        let md = html_to_markdown(html);
        assert!(md.contains("# Approval"), "{md}");
        assert!(md.contains("## Exceptions"), "{md}");
        assert!(md.contains("Corporate orders need approval."));
    }

    #[test]
    fn script_and_style_content_never_reaches_the_index() {
        let html = "<style>.a{color:red}</style><script>var secret=1;</script><p>Real text</p>";
        let md = html_to_markdown(html);
        assert!(md.contains("Real text"));
        assert!(!md.contains("color"), "{md}");
        assert!(!md.contains("secret"), "{md}");
    }

    #[test]
    fn html_entities_are_decoded() {
        assert!(html_to_markdown("<p>A &amp; B &lt;x&gt;</p>").contains("A & B <x>"));
    }

    #[test]
    fn malformed_html_does_not_panic() {
        for bad in ["<p>unclosed", "<<<>>>", "<h1>", "", "<script>no end"] {
            let _ = html_to_markdown(bad);
        }
    }

    #[test]
    fn word_heading_styles_become_heading_levels() {
        let xml = r#"<?xml version="1.0"?>
<w:document xmlns:w="x"><w:body>
  <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Approval BRD</w:t></w:r></w:p>
  <w:p><w:r><w:t>Corporate orders require approval.</w:t></w:r></w:p>
  <w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:r><w:t>Exceptions</w:t></w:r></w:p>
  <w:p><w:r><w:t>Strategic accounts are exempt.</w:t></w:r></w:p>
</w:body></w:document>"#;
        let md = wordml_to_markdown(xml);
        assert!(md.contains("# Approval BRD"), "{md}");
        assert!(md.contains("## Exceptions"), "{md}");
        assert!(md.contains("Corporate orders require approval."));
    }

    #[test]
    fn a_word_run_split_across_elements_is_joined_into_one_paragraph() {
        // Word splits a sentence across runs at every formatting change; a naive
        // reader produces fragments that match nothing.
        let xml = r#"<w:document xmlns:w="x"><w:body><w:p>
            <w:r><w:t>Strategic </w:t></w:r><w:r><w:t>accounts are </w:t></w:r>
            <w:r><w:t>exempt.</w:t></w:r></w:p></w:body></w:document>"#;
        assert!(wordml_to_markdown(xml).contains("Strategic accounts are exempt."));
    }

    #[test]
    fn heading_style_names_are_parsed_leniently() {
        assert_eq!(heading_from_style("Heading1"), Some(1));
        assert_eq!(heading_from_style("heading 3"), Some(3));
        assert_eq!(heading_from_style("Heading-2"), Some(2));
        assert_eq!(heading_from_style("Normal"), None);
    }

    #[test]
    fn a_docx_that_is_not_a_zip_reports_why() {
        let err = docx_to_markdown(b"not a zip").unwrap_err().to_string();
        assert!(err.contains("readable .docx"), "{err}");
    }

    #[test]
    fn a_missing_pdf_extractor_names_the_tool_and_the_fix() {
        // Only meaningful where pdftotext is genuinely absent; where it is present the
        // call simply fails on a nonexistent path, which is also a named error.
        let err = pdf_to_text(Path::new("/nonexistent/file.pdf"))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("pdftotext"),
            "the failure must name the tool: {err}"
        );
    }
}

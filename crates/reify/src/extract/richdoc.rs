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

/// Extract an OpenDocument text file (`.odt`).
///
/// The same idea as DOCX in a different vocabulary: `text:h` carries an outline level,
/// `text:p` is a paragraph.
pub fn odt_to_markdown(bytes: &[u8]) -> Result<String> {
    let xml = read_zip_entry(bytes, &["content.xml"], ".odt")?;
    Ok(opendocument_to_markdown(&xml))
}

/// Extract a spreadsheet (`.xlsx`).
///
/// Business analysts write requirements in spreadsheets more often than anyone admits:
/// one row per rule, one column for the condition. Cell text lives in a shared string
/// table, so reading that recovers the vocabulary even without the grid.
pub fn xlsx_to_markdown(bytes: &[u8]) -> Result<String> {
    let xml = read_zip_entry(bytes, &["xl/sharedStrings.xml"], ".xlsx")?;
    let strings = element_texts(&xml, "t");
    if strings.is_empty() {
        return Err(anyhow!("the spreadsheet contains no text"));
    }
    // No headings exist, so the sheet becomes one section. That is accurate, rather
    // than inventing a structure the document does not have.
    Ok(strings.join("\n"))
}

/// Extract a presentation (`.pptx`), slide by slide.
pub fn pptx_to_markdown(bytes: &[u8]) -> Result<String> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|e| anyhow!("not a readable .pptx: {e}"))?;
    let mut slides: Vec<String> = archive
        .file_names()
        .filter(|n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml"))
        .map(str::to_string)
        .collect();
    // `slide2` must not sort before `slide10` purely as text.
    slides.sort_by_key(|n| {
        n.trim_start_matches("ppt/slides/slide")
            .trim_end_matches(".xml")
            .parse::<u32>()
            .unwrap_or(u32::MAX)
    });

    let mut out = String::new();
    for (index, name) in slides.iter().enumerate() {
        let mut xml = String::new();
        let read = archive
            .by_name(name)
            .ok()
            .and_then(|mut f| f.read_to_string(&mut xml).ok());
        if read.is_none() {
            continue;
        }
        let texts = element_texts(&xml, "t");
        if texts.is_empty() {
            continue;
        }
        // A slide's first line is its title in practice, so it becomes the heading and
        // the slide becomes a citable section.
        out.push_str(&format!("\n## Slide {}: {}\n\n", index + 1, texts[0]));
        for line in texts.iter().skip(1) {
            out.push_str(line);
            out.push('\n');
        }
    }
    if out.is_empty() {
        return Err(anyhow!("the presentation contains no text"));
    }
    Ok(out)
}

/// Strip RTF control words, leaving the text.
///
/// Hand-rolled rather than a dependency: RTF is control words, braces and text, and
/// only the text is wanted.
pub fn rtf_to_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len() / 2);
    let mut chars = text.chars().peekable();
    let mut skip_group = 0usize;
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                let mut word = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_alphabetic() {
                        word.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                // A numeric parameter belongs to the control word.
                while chars
                    .peek()
                    .is_some_and(|c| c.is_ascii_digit() || *c == '-')
                {
                    chars.next();
                }
                if chars.peek() == Some(&' ') {
                    chars.next();
                }
                match word.as_str() {
                    "par" | "line" | "sect" => out.push('\n'),
                    "tab" => out.push('\t'),
                    // Metadata groups carry no document text.
                    "fonttbl" | "colortbl" | "stylesheet" | "info" | "pict" => skip_group += 1,
                    _ => {}
                }
            }
            '{' => {}
            '}' => skip_group = skip_group.saturating_sub(1),
            c if skip_group == 0 => out.push(c),
            _ => {}
        }
    }
    out.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Read one entry out of a zip-based document format.
fn read_zip_entry(bytes: &[u8], candidates: &[&str], label: &str) -> Result<String> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|e| anyhow!("not a readable {label}: {e}"))?;
    for name in candidates {
        let mut text = String::new();
        if let Ok(mut entry) = archive.by_name(name) {
            entry
                .read_to_string(&mut text)
                .map_err(|e| anyhow!("reading {name}: {e}"))?;
            return Ok(text);
        }
    }
    Err(anyhow!(
        "no {} in the archive; is this really a {label}?",
        candidates.join(" or ")
    ))
}

/// Every text node of a named element, in document order.
fn element_texts(xml: &str, element: &str) -> Vec<String> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut buffer = Vec::new();
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(e)) if local_name(e.name().as_ref()) == element => depth += 1,
            Ok(Event::Text(e)) if depth > 0 => {
                if let Ok(text) = e.decode() {
                    current.push_str(&text);
                }
            }
            Ok(Event::End(e)) if local_name(e.name().as_ref()) == element => {
                depth = depth.saturating_sub(1);
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    out.push(trimmed.to_string());
                }
                current.clear();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buffer.clear();
    }
    out
}

/// Convert OpenDocument text to the Markdown-shaped intermediate.
fn opendocument_to_markdown(xml: &str) -> String {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut buffer = Vec::new();
    let mut out = String::new();
    let mut current = String::new();
    let mut heading_level: Option<usize> = None;
    let mut in_block = false;

    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(e)) => {
                let name = local_name(e.name().as_ref());
                if name == "h" || name == "p" {
                    in_block = true;
                    current.clear();
                    heading_level = if name == "h" {
                        Some(
                            e.attributes()
                                .flatten()
                                .find_map(|a| {
                                    (local_name(a.key.as_ref()) == "outline-level").then(|| {
                                        String::from_utf8_lossy(&a.value).parse().unwrap_or(1)
                                    })
                                })
                                .unwrap_or(1),
                        )
                    } else {
                        None
                    };
                }
            }
            Ok(Event::Text(e)) if in_block => {
                if let Ok(text) = e.decode() {
                    current.push_str(&text);
                }
            }
            Ok(Event::End(e)) => {
                let name = local_name(e.name().as_ref());
                if name == "h" || name == "p" {
                    in_block = false;
                    let trimmed = current.trim();
                    if !trimmed.is_empty() {
                        match heading_level {
                            Some(level) => out.push_str(&format!(
                                "\n{} {trimmed}\n",
                                "#".repeat(level.clamp(1, 6))
                            )),
                            None => {
                                out.push_str(trimmed);
                                out.push('\n');
                            }
                        }
                    }
                    current.clear();
                    heading_level = None;
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buffer.clear();
    }
    out
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

/// The tag or attribute name with any namespace prefix removed.
///
/// Shared with the schema extractor: `w:t` and `hbm:class` are both namespaced, and
/// neither parser cares which namespace an element came from.
pub(crate) fn local_name(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    text.rsplit(':').next().unwrap_or(&text).to_string()
}

fn attribute(event: &quick_xml::events::BytesStart<'_>, wanted: &str) -> Option<String> {
    event.attributes().flatten().find_map(|attr| {
        (local_name(attr.key.as_ref()) == wanted)
            .then(|| String::from_utf8_lossy(&attr.value).to_string())
    })
}

/// An external program that can turn a document into text on stdout.
///
/// Several formats have no usable pure-Rust reader — legacy binary `.doc` above all —
/// and shipping a half-working parser for them would be worse than delegating. Each
/// format lists the converters it accepts, in preference order, so Reify works with
/// whichever one a machine happens to have.
struct Converter {
    program: &'static str,
    /// Arguments, where `{}` is replaced by the document path.
    args: &'static [&'static str],
    /// How to install it, named in the error so a user is never simply stuck.
    hint: &'static str,
}

const PDF_CONVERTERS: &[Converter] = &[
    Converter {
        program: "pdftotext",
        args: &["-layout", "-enc", "UTF-8", "{}", "-"],
        hint: "macOS: `brew install poppler`, Debian: `apt install poppler-utils`",
    },
    Converter {
        program: "mutool",
        args: &["draw", "-F", "txt", "{}"],
        hint: "macOS: `brew install mupdf`, Debian: `apt install mupdf-tools`",
    },
];

const DOC_CONVERTERS: &[Converter] = &[
    Converter {
        program: "antiword",
        args: &["{}"],
        hint: "Debian: `apt install antiword`",
    },
    Converter {
        program: "textutil",
        args: &["-convert", "txt", "-stdout", "{}"],
        hint: "built in on macOS",
    },
    Converter {
        program: "soffice",
        args: &["--headless", "--cat", "{}"],
        hint: "install LibreOffice",
    },
];

/// Converters for `.rtf`, used only if the built-in reader finds nothing.
const RTF_CONVERTERS: &[Converter] = &[Converter {
    program: "textutil",
    args: &["-convert", "txt", "-stdout", "{}"],
    hint: "built in on macOS",
}];

/// Every external program Reify may spawn to extract document text.
///
/// Exposed so the offline test can assert on the actual list rather than on a source
/// grep: adding a converter changes this function, and the test fails until the new
/// program is reviewed and allowed.
pub fn external_tools() -> Vec<&'static str> {
    let mut tools: Vec<&'static str> = PDF_CONVERTERS
        .iter()
        .chain(DOC_CONVERTERS)
        .chain(RTF_CONVERTERS)
        .map(|c| c.program)
        .collect();
    tools.sort_unstable();
    tools.dedup();
    tools
}

/// Is any converter for this format installed?
fn any_available(converters: &[Converter]) -> bool {
    converters.iter().any(|c| {
        Command::new(c.program)
            .arg("--help")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
    })
}

/// Whether a PDF text extractor is available on this machine.
pub fn pdf_extractor_available() -> bool {
    any_available(PDF_CONVERTERS)
}

/// Run the first converter that works, or explain precisely what is missing.
fn convert(converters: &[Converter], path: &Path, format: &str) -> Result<String> {
    let mut attempted = Vec::new();
    for converter in converters {
        attempted.push(format!("{} ({})", converter.program, converter.hint));
        let args: Vec<String> = converter
            .args
            .iter()
            .map(|a| a.replace("{}", &path.to_string_lossy()))
            .collect();
        let mut command = Command::new(converter.program);
        command
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let Ok(mut child) = command.spawn() else {
            continue; // not installed
        };
        let started = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) if !status.success() => break,
                Ok(Some(_)) => {
                    let output = child
                        .wait_with_output()
                        .map_err(|e| anyhow!("reading {}: {e}", converter.program))?;
                    let text = String::from_utf8_lossy(&output.stdout).into_owned();
                    if !text.trim().is_empty() {
                        return Ok(text);
                    }
                    break;
                }
                Ok(None) if started.elapsed() >= EXTRACTOR_TIMEOUT => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(_) => break,
            }
        }
    }
    Err(anyhow!(
        "no working {format} text extractor, so these documents cannot be indexed. \
         Install one of: {}",
        attempted.join("; ")
    ))
}

/// Extract a PDF's text, or explain precisely why it could not be done.
///
/// The error names the missing tools and how to install them, because a user whose
/// requirements are all PDFs needs to know that Reify indexed none of them.
pub fn pdf_to_text(path: &Path) -> Result<String> {
    convert(PDF_CONVERTERS, path, "PDF")
}

/// Extract a legacy binary Word document (`.doc`).
///
/// There is no usable pure-Rust reader for the format, and business requirements
/// written in 2011 are still `.doc`, so delegation is the only honest option.
pub fn doc_to_text(path: &Path) -> Result<String> {
    convert(DOC_CONVERTERS, path, "legacy .doc")
}

/// Extract RTF, preferring the built-in reader and falling back to a converter.
pub fn rtf_to_markdown(path: &Path, text: &str) -> Result<String> {
    let stripped = rtf_to_text(text);
    if stripped.split_whitespace().count() >= 8 {
        return Ok(stripped);
    }
    convert(RTF_CONVERTERS, path, "RTF")
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
    fn a_failed_conversion_names_every_tool_it_tried_and_how_to_install_them() {
        // A user whose requirements are all PDFs must learn that none were indexed,
        // and what to do about it — not silently get an empty index.
        let err = pdf_to_text(Path::new("/nonexistent/file.pdf"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("pdftotext"), "{err}");
        assert!(
            err.contains("mutool"),
            "every candidate must be listed: {err}"
        );
        assert!(err.contains("brew install"), "and how to get one: {err}");

        let err = doc_to_text(Path::new("/nonexistent/file.doc"))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("antiword") && err.contains("textutil"),
            "{err}"
        );
    }

    #[test]
    fn opendocument_headings_carry_their_outline_level() {
        let xml = r#"<office:document xmlns:office="o" xmlns:text="t">
          <office:body><office:text>
            <text:h text:outline-level="1">Approval BRD</text:h>
            <text:p>Corporate orders require approval.</text:p>
            <text:h text:outline-level="2">Exceptions</text:h>
            <text:p>Strategic accounts are exempt.</text:p>
          </office:text></office:body></office:document>"#;
        let md = opendocument_to_markdown(xml);
        assert!(md.contains("# Approval BRD"), "{md}");
        assert!(md.contains("## Exceptions"), "{md}");
        assert!(md.contains("Corporate orders require approval."));
    }

    #[test]
    fn rtf_control_words_and_metadata_groups_are_stripped() {
        let rtf = r"{\rtf1\ansi{\fonttbl{\f0 Times;}}\f0\fs24 Corporate orders                     require approval.\par Strategic accounts are exempt.\par}";
        let text = rtf_to_text(rtf);
        assert!(text.contains("Corporate orders"), "{text}");
        assert!(text.contains("Strategic accounts are exempt."), "{text}");
        assert!(
            !text.contains("fonttbl"),
            "metadata must not survive: {text}"
        );
        assert!(
            !text.contains("fs24"),
            "control words must not survive: {text}"
        );
    }

    #[test]
    fn a_spreadsheet_yields_its_shared_strings() {
        // Requirements written as one row per rule are common and otherwise invisible.
        let xml = r#"<sst xmlns="x"><si><t>Corporate orders</t></si>                     <si><t>require L2 approval</t></si></sst>"#;
        assert_eq!(
            element_texts(xml, "t"),
            vec!["Corporate orders", "require L2 approval"]
        );
    }

    #[test]
    fn a_zip_that_is_not_the_expected_format_says_which_entry_is_missing() {
        let err = read_zip_entry(b"not a zip", &["content.xml"], ".odt")
            .unwrap_err()
            .to_string();
        assert!(err.contains(".odt"), "{err}");
    }
}

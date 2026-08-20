//! Document extraction: Markdown and plain text into addressable sections.
//!
//! Sections rather than whole documents are the unit of evidence. "BRD-42 §4.2" is a
//! citation an engineer can check in seconds; "BRD-42" is not.

use anyhow::Result;
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

use super::FileExtract;
use crate::model::{uid, Lang, NewNode, NodeKind, Status};

/// Sections shorter than this carry no retrievable content and are folded into the
/// document node instead of becoming their own citable unit.
const MIN_SECTION_CHARS: usize = 24;

/// One heading and the prose beneath it.
#[derive(Debug, Clone)]
pub struct Section {
    /// Dotted heading path, e.g. `approval.thresholds`.
    pub slug: String,
    pub title: String,
    pub level: u8,
    pub line: u32,
    pub body: String,
    /// ISO-639-1 code detected for this section's prose, if confident.
    pub lang: Option<String>,
}

/// Extract a document's sections and stage them as nodes.
pub fn extract(path: &str, text: &str, lang: Lang) -> Result<FileExtract> {
    let sections = match lang {
        Lang::Markdown => split_markdown(text),
        _ => split_plain(text),
    };

    let mut out = FileExtract::default();
    let title = document_title(path, &sections);

    for section in &sections {
        if section.body.chars().count() < MIN_SECTION_CHARS && section.title.is_empty() {
            continue;
        }
        let section_uid = uid::doc_section(path, &section.slug);
        let display = if section.title.is_empty() {
            title.clone()
        } else {
            section.title.clone()
        };
        let mut search = String::with_capacity(section.body.len() + 64);
        search.push_str(&section.title);
        search.push(' ');
        search.push_str(&section.body);

        out.vocabulary
            .extend(words(&section.title).into_iter().filter(|w| w.len() > 2));

        out.batch.node(
            NewNode::new(&section_uid, NodeKind::DocSection, display)
                .at(path, section.line, section.line)
                .lang(lang)
                .status(Status::Confirmed, 1.0)
                .search(search)
                .data(serde_json::json!({
                    "slug": section.slug,
                    "level": section.level,
                    "lang": section.lang,
                    "document": title,
                    "excerpt": excerpt(&section.body),
                    "summary": format!("{title} §{}", section.slug),
                })),
        );
    }
    Ok(out)
}

/// A readable document title: the first level-1 heading, else the file stem.
fn document_title(path: &str, sections: &[Section]) -> String {
    sections
        .iter()
        .find(|s| s.level == 1 && !s.title.is_empty())
        .map(|s| s.title.clone())
        .unwrap_or_else(|| {
            path.rsplit('/')
                .next()
                .unwrap_or(path)
                .rsplit_once('.')
                .map(|(stem, _)| stem.to_string())
                .unwrap_or_else(|| path.to_string())
        })
}

fn excerpt(body: &str) -> String {
    let normalized = body.split_whitespace().collect::<Vec<_>>().join(" ");
    normalized.chars().take(280).collect()
}

/// Split Markdown into sections at every heading.
pub fn split_markdown(text: &str) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut current = Section {
        slug: "_preamble".into(),
        title: String::new(),
        level: 0,
        line: 1,
        body: String::new(),
        lang: None,
    };

    let mut in_heading: Option<u8> = None;
    let mut heading_text = String::new();
    let parser = Parser::new(text).into_offset_iter();

    for (event, range) in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                in_heading = Some(heading_number(level));
                heading_text.clear();
            }
            Event::End(TagEnd::Heading(_)) => {
                let level = in_heading.take().unwrap_or(1);
                finish(&mut sections, std::mem::replace(&mut current, Section {
                    slug: String::new(),
                    title: heading_text.trim().to_string(),
                    level,
                    line: line_of(text, range.start),
                    body: String::new(),
                    lang: None,
                }));
                stack.truncate(level.saturating_sub(1) as usize);
                stack.push(slugify(&heading_text));
                current.slug = stack.join(".");
            }
            Event::Text(t) | Event::Code(t) => {
                if in_heading.is_some() {
                    heading_text.push_str(&t);
                } else {
                    current.body.push_str(&t);
                    current.body.push(' ');
                }
            }
            Event::SoftBreak | Event::HardBreak => current.body.push(' '),
            _ => {}
        }
    }
    finish(&mut sections, current);
    sections
}

/// Plain text has no headings, so the whole file is one section.
fn split_plain(text: &str) -> Vec<Section> {
    let body = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if body.is_empty() {
        return Vec::new();
    }
    vec![Section {
        slug: "_document".into(),
        title: String::new(),
        level: 0,
        line: 1,
        body: body.clone(),
        lang: detect_language(&body),
    }]
}

fn finish(sections: &mut Vec<Section>, mut section: Section) {
    section.body = section.body.split_whitespace().collect::<Vec<_>>().join(" ");
    if section.body.is_empty() && section.title.is_empty() {
        return;
    }
    if section.slug.is_empty() {
        section.slug = slugify(&section.title);
    }
    section.lang = detect_language(&format!("{} {}", section.title, section.body));
    sections.push(section);
}

/// Detect the prose language of a section.
///
/// Detection is per section rather than per document because mixed-language business
/// documents are the norm in the codebases Reify targets, not the exception.
pub fn detect_language(text: &str) -> Option<String> {
    if text.trim().chars().count() < 12 {
        return None;
    }
    whatlang::detect(text).and_then(|info| {
        info.is_reliable()
            .then(|| info.lang().code().chars().take(3).collect::<String>())
    })
}

fn heading_number(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn line_of(text: &str, offset: usize) -> u32 {
    text[..offset.min(text.len())].lines().count().max(1) as u32
}

/// A heading turned into a stable, url-safe slug component.
pub fn slugify(text: &str) -> String {
    let mut out = String::new();
    let mut last_dash = true;
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "section".into()
    } else {
        trimmed
    }
}

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MD: &str = r#"# Order Approval BRD

Background text for the document.

## Approval thresholds

Corporate orders above 50M VND require L2 approval.

### Exceptions

Strategic accounts are exempt from L2 approval.

## Pricing

Discounts stack additively.
"#;

    #[test]
    fn markdown_splits_at_every_heading() {
        let sections = split_markdown(MD);
        let titles: Vec<&str> = sections.iter().map(|s| s.title.as_str()).collect();
        assert!(titles.contains(&"Order Approval BRD"));
        assert!(titles.contains(&"Approval thresholds"));
        assert!(titles.contains(&"Exceptions"));
        assert!(titles.contains(&"Pricing"));
    }

    #[test]
    fn nested_headings_produce_a_dotted_citable_path() {
        let sections = split_markdown(MD);
        let exceptions = sections.iter().find(|s| s.title == "Exceptions").unwrap();
        assert_eq!(exceptions.slug, "order-approval-brd.approval-thresholds.exceptions");
        assert_eq!(exceptions.level, 3);
    }

    #[test]
    fn a_sibling_heading_pops_back_to_the_right_level() {
        let sections = split_markdown(MD);
        let pricing = sections.iter().find(|s| s.title == "Pricing").unwrap();
        assert_eq!(pricing.slug, "order-approval-brd.pricing");
    }

    #[test]
    fn section_bodies_carry_the_prose_beneath_the_heading() {
        let sections = split_markdown(MD);
        let thresholds = sections
            .iter()
            .find(|s| s.title == "Approval thresholds")
            .unwrap();
        assert!(thresholds.body.contains("50M VND"));
        assert!(!thresholds.body.contains("Strategic accounts"), "must not swallow the child section");
    }

    #[test]
    fn heading_line_numbers_point_at_the_heading() {
        let sections = split_markdown(MD);
        let thresholds = sections
            .iter()
            .find(|s| s.title == "Approval thresholds")
            .unwrap();
        assert!(thresholds.line >= 4 && thresholds.line <= 6, "got {}", thresholds.line);
    }

    #[test]
    fn vietnamese_prose_is_detected_as_vietnamese() {
        let text = "Khách hàng doanh nghiệp thuộc nhóm chiến lược được miễn phê duyệt cấp hai theo quy định.";
        assert_eq!(detect_language(text).as_deref(), Some("vie"));
    }

    #[test]
    fn english_prose_is_detected_as_english() {
        let text = "Strategic accounts are exempt from the level two approval requirement.";
        assert_eq!(detect_language(text).as_deref(), Some("eng"));
    }

    #[test]
    fn very_short_text_yields_no_language_guess() {
        assert_eq!(detect_language("ok"), None);
    }

    #[test]
    fn slugs_are_stable_and_url_safe() {
        assert_eq!(slugify("Approval Thresholds!"), "approval-thresholds");
        assert_eq!(slugify("§4.2 — Rules"), "4-2-rules");
        assert_eq!(slugify("!!!"), "section");
    }

    #[test]
    fn extraction_stages_one_node_per_meaningful_section() {
        let fx = extract("docs/BRD-42.md", MD, Lang::Markdown).unwrap();
        assert!(fx.batch.nodes.len() >= 4);
        let node = fx
            .batch
            .nodes
            .iter()
            .find(|n| n.name == "Exceptions")
            .unwrap();
        assert_eq!(node.uid, "doc:docs/BRD-42.md#order-approval-brd.approval-thresholds.exceptions");
        assert_eq!(node.data["document"], "Order Approval BRD");
        assert!(node.data["excerpt"].as_str().unwrap().contains("Strategic"));
    }

    #[test]
    fn plain_text_becomes_a_single_document_section() {
        let fx = extract("notes.txt", "Corporate orders need approval above the limit.", Lang::Text).unwrap();
        assert_eq!(fx.batch.nodes.len(), 1);
        assert_eq!(fx.batch.nodes[0].data["slug"], "_document");
    }

    #[test]
    fn an_empty_document_produces_nothing_rather_than_an_empty_node() {
        let fx = extract("empty.md", "\n\n", Lang::Markdown).unwrap();
        assert!(fx.batch.nodes.is_empty());
    }
}

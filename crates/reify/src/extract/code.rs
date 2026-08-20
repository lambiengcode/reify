//! Source-code extraction with tree-sitter.
//!
//! tree-sitter is chosen over per-language compilers because it is error tolerant —
//! mature repositories contain files that do not fully parse — and because adding a
//! language becomes "add a grammar", not "write a front end". See `docs/PLAN.md` §H.2.
//!
//! Call resolution here is a *heuristic*, and says so in the data: every produced edge
//! carries a confidence derived from how many candidates the name matched.

use anyhow::{anyhow, Result};
use std::collections::{HashMap, HashSet};
use tree_sitter::{Node as TsNode, Parser};

use super::{FileExtract, PendingRef};
use crate::model::{uid, EdgeKind, Lang, NewEdge, NewNode, NodeKind, Status};
use crate::store::Batch;

/// A name matching more candidates than this is treated as unresolvable.
///
/// Emitting twenty low-confidence edges for a name like `get` pollutes the graph and
/// costs the context compiler far more than the edges are worth.
const MAX_CALL_CANDIDATES: usize = 5;

/// Names shorter than this only resolve inside their own file.
///
/// `_` is the translation helper in most codebases and appears in every file of every
/// language; resolving it globally invents thousands of edges that mean nothing.
const MIN_CROSS_FILE_NAME_LEN: usize = 3;

/// Extract symbols, calls, imports and inheritance from one source file.
pub fn extract(path: &str, text: &str, lang: Lang) -> Result<FileExtract> {
    let mut parser = Parser::new();
    let language = match lang {
        Lang::Python => tree_sitter_python::LANGUAGE.into(),
        Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        Lang::JavaScript => tree_sitter_typescript::LANGUAGE_TSX.into(),
        Lang::Java => tree_sitter_java::LANGUAGE.into(),
        other => return Err(anyhow!("no code grammar for {}", other.as_str())),
    };
    parser
        .set_language(&language)
        .map_err(|e| anyhow!("loading grammar for {}: {e}", lang.as_str()))?;
    let tree = parser
        .parse(text, None)
        .ok_or_else(|| anyhow!("tree-sitter returned no tree for {path}"))?;

    let mut ctx = Ctx {
        path,
        text,
        lang,
        out: FileExtract::default(),
        scope: Vec::new(),
        enclosing: Vec::new(),
        used_names: HashSet::new(),
        consumed: Vec::new(),
    };
    ctx.walk(tree.root_node());
    Ok(ctx.out)
}

struct Ctx<'a> {
    path: &'a str,
    text: &'a str,
    lang: Lang,
    out: FileExtract,
    /// Enclosing declaration names, forming the qualified name.
    scope: Vec<String>,
    /// Uids of the enclosing declarations, for attributing references.
    enclosing: Vec<String>,
    /// Qualified names already emitted in this file, so redefinitions stay addressable.
    used_names: HashSet<String>,
    /// Line spans already claimed by a nested declaration.
    ///
    /// A guard belongs to the innermost declaration containing it. Without this, a
    /// class and each of its methods would all mine the same `if` and the repository
    /// would appear to state one rule several times.
    consumed: Vec<(u32, u32)>,
}

impl<'a> Ctx<'a> {
    fn slice(&self, node: TsNode<'_>) -> &'a str {
        &self.text[node.byte_range()]
    }

    fn named_child_text(&self, node: TsNode<'_>, field: &str) -> Option<&'a str> {
        node.child_by_field_name(field).map(|c| self.slice(c))
    }

    fn walk(&mut self, node: TsNode<'_>) {
        if let Some(decl) = self.declaration(node) {
            self.enter_declaration(node, decl);
            return;
        }
        match node.kind() {
            "call" | "call_expression" | "method_invocation" => self.record_call(node),
            "import_statement" | "import_from_statement" | "import_declaration" => {
                self.record_import(node)
            }
            _ => {}
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk(child);
        }
    }

    /// Recognise a declaration node and return its kind label and name.
    fn declaration(&self, node: TsNode<'_>) -> Option<(&'static str, String)> {
        let kind = match node.kind() {
            "function_definition" | "function_declaration" | "generator_function_declaration" => {
                "function"
            }
            "class_definition" | "class_declaration" | "abstract_class_declaration" => "class",
            "method_definition" => "method",
            "interface_declaration" => "interface",
            "type_alias_declaration" => "type",
            "enum_declaration" => "enum",
            _ => return None,
        };
        let name = self.named_child_text(node, "name")?.to_string();
        Some((kind, name))
    }

    fn enter_declaration(&mut self, node: TsNode<'_>, (kind, name): (&'static str, String)) {
        let mut qualified = if self.scope.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", self.scope.join("."), name)
        };
        // Two declarations may legitimately share a qualified name (conditional
        // definitions, overloads). Uids must stay unique or edges collapse onto one.
        let mut suffix = 2;
        while !self.used_names.insert(qualified.clone()) {
            qualified = format!(
                "{}~{suffix}",
                qualified.trim_end_matches(|c: char| c.is_ascii_digit() || c == '~')
            );
            suffix += 1;
        }

        let start = node.start_position().row as u32 + 1;
        let end = node.end_position().row as u32 + 1;
        let symbol_uid = uid::symbol(self.path, &qualified);
        let signature = self.signature(node);
        let doc = self.leading_doc(node);

        let mut search = String::new();
        search.push_str(&split_identifier(&name).join(" "));
        search.push(' ');
        search.push_str(&name);
        if let Some(d) = &doc {
            search.push(' ');
            search.push_str(d);
        }

        self.out.vocabulary.extend(split_identifier(&name));

        self.out.batch.node(
            NewNode::new(&symbol_uid, NodeKind::Symbol, &name)
                .at(self.path, start, end)
                .lang(self.lang)
                .status(Status::Confirmed, 1.0)
                .search(search)
                .data(serde_json::json!({
                    "qualified": qualified,
                    "symbol_kind": kind,
                    "signature": signature,
                    "doc": doc,
                    "summary": format!("{kind} {qualified}"),
                })),
        );

        if let Some(parent) = self.enclosing.last() {
            self.out.batch.edge(NewEdge::new(
                parent.clone(),
                symbol_uid.clone(),
                EdgeKind::Calls,
                Status::Confirmed,
                0.5,
            ));
        }

        self.record_inheritance(node, &symbol_uid);

        // Descend first, so nested declarations can claim their own line spans.
        let mark = self.consumed.len();
        self.scope.push(name.clone());
        self.enclosing.push(symbol_uid.clone());
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk(child);
        }
        self.enclosing.pop();
        self.scope.pop();

        // Mine rule candidates from what is left: the declaration's own name (a test
        // name is an executable rule), its documentation, and the guards in its body
        // that no nested declaration already claimed.
        let nested: Vec<(u32, u32)> = self.consumed[mark..].to_vec();
        let body = self.body_without_nested(node, start, &nested);
        self.out.rules.extend(crate::rules::from_symbol(
            &name,
            &qualified,
            doc.as_deref(),
            &body,
            &format!("{}:{start}", self.path),
            &symbol_uid,
        ));
        self.consumed.push((start, end));
    }

    /// The declaration's text with every nested declaration's lines blanked out.
    fn body_without_nested(&self, node: TsNode<'_>, start: u32, nested: &[(u32, u32)]) -> String {
        if nested.is_empty() {
            return self.slice(node).to_string();
        }
        self.slice(node)
            .lines()
            .enumerate()
            .map(|(offset, line)| {
                let absolute = start + offset as u32;
                if nested.iter().any(|(s, e)| *s <= absolute && absolute <= *e) {
                    ""
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The declaration's first line, trimmed — enough for an agent to judge relevance
    /// without opening the file.
    fn signature(&self, node: TsNode<'_>) -> String {
        let text = self.slice(node);
        let first = text.lines().next().unwrap_or("").trim();
        let capped: String = first.chars().take(160).collect();
        capped
    }

    /// Docstring (Python) or preceding line comments (TypeScript/JavaScript).
    fn leading_doc(&self, node: TsNode<'_>) -> Option<String> {
        if self.lang == Lang::Python {
            let body = node.child_by_field_name("body")?;
            let first = body.named_child(0)?;
            if first.kind() == "expression_statement" {
                let inner = first.named_child(0)?;
                if inner.kind() == "string" {
                    let raw = self.slice(inner);
                    return Some(clean_docstring(raw));
                }
            }
            return None;
        }
        // A comment above `export class X` is a sibling of the export wrapper, not of
        // the class, so climb out of the wrapper before looking backwards.
        let mut anchor = node;
        while anchor
            .parent()
            .is_some_and(|p| matches!(p.kind(), "export_statement" | "ambient_declaration"))
        {
            anchor = anchor.parent().expect("checked by the loop condition");
        }
        let mut prev = anchor.prev_named_sibling()?;
        if !matches!(prev.kind(), "comment" | "block_comment" | "line_comment") {
            return None;
        }
        let mut parts = vec![clean_comment(self.slice(prev))];
        while let Some(p) = prev.prev_named_sibling() {
            if !matches!(p.kind(), "comment" | "block_comment" | "line_comment") {
                break;
            }
            parts.push(clean_comment(self.slice(p)));
            prev = p;
        }
        parts.reverse();
        let joined = parts.join(" ").trim().to_string();
        (!joined.is_empty()).then_some(joined)
    }

    fn record_inheritance(&mut self, node: TsNode<'_>, from: &str) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            let bases: Vec<&str> = match child.kind() {
                // Python: `class A(Base, Mixin):`
                "argument_list" => child
                    .named_children(&mut child.walk())
                    .filter(|c| c.kind() == "identifier" || c.kind() == "attribute")
                    .map(|c| self.slice(c))
                    .collect(),
                // Java: `class A extends Base implements I`
                "superclass" | "super_interfaces" | "extends_interfaces" => child
                    .named_children(&mut child.walk())
                    .flat_map(|c| {
                        if c.kind() == "type_identifier" {
                            vec![c]
                        } else {
                            c.named_children(&mut c.walk()).collect()
                        }
                    })
                    .filter(|c| c.kind() == "type_identifier")
                    .map(|c| self.slice(c))
                    .collect(),
                // TypeScript: `class A extends Base implements I`
                "class_heritage" => child
                    .named_children(&mut child.walk())
                    .flat_map(|c| c.named_children(&mut c.walk()).collect::<Vec<_>>())
                    .filter(|c| c.kind() == "identifier" || c.kind() == "type_identifier")
                    .map(|c| self.slice(c))
                    .collect(),
                _ => continue,
            };
            for base in bases {
                let bare = base.rsplit('.').next().unwrap_or(base);
                self.out.pending.push(PendingRef {
                    from: from.to_string(),
                    name: bare.to_string(),
                    file: self.path.to_string(),
                    kind: EdgeKind::Inherits,
                });
            }
        }
    }

    fn record_call(&mut self, node: TsNode<'_>) {
        let Some(from) = self.enclosing.last().cloned() else {
            return; // a call at module level belongs to no symbol
        };
        // Java names the callee in `name`; the JS and Python grammars use `function`.
        let Some(func) = node
            .child_by_field_name("function")
            .or_else(|| node.child_by_field_name("name"))
        else {
            return;
        };
        let name = match func.kind() {
            "identifier" if node.kind() == "method_invocation" => self.slice(func).to_string(),
            "identifier" => self.slice(func).to_string(),
            "attribute" | "member_expression" => match self
                .named_child_text(func, "property")
                .or_else(|| self.named_child_text(func, "attribute"))
            {
                Some(p) => p.to_string(),
                None => return,
            },
            _ => return,
        };
        if name.is_empty() {
            return;
        }
        self.out.vocabulary.extend(split_identifier(&name));
        self.out.pending.push(PendingRef {
            from,
            name,
            file: self.path.to_string(),
            kind: EdgeKind::Calls,
        });
    }

    fn record_import(&mut self, node: TsNode<'_>) {
        let text = self.slice(node);
        for module in parse_import_modules(text) {
            self.out.imports.push(module);
        }
    }
}

/// Pull module specifiers out of an import statement's source text.
///
/// Text-level parsing is used rather than per-grammar field access because the two
/// grammars name these fields differently and the payoff of exactness here is small.
fn parse_import_modules(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(rest) = text.strip_prefix("from ") {
        if let Some(module) = rest.split_whitespace().next() {
            out.push(module.trim_matches(|c| c == '"' || c == '\'').to_string());
        }
    } else if let Some(rest) = text.strip_prefix("import ") {
        for part in rest.split(',') {
            let name = part.split_whitespace().next().unwrap_or("");
            if !name.is_empty() && name != "{" {
                out.push(name.trim_matches(|c| c == '"' || c == '\'').to_string());
            }
        }
    }
    // ES imports: `import x from "mod"` / `import "mod"`
    if let Some(idx) = text.find(" from ") {
        let tail = text[idx + 6..].trim();
        let module = tail.trim_matches(|c| c == '"' || c == '\'' || c == ';');
        if !module.is_empty() {
            out.push(module.to_string());
        }
    }
    out.retain(|m| !m.is_empty());
    out.sort();
    out.dedup();
    out
}

fn clean_docstring(raw: &str) -> String {
    let trimmed = raw
        .trim_start_matches(['r', 'b', 'f', 'u'])
        .trim_matches('"')
        .trim_matches('\'');
    normalize_ws(trimmed).chars().take(400).collect()
}

fn clean_comment(raw: &str) -> String {
    let t = raw
        .trim_start_matches("//")
        .trim_start_matches("/**")
        .trim_start_matches("/*")
        .trim_end_matches("*/")
        .replace("\n *", " ")
        .replace('*', " ");
    normalize_ws(&t)
}

fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Split an identifier into lowercase words.
///
/// This is the first bridge from code vocabulary to business vocabulary: without it,
/// `bypassLevelTwoApproval` never matches a document that says "approval".
pub fn split_identifier(name: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = name.chars().collect();
    for (i, &ch) in chars.iter().enumerate() {
        if ch == '_' || ch == '-' || ch == '.' || ch == ' ' {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            continue;
        }
        let prev = if i > 0 { chars[i - 1] } else { '\0' };
        let next = chars.get(i + 1).copied().unwrap_or('\0');
        let starts_word = ch.is_uppercase()
            && (prev.is_lowercase()
                || prev.is_ascii_digit()
                || (prev.is_uppercase() && next.is_lowercase()));
        let digit_boundary = ch.is_ascii_digit() != prev.is_ascii_digit() && prev != '\0';
        if (starts_word || digit_boundary) && !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
        current.push(ch.to_ascii_lowercase());
    }
    if !current.is_empty() {
        words.push(current);
    }
    words.retain(|w| w.len() > 1);
    words
}

/// Resolve pending references against the repository-wide symbol index.
///
/// Preference order is local file, then imported module path, then a globally unique
/// name. Ambiguity is expressed as reduced confidence rather than hidden by a guess.
pub fn resolve(pending: &[PendingRef], symbols: &SymbolIndex) -> Batch {
    let mut batch = Batch::default();
    for r in pending {
        let Some(all) = symbols.by_name.get(&r.name) else {
            continue;
        };
        let source_lang = symbols.lang_of(&r.from);
        let candidates: Vec<&String> = all
            .iter()
            .filter(|uid| match (source_lang, symbols.lang_of(uid)) {
                (Some(a), Some(b)) => SymbolIndex::callable_across(a, b),
                _ => true,
            })
            .collect();
        if candidates.is_empty() {
            continue;
        }
        let local: Vec<&String> = candidates
            .iter()
            .copied()
            .filter(|uid| symbols.file_of(uid) == Some(r.file.as_str()))
            .collect();
        let (chosen, confidence): (Vec<&String>, f32) = if local.len() == 1 {
            (local, 0.95)
        } else if r.name.chars().count() < MIN_CROSS_FILE_NAME_LEN {
            continue; // too generic to resolve outside its own file
        } else if candidates.len() == 1 {
            (candidates, 0.9)
        } else if candidates.len() <= MAX_CALL_CANDIDATES {
            let n = candidates.len() as f32;
            (candidates, 1.0 / n)
        } else {
            continue; // too ambiguous to be worth an edge
        };
        for target in chosen {
            if target == &r.from {
                continue;
            }
            batch.edge(NewEdge::new(
                r.from.clone(),
                target.clone(),
                r.kind,
                Status::Observed,
                confidence,
            ));
        }
    }
    batch
}

/// Repository-wide map from bare symbol name to the uids that declare it.
#[derive(Debug, Default)]
pub struct SymbolIndex {
    pub by_name: HashMap<String, Vec<String>>,
    /// `uid -> (path, language)`. Language is kept because a call never crosses a
    /// language boundary, and matching on name alone happily claims that a Python
    /// function calls a TypeScript one.
    location_by_uid: HashMap<String, (String, Lang)>,
}

impl SymbolIndex {
    pub fn add(&mut self, name: &str, uid: &str, path: &str, lang: Lang) {
        self.by_name
            .entry(name.to_string())
            .or_default()
            .push(uid.to_string());
        self.location_by_uid
            .insert(uid.to_string(), (path.to_string(), lang));
    }

    fn file_of(&self, uid: &str) -> Option<&str> {
        self.location_by_uid.get(uid).map(|(path, _)| path.as_str())
    }

    fn lang_of(&self, uid: &str) -> Option<Lang> {
        self.location_by_uid.get(uid).map(|(_, lang)| *lang)
    }

    /// Do these two languages share a call graph?
    ///
    /// TypeScript and JavaScript do. Nothing else in the supported set does.
    fn callable_across(a: Lang, b: Lang) -> bool {
        a == b
            || matches!(
                (a, b),
                (Lang::TypeScript, Lang::JavaScript) | (Lang::JavaScript, Lang::TypeScript)
            )
    }

    pub fn len(&self) -> usize {
        self.location_by_uid.len()
    }

    pub fn is_empty(&self) -> bool {
        self.location_by_uid.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PY: &str = r#"
class OrderService:
    """Handles order approval."""

    def requires_approval(self, order):
        if order.customer_group == 7:
            return self.bypass_level_two(order)
        return True

    def bypass_level_two(self, order):
        return False

def standalone(x):
    return x
"#;

    const TS: &str = r#"
// Applies the discount policy.
export class DiscountPolicy extends BasePolicy {
  apply(order: Order): number {
    return this.rate(order);
  }
  rate(order: Order): number { return 0.15; }
}

export function helper(a: number) { return a; }
"#;

    #[test]
    fn python_symbols_carry_qualified_names_and_spans() {
        let fx = extract("order.py", PY, Lang::Python).unwrap();
        let names: Vec<&str> = fx.batch.nodes.iter().map(|n| n.name.as_str()).collect();
        assert!(names.contains(&"OrderService"));
        assert!(names.contains(&"requires_approval"));
        assert!(names.contains(&"standalone"));

        let method = fx
            .batch
            .nodes
            .iter()
            .find(|n| n.name == "requires_approval")
            .unwrap();
        assert_eq!(method.data["qualified"], "OrderService.requires_approval");
        assert_eq!(method.data["symbol_kind"], "function");
        assert!(method.line_start > 0 && method.line_end >= method.line_start);
    }

    #[test]
    fn python_docstrings_are_captured_as_symbol_documentation() {
        let fx = extract("order.py", PY, Lang::Python).unwrap();
        let class = fx
            .batch
            .nodes
            .iter()
            .find(|n| n.name == "OrderService")
            .unwrap();
        assert_eq!(class.data["doc"], "Handles order approval.");
    }

    #[test]
    fn typescript_leading_comments_become_documentation() {
        let fx = extract("policy.ts", TS, Lang::TypeScript).unwrap();
        let class = fx
            .batch
            .nodes
            .iter()
            .find(|n| n.name == "DiscountPolicy")
            .unwrap();
        assert_eq!(class.data["doc"], "Applies the discount policy.");
    }

    #[test]
    fn calls_are_recorded_as_pending_references() {
        let fx = extract("order.py", PY, Lang::Python).unwrap();
        let called: Vec<&str> = fx
            .pending
            .iter()
            .filter(|p| p.kind == EdgeKind::Calls)
            .map(|p| p.name.as_str())
            .collect();
        assert!(called.contains(&"bypass_level_two"));
    }

    #[test]
    fn typescript_inheritance_is_recorded() {
        let fx = extract("policy.ts", TS, Lang::TypeScript).unwrap();
        let inherits: Vec<&str> = fx
            .pending
            .iter()
            .filter(|p| p.kind == EdgeKind::Inherits)
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(inherits, vec!["BasePolicy"]);
    }

    #[test]
    fn resolution_prefers_a_local_definition_over_a_remote_one() {
        let mut idx = SymbolIndex::default();
        idx.add("apply", "sym:a.ts#A.apply", "a.ts", Lang::TypeScript);
        idx.add("apply", "sym:b.ts#B.apply", "b.ts", Lang::TypeScript);
        let pending = vec![PendingRef {
            from: "sym:a.ts#A.run".into(),
            name: "apply".into(),
            file: "a.ts".into(),
            kind: EdgeKind::Calls,
        }];
        let batch = resolve(&pending, &idx);
        assert_eq!(batch.edges.len(), 1);
        assert_eq!(batch.edges[0].dst, "sym:a.ts#A.apply");
        assert!(batch.edges[0].confidence > 0.9);
    }

    #[test]
    fn ambiguous_names_produce_low_confidence_edges_not_a_guess() {
        let mut idx = SymbolIndex::default();
        for i in 0..3 {
            idx.add(
                "save",
                &format!("sym:f{i}.py#save"),
                &format!("f{i}.py"),
                Lang::Python,
            );
        }
        let pending = vec![PendingRef {
            from: "sym:x.py#run".into(),
            name: "save".into(),
            file: "x.py".into(),
            kind: EdgeKind::Calls,
        }];
        let batch = resolve(&pending, &idx);
        assert_eq!(batch.edges.len(), 3);
        for e in &batch.edges {
            assert!((e.confidence - 1.0 / 3.0).abs() < 1e-5);
            assert_eq!(e.status, Status::Observed);
        }
    }

    #[test]
    fn a_call_never_crosses_a_language_boundary() {
        // `_` is the translation helper in Python, JavaScript and TypeScript alike.
        // Matching on name alone claims that Python calls TypeScript.
        let mut idx = SymbolIndex::default();
        idx.add("helper", "sym:a.py#helper", "a.py", Lang::Python);
        idx.add("helper", "sym:b.ts#helper", "b.ts", Lang::TypeScript);
        let pending = vec![PendingRef {
            from: "sym:c.py#caller".into(),
            name: "helper".into(),
            file: "c.py".into(),
            kind: EdgeKind::Calls,
        }];
        idx.add("caller", "sym:c.py#caller", "c.py", Lang::Python);
        let batch = resolve(&pending, &idx);
        assert_eq!(batch.edges.len(), 1);
        assert_eq!(batch.edges[0].dst, "sym:a.py#helper");
    }

    #[test]
    fn typescript_and_javascript_still_share_a_call_graph() {
        let mut idx = SymbolIndex::default();
        idx.add("caller", "sym:a.js#caller", "a.js", Lang::JavaScript);
        idx.add("helper", "sym:b.ts#helper", "b.ts", Lang::TypeScript);
        let pending = vec![PendingRef {
            from: "sym:a.js#caller".into(),
            name: "helper".into(),
            file: "a.js".into(),
            kind: EdgeKind::Calls,
        }];
        assert_eq!(resolve(&pending, &idx).edges.len(), 1);
    }

    #[test]
    fn a_one_character_name_does_not_resolve_across_files() {
        let mut idx = SymbolIndex::default();
        idx.add("caller", "sym:a.py#caller", "a.py", Lang::Python);
        idx.add("_", "sym:translate.py#_", "translate.py", Lang::Python);
        let pending = vec![PendingRef {
            from: "sym:a.py#caller".into(),
            name: "_".into(),
            file: "a.py".into(),
            kind: EdgeKind::Calls,
        }];
        assert!(resolve(&pending, &idx).edges.is_empty());
    }

    #[test]
    fn hopelessly_ambiguous_names_are_dropped_rather_than_polluting_the_graph() {
        let mut idx = SymbolIndex::default();
        for i in 0..20 {
            idx.add(
                "get",
                &format!("sym:f{i}.py#get"),
                &format!("f{i}.py"),
                Lang::Python,
            );
        }
        let pending = vec![PendingRef {
            from: "sym:x.py#run".into(),
            name: "get".into(),
            file: "x.py".into(),
            kind: EdgeKind::Calls,
        }];
        assert!(resolve(&pending, &idx).edges.is_empty());
    }

    #[test]
    fn identifier_splitting_bridges_code_and_business_vocabulary() {
        assert_eq!(
            split_identifier("bypassLevelTwoApproval"),
            vec!["bypass", "level", "two", "approval"]
        );
        assert_eq!(
            split_identifier("CUSTOMER_GROUP_ID"),
            vec!["customer", "group", "id"]
        );
        assert_eq!(
            split_identifier("HTTPServerError"),
            vec!["http", "server", "error"]
        );
        assert_eq!(split_identifier("order2Invoice"), vec!["order", "invoice"]);
    }

    #[test]
    fn a_guard_is_mined_once_by_the_innermost_declaration() {
        // A class and its method both contain the same `if`; only the method owns it.
        let src = "class Order:\n    def check(self):\n        if self.corporate:\n            return self.bypass_approval()\n";
        let fx = extract("order.py", src, Lang::Python).unwrap();
        let guards: Vec<&crate::rules::RuleCandidate> = fx
            .rules
            .iter()
            .filter(|r| r.source == crate::rules::RuleSource::CodeGuard)
            .collect();
        assert_eq!(guards.len(), 1, "got {guards:#?}");
        assert!(guards[0].anchor.ends_with("Order.check"));
    }

    #[test]
    fn duplicate_qualified_names_stay_individually_addressable() {
        let src = "def f():\n    pass\ndef f():\n    pass\n";
        let fx = extract("d.py", src, Lang::Python).unwrap();
        let uids: HashSet<&str> = fx.batch.nodes.iter().map(|n| n.uid.as_str()).collect();
        assert_eq!(
            uids.len(),
            2,
            "redefinitions must not collapse onto one uid"
        );
    }

    #[test]
    fn a_file_that_does_not_fully_parse_still_yields_what_it_can() {
        // Mature repositories contain files with syntax the grammar chokes on; the
        // extractor must degrade rather than fail the whole index.
        let src = "def good():\n    pass\n\ndef broken(:\n";
        let fx = extract("b.py", src, Lang::Python).unwrap();
        assert!(fx.batch.nodes.iter().any(|n| n.name == "good"));
    }

    #[test]
    fn import_specifiers_are_extracted_from_both_syntaxes() {
        assert_eq!(
            parse_import_modules("from erpnext.selling import order"),
            vec!["erpnext.selling"]
        );
        assert_eq!(parse_import_modules("import os"), vec!["os"]);
        let es = parse_import_modules("import { A } from \"./policy\"");
        assert!(es.contains(&"./policy".to_string()), "got {es:?}");
    }
}

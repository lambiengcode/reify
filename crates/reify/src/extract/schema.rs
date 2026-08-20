//! Structured model metadata: the declared business vocabulary of a system.
//!
//! Many frameworks describe their domain entities in data rather than in code — a JSON
//! or YAML file naming an entity and listing its fields, each with a machine name and
//! a human label. That file is the single best concept source a repository can offer:
//! it is authored by the team, kept current because the application reads it, and it
//! states the mapping from business vocabulary to database vocabulary explicitly.
//!
//! This module recognises the *shape* rather than any one framework: an object with a
//! name and a list of field objects carrying a machine name and a label. Frappe and
//! ERPNext DocTypes are the shape this was first validated against; the same shape
//! appears wherever a system is metadata-driven.

use std::collections::{BTreeMap, BTreeSet};

use super::FileExtract;
use crate::concepts::{Bridge, Concept};
use crate::extract::code::split_identifier;
use crate::model::{uid, Lang, NewNode, NodeKind, Status};

/// Keys that may hold the entity's human name.
const NAME_KEYS: &[&str] = &["name", "title", "label", "entity", "model"];
/// Keys that may hold the field list.
const FIELD_KEYS: &[&str] = &["fields", "columns", "properties", "attributes"];
/// Keys that may hold a field's machine name.
const FIELD_NAME_KEYS: &[&str] = &["fieldname", "name", "column", "id", "key"];
/// Keys that may hold a field's human label.
const LABEL_KEYS: &[&str] = &["label", "title", "display_name", "caption"];

/// Field types that are layout, not data. They carry no domain vocabulary.
const LAYOUT_TYPES: &[&str] = &[
    "Section Break",
    "Column Break",
    "Tab Break",
    "HTML",
    "Heading",
    "Button",
    "Fold",
];

/// A recognised entity definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelDefinition {
    /// Human name of the entity, e.g. `Sales Order`.
    pub name: String,
    /// `(machine_name, human_label)` for every data-carrying field.
    pub fields: Vec<(String, String)>,
    /// Whether this is a Frappe DocType, whose physical table is `tab<Name>`.
    pub frappe_doctype: bool,
}

impl ModelDefinition {
    /// The physical table name, when the format states enough to know it.
    ///
    /// Frappe stores every DocType in a table called `tab<Name>`, including the space.
    /// Emitting that exact name is what lets a definition merge with the table
    /// references already found in SQL, rather than sitting beside them unconnected.
    pub fn table_name(&self) -> Option<String> {
        self.frappe_doctype
            .then(|| format!("tab{}", self.name).to_ascii_lowercase())
    }
}

/// Recognise an entity definition, or return `None` for ordinary JSON.
pub fn parse(text: &str) -> Option<ModelDefinition> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let object = value.as_object()?;

    let name = NAME_KEYS
        .iter()
        .find_map(|key| object.get(*key).and_then(|v| v.as_str()))
        .filter(|n| !n.is_empty() && n.len() < 80)?
        .to_string();

    let raw_fields = FIELD_KEYS
        .iter()
        .find_map(|key| object.get(*key).and_then(|v| v.as_array()))?;

    let mut fields = Vec::new();
    for entry in raw_fields {
        let Some(field) = entry.as_object() else {
            continue;
        };
        let field_type = field
            .get("fieldtype")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if LAYOUT_TYPES.contains(&field_type) {
            continue;
        }
        let Some(machine) = FIELD_NAME_KEYS
            .iter()
            .find_map(|key| field.get(*key).and_then(|v| v.as_str()))
            .filter(|n| !n.is_empty())
        else {
            continue;
        };
        let label = LABEL_KEYS
            .iter()
            .find_map(|key| field.get(*key).and_then(|v| v.as_str()))
            .filter(|l| !l.is_empty())
            .unwrap_or(machine);
        fields.push((machine.to_string(), label.to_string()));
    }

    // A name with no usable fields is just an object that happens to have a `name`.
    if fields.is_empty() {
        return None;
    }

    let frappe_doctype = object.get("doctype").and_then(|v| v.as_str()) == Some("DocType");
    Some(ModelDefinition {
        name,
        fields,
        frappe_doctype,
    })
}

/// Stage an entity definition as a database object and a grounded concept.
pub fn extract(path: &str, text: &str) -> FileExtract {
    let mut out = FileExtract::default();
    let Some(model) = parse(text) else {
        return out;
    };

    let table = model
        .table_name()
        .unwrap_or_else(|| model.name.to_ascii_lowercase());
    let table_uid = uid::db_object(&table);
    let field_names: Vec<&str> = model.fields.iter().map(|(m, _)| m.as_str()).collect();

    out.batch.node(
        NewNode::new(&table_uid, NodeKind::DatabaseObject, &table)
            .at(path, 0, 0)
            .lang(Lang::Sql)
            .status(Status::Confirmed, 1.0)
            .search(format!(
                "{} {} {}",
                model.name,
                field_names.join(" "),
                model
                    .fields
                    .iter()
                    .map(|(_, label)| label.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            ))
            .data(serde_json::json!({
                "object_kind": "entity",
                "entity": model.name,
                "field_count": model.fields.len(),
                "summary": format!("entity {} ({} fields)", model.name, model.fields.len()),
            })),
    );

    // Every field name and label is domain vocabulary, so it feeds grounding for the
    // whole concept layer, not only for this entity.
    for (machine, label) in &model.fields {
        out.vocabulary.extend(split_identifier(machine));
        out.vocabulary.extend(split_identifier(label));
    }
    out.vocabulary.extend(split_identifier(&model.name));

    // One concept per entity, not one per field: entities are the business nouns a
    // person would name in a task, while field labels like "Status" or "Company"
    // are too generic to be worth their own identity.
    let mut labels = BTreeMap::new();
    labels.insert("eng".to_string(), model.name.clone());
    let mut code = BTreeSet::new();
    code.insert(model.name.replace(' ', ""));
    code.insert(model.name.to_lowercase().replace(' ', "_"));
    let mut db = BTreeSet::new();
    db.insert(table.clone());

    out.concepts.push(Concept {
        id: crate::concepts::normalize_id(&model.name),
        labels,
        code,
        db,
        // Declared by the team in a file the application itself reads, so this is a
        // read fact rather than an inference — but it is the *format* being trusted,
        // not a human statement about the domain, hence Observed rather than Confirmed.
        status: Status::Observed,
        confidence: 0.9,
        bridge: Bridge::Declared,
    });

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOCTYPE: &str = r#"{
      "doctype": "DocType",
      "name": "Sales Order",
      "engine": "InnoDB",
      "fields": [
        {"fieldname": "customer_section", "fieldtype": "Section Break"},
        {"fieldname": "customer", "label": "Customer", "fieldtype": "Link"},
        {"fieldname": "customer_group", "label": "Customer Group", "fieldtype": "Link"},
        {"fieldname": "grand_total", "label": "Grand Total", "fieldtype": "Currency"}
      ]
    }"#;

    #[test]
    fn a_doctype_definition_is_recognised() {
        let model = parse(DOCTYPE).expect("should parse");
        assert_eq!(model.name, "Sales Order");
        assert!(model.frappe_doctype);
        assert_eq!(model.fields.len(), 3, "layout fields must be dropped");
        assert!(model
            .fields
            .contains(&("customer_group".into(), "Customer Group".into())));
    }

    #[test]
    fn the_physical_table_name_matches_what_sql_references() {
        // The SQL extractor sees `tabSales Order`; the definition must land on the
        // same node or the two halves of the data layer never connect.
        let model = parse(DOCTYPE).unwrap();
        assert_eq!(model.table_name().as_deref(), Some("tabsales order"));
        let fx = extract("selling/doctype/sales_order/sales_order.json", DOCTYPE);
        assert!(fx.batch.nodes.iter().any(|n| n.uid == "db:tabsales order"));
    }

    #[test]
    fn an_entity_becomes_one_concept_not_one_per_field() {
        let fx = extract("a/sales_order.json", DOCTYPE);
        assert_eq!(fx.concepts.len(), 1);
        let concept = &fx.concepts[0];
        assert_eq!(concept.id, "SALES_ORDER");
        assert_eq!(concept.labels["eng"], "Sales Order");
        assert!(concept.code.contains("SalesOrder"));
        assert!(concept.code.contains("sales_order"));
        assert!(concept.db.contains("tabsales order"));
    }

    #[test]
    fn field_names_and_labels_all_become_grounding_vocabulary() {
        let fx = extract("a/sales_order.json", DOCTYPE);
        for word in ["customer", "group", "grand", "total", "sales", "order"] {
            assert!(fx.vocabulary.contains(&word.to_string()), "missing {word}");
        }
    }

    #[test]
    fn ordinary_json_is_not_mistaken_for_a_model() {
        assert!(parse(r#"{"name": "reify", "version": "0.1.0"}"#).is_none());
        assert!(parse(r#"{"fields": [1, 2, 3]}"#).is_none());
        assert!(parse("[1, 2, 3]").is_none());
        assert!(parse("not json").is_none());
    }

    #[test]
    fn a_named_object_with_no_usable_fields_is_not_a_model() {
        // package.json has a name and often a list; it is not a domain entity.
        let json = r#"{"name": "app", "fields": [{"fieldtype": "Section Break"}]}"#;
        assert!(parse(json).is_none());
    }

    #[test]
    fn a_generic_field_list_without_frappe_markers_still_works() {
        let json = r#"{
          "title": "Invoice",
          "columns": [
            {"column": "invoice_no", "title": "Invoice Number"},
            {"column": "total_due", "title": "Total Due"}
          ]
        }"#;
        let model = parse(json).expect("the shape, not the framework, is what matters");
        assert_eq!(model.name, "Invoice");
        assert!(!model.frappe_doctype);
        assert_eq!(model.table_name().as_deref(), None);
        assert_eq!(model.fields.len(), 2);
    }
}

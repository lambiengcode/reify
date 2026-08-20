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

use super::richdoc::local_name;
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
    /// Human name of the entity, e.g. `Sales Order` or `Alert`.
    pub name: String,
    /// `(machine_name, human_label)` for every data-carrying field.
    ///
    /// "Label" is whatever the format offers as the second name for a field: a display
    /// label in a UI-driven format, a database column in an ORM mapping. Both are the
    /// same thing for Reify's purposes — a second surface form to ground vocabulary on.
    pub fields: Vec<(String, String)>,
    /// The physical table, when the format states or implies it.
    ///
    /// Carrying the resolved name rather than a per-framework flag keeps this type
    /// honest as formats are added: whoever parses the format knows the naming rule,
    /// and everything downstream just reads a table name.
    pub table: Option<String>,
    /// The code identifier this entity is realised as, when the format states it.
    pub class: Option<String>,
}

impl ModelDefinition {
    /// The physical table name, when the format states enough to know it.
    ///
    /// Emitting the exact name is what lets a definition merge with the table
    /// references already found in SQL, rather than sitting beside them unconnected.
    pub fn table_name(&self) -> Option<String> {
        self.table.clone()
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

    // Frappe stores every DocType in a table called `tab<Name>`, space included.
    let table = (object.get("doctype").and_then(|v| v.as_str()) == Some("DocType"))
        .then(|| format!("tab{name}").to_ascii_lowercase());
    Some(ModelDefinition {
        name,
        fields,
        table,
        class: None,
    })
}

/// Parse a Hibernate mapping (`*.hbm.xml`).
///
/// An ORM mapping is the most explicit statement a codebase makes about its domain: it
/// names the entity, the Java class that realises it, the table it lives in, and every
/// field's column. That is the concept-to-code-to-data triple Reify otherwise has to
/// infer, declared outright.
pub fn parse_hibernate(text: &str) -> Option<ModelDefinition> {
    use quick_xml::events::Event;

    // Cheap gate: every other `.xml` in a Java repository is a build file.
    if !text.contains("hibernate-mapping") {
        return None;
    }

    let mut reader = quick_xml::Reader::from_str(text);
    let mut buffer = Vec::new();
    let mut class: Option<String> = None;
    let mut table: Option<String> = None;
    let mut fields: Vec<(String, String)> = Vec::new();

    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let element = local_name(e.name().as_ref());
                let attribute = |wanted: &str| -> Option<String> {
                    e.attributes().flatten().find_map(|a| {
                        (local_name(a.key.as_ref()) == wanted)
                            .then(|| String::from_utf8_lossy(&a.value).to_string())
                    })
                };
                match element.as_str() {
                    // The first `class` element is the entity; nested ones are
                    // association targets and belong to their own mapping file.
                    "class" if class.is_none() => {
                        class = attribute("name");
                        table = attribute("table").map(|t| t.to_ascii_lowercase());
                    }
                    "id" | "property" | "many-to-one" | "one-to-one" | "version" => {
                        if let Some(field) = attribute("name") {
                            // Hibernate omits `column` when it equals the field name.
                            let column = attribute("column").unwrap_or_else(|| field.clone());
                            fields.push((field, column));
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buffer.clear();
    }

    let class = class?;
    if fields.is_empty() {
        return None;
    }
    // `org.openmrs.notification.Alert` is the entity `Alert`.
    let name = class.rsplit('.').next().unwrap_or(&class).to_string();
    Some(ModelDefinition {
        name,
        fields,
        table,
        class: Some(class),
    })
}

/// Stage a Hibernate mapping.
pub fn extract_hibernate(path: &str, text: &str) -> FileExtract {
    match parse_hibernate(text) {
        Some(model) => stage_model(path, model),
        None => FileExtract::default(),
    }
}

/// Stage an entity definition as a database object and a grounded concept.
pub fn extract(path: &str, text: &str) -> FileExtract {
    match parse(text) {
        Some(model) => stage_model(path, model),
        None => FileExtract::default(),
    }
}

/// Stage an entity definition as a database object and a grounded concept.
fn stage_model(path: &str, model: ModelDefinition) -> FileExtract {
    let mut out = FileExtract::default();
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
    if let Some(class) = &model.class {
        code.insert(class.rsplit('.').next().unwrap_or(class).to_string());
    }
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
        assert_eq!(model.table.as_deref(), Some("tabsales order"));
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

    const HBM: &str = r#"<?xml version="1.0"?>
<hibernate-mapping package="org.openmrs.notification">
  <class name="org.openmrs.notification.Alert" table="notification_alert">
    <id name="alertId" type="java.lang.Integer" column="alert_id"/>
    <property name="text" type="java.lang.String" not-null="true"/>
    <property name="satisfiedByAny" column="satisfied_by_any" type="java.lang.Boolean"/>
    <many-to-one name="creator" class="org.openmrs.User" not-null="true"/>
  </class>
</hibernate-mapping>"#;

    #[test]
    fn a_hibernate_mapping_declares_the_entity_class_and_table() {
        // An ORM mapping states the concept-to-code-to-data triple outright, which is
        // otherwise the thing Reify has to infer.
        let model = parse_hibernate(HBM).expect("should parse");
        assert_eq!(
            model.name, "Alert",
            "the entity is the class's last segment"
        );
        assert_eq!(
            model.class.as_deref(),
            Some("org.openmrs.notification.Alert")
        );
        assert_eq!(model.table.as_deref(), Some("notification_alert"));
    }

    #[test]
    fn hibernate_fields_carry_their_column_even_when_it_is_implied() {
        let model = parse_hibernate(HBM).unwrap();
        // Explicit column.
        assert!(model
            .fields
            .contains(&("satisfiedByAny".into(), "satisfied_by_any".into())));
        // Hibernate omits `column` when it matches the field name.
        assert!(model.fields.contains(&("text".into(), "text".into())));
        // Associations are fields too: they name a domain relationship.
        assert!(model.fields.iter().any(|(f, _)| f == "creator"));
        assert!(model.fields.iter().any(|(f, _)| f == "alertId"));
    }

    #[test]
    fn a_hibernate_mapping_grounds_a_concept_on_both_class_and_table() {
        let fx = extract_hibernate("db/Alert.hbm.xml", HBM);
        assert_eq!(fx.concepts.len(), 1);
        let concept = &fx.concepts[0];
        assert_eq!(concept.id, "ALERT");
        assert!(concept.code.contains("Alert"));
        assert!(concept.db.contains("notification_alert"));
        assert!(fx
            .batch
            .nodes
            .iter()
            .any(|n| n.uid == "db:notification_alert"));
        for word in ["satisfied", "alert", "creator"] {
            assert!(fx.vocabulary.contains(&word.to_string()), "missing {word}");
        }
    }

    #[test]
    fn ordinary_xml_is_not_mistaken_for_a_mapping() {
        // Every other .xml in a Java repository is build configuration.
        assert!(parse_hibernate("<project><artifactId>x</artifactId></project>").is_none());
        assert!(parse_hibernate("not xml at all").is_none());
        assert!(
            parse_hibernate("<hibernate-mapping><class name=\"A\"/></hibernate-mapping>").is_none(),
            "a class with no fields declares nothing"
        );
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
        assert_eq!(model.table, None, "this format does not state a table");
        assert_eq!(model.table_name().as_deref(), None);
        assert_eq!(model.fields.len(), 2);
    }
}

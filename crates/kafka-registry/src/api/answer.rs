//! The shapes of Confluent's answers that carry a schema's contract, in its field order.

use serde::Serialize;

/// A registration answered with its schema, in the order Confluent writes one (`RegisterSchemaResponse`).
#[derive(Serialize)]
pub(crate) struct Registered {
    pub(crate) id: u32,
    pub(crate) version: u32,
    #[serde(rename = "schemaType", skip_serializing_if = "Option::is_none")]
    pub(crate) schema_type: Option<crate::format::SchemaType>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) references: Vec<crate::store::Reference>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::registry::contract_json::metadata"
    )]
    pub(crate) metadata: Option<crate::registry::Metadata>,
    #[serde(
        rename = "ruleSet",
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::registry::contract_json::rule_set"
    )]
    pub(crate) rule_set: Option<crate::registry::RuleSet>,
    pub(crate) schema: String,
}

/// A schema by id, in the order Confluent writes one (`SchemaString`): its type, its text, its references, its
/// metadata and rule set, the highest id.
#[derive(Serialize)]
pub(crate) struct ById {
    #[serde(rename = "schemaType", skip_serializing_if = "Option::is_none")]
    pub(crate) schema_type: Option<crate::format::SchemaType>,
    pub(crate) schema: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) references: Vec<crate::store::Reference>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::registry::contract_json::metadata"
    )]
    pub(crate) metadata: Option<crate::registry::Metadata>,
    #[serde(
        rename = "ruleSet",
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::registry::contract_json::rule_set"
    )]
    pub(crate) rule_set: Option<crate::registry::RuleSet>,
    #[serde(rename = "maxId", skip_serializing_if = "Option::is_none")]
    pub(crate) max_id: Option<u32>,
}

//! Which differences break a reader, and Confluent's sentence for each.

/// The differences a reader can live with.
const COMPATIBLE: &[&str] = &[
    "TYPE_EXTENDED",
    "SCHEMA_REMOVED",
    "MAX_LENGTH_REMOVED",
    "MAX_LENGTH_INCREASED",
    "MIN_LENGTH_REMOVED",
    "MIN_LENGTH_DECREASED",
    "PATTERN_REMOVED",
    "MAXIMUM_REMOVED",
    "MAXIMUM_INCREASED",
    "MINIMUM_REMOVED",
    "MINIMUM_DECREASED",
    "EXCLUSIVE_MAXIMUM_REMOVED",
    "EXCLUSIVE_MAXIMUM_INCREASED",
    "EXCLUSIVE_MINIMUM_REMOVED",
    "EXCLUSIVE_MINIMUM_DECREASED",
    "MULTIPLE_OF_REMOVED",
    "MULTIPLE_OF_REDUCED",
    "REQUIRED_ATTRIBUTE_WITH_DEFAULT_ADDED",
    "REQUIRED_ATTRIBUTE_REMOVED",
    "DEPENDENCY_ARRAY_REMOVED",
    "DEPENDENCY_ARRAY_NARROWED",
    "PROPERTY_REMOVED_FROM_OPEN_CONTENT_MODEL",
    "PROPERTY_ADDED_TO_CLOSED_CONTENT_MODEL",
    "REQUIRED_PROPERTY_WITH_DEFAULT_ADDED_TO_UNOPEN_CONTENT_MODEL",
    "PROPERTY_ADDED_IS_COVERED_BY_PARTIALLY_OPEN_CONTENT_MODEL",
    "PROPERTY_REMOVED_IS_COVERED_BY_PARTIALLY_OPEN_CONTENT_MODEL",
    "ADDITIONAL_PROPERTIES_ADDED",
    "ADDITIONAL_PROPERTIES_EXTENDED",
    "MAX_PROPERTIES_REMOVED",
    "MAX_PROPERTIES_INCREASED",
    "MIN_PROPERTIES_REMOVED",
    "MIN_PROPERTIES_DECREASED",
    "MAX_ITEMS_REMOVED",
    "MAX_ITEMS_INCREASED",
    "MIN_ITEMS_REMOVED",
    "MIN_ITEMS_DECREASED",
    "UNIQUE_ITEMS_REMOVED",
    "ADDITIONAL_ITEMS_ADDED",
    "ADDITIONAL_ITEMS_EXTENDED",
    "ITEM_REMOVED_FROM_CLOSED_CONTENT_MODEL",
    "ITEM_ADDED_IS_COVERED_BY_PARTIALLY_OPEN_CONTENT_MODEL",
    "ITEM_REMOVED_IS_COVERED_BY_PARTIALLY_OPEN_CONTENT_MODEL",
    "ITEM_REMOVED_FROM_OPEN_CONTENT_MODEL",
    "ITEM_ADDED_TO_CLOSED_CONTENT_MODEL",
    "ENUM_ARRAY_EXTENDED",
    "COMBINED_TYPE_EXTENDED",
    "PRODUCT_TYPE_NARROWED",
    "SUM_TYPE_EXTENDED",
    "NOT_TYPE_NARROWED",
];

pub(super) fn breaks(kind: &str) -> bool {
    !COMPATIBLE.contains(&kind)
}

/// Confluent's sentence for a change, the reader's side and the writer's named ("new" or "old"). A type the corpus has
/// not shown takes its family's sentence (all the *_ADDED, *_INCREASED ... that were recorded share one each).
pub(crate) fn describe(kind: &str, path: &str, reader: &str, writer: &str) -> String {
    const TYPE_DIFFERS: &str =
        "A type at path '%s' is different between the R schema and the W schema";
    const VALUE_DIFFERS: &str = "The value at path '%s' is different between the R and W schema";
    let t = match kind {
        "TYPE_CHANGED"
        | "TYPE_NARROWED"
        | "COMBINED_TYPE_CHANGED"
        | "COMBINED_TYPE_SUBSCHEMAS_CHANGED"
        | "ENUM_ARRAY_CHANGED" => TYPE_DIFFERS,
        "MULTIPLE_OF_CHANGED" | "PATTERN_CHANGED" => VALUE_DIFFERS,
        "ITEM_ADDED_TO_OPEN_CONTENT_MODEL" | "PROPERTY_ADDED_TO_OPEN_CONTENT_MODEL" => {
            "The R schema has an open content model and has a property or item at path '%s' which is missing in the W schema"
        }
        "PROPERTY_ADDED_NOT_COVERED_BY_PARTIALLY_OPEN_CONTENT_MODEL"
        | "ITEM_ADDED_NOT_COVERED_BY_PARTIALLY_OPEN_CONTENT_MODEL" => {
            "The R schema has a property or item at path '%s' which is missing in the W schema and is not covered by its partially open content model"
        }
        "PROPERTY_REMOVED_FROM_CLOSED_CONTENT_MODEL" | "ITEM_REMOVED_FROM_CLOSED_CONTENT_MODEL" => {
            "The R has a closed content model and is missing a property or item present at path '%s' in the W schema"
        }
        "PROPERTY_REMOVED_NOT_COVERED_BY_PARTIALLY_OPEN_CONTENT_MODEL"
        | "ITEM_REMOVED_NOT_COVERED_BY_PARTIALLY_OPEN_CONTENT_MODEL" => {
            "A property or item is missing in the R schema but present at path '%s' in the W schema and is not covered by its partially open content model"
        }
        "REQUIRED_PROPERTY_ADDED_TO_UNOPEN_CONTENT_MODEL" => {
            "The R schema has an unopen content model and has a required property at path '%s' which is missing in the W schema"
        }
        "SCHEMA_ADDED" => return String::new(),
        k if k.ends_with("_ADDED") || k.ends_with("_REMOVED") => {
            "The keyword at path '%s' in the R schema is not present in the W schema"
        }
        k if k.ends_with("_INCREASED") || k.ends_with("_EXPANDED") => {
            "The value at path '%s' in the R schema is more than its value in the W schema"
        }
        k if k.ends_with("_DECREASED") || k.ends_with("_REDUCED") => {
            "The value at path '%s' in the R schema is less than its value in the W schema"
        }
        k if k.ends_with("_NARROWED") => {
            "An array or combined type at path '%s' has fewer elements in the R schema than the W schema"
        }
        k if k.ends_with("_EXTENDED") => {
            "An array or combined type at path '%s' has more elements in the R schema than the W schema"
        }
        _ => TYPE_DIFFERS,
    };
    let path = if path == "#" { "#/" } else { path }; // the root, as Confluent writes it
    t.replace("R and W", &format!("{reader} and {writer}"))
        .replace("The R has", &format!("The {reader} has"))
        .replace("R schema", &format!("{reader} schema"))
        .replace("W schema", &format!("{writer} schema"))
        .replace("%s", path)
}

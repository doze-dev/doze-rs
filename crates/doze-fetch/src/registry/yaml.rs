//! The few YAML reads the registry's files and `doze.lock` need, on `yaml-rust2`'s tree: a mapping at the top,
//! scalars read as the text they were written as.

use yaml_rust2::Yaml;
use yaml_rust2::yaml::Hash;

/// Why a YAML file could not be read, and where (a byte offset into it), so the error can point at the line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YamlError {
    pub message: String,
    pub at: Option<usize>,
}

impl YamlError {
    pub(crate) fn new(message: impl Into<String>) -> YamlError {
        YamlError {
            message: message.into(),
            at: None,
        }
    }
}

/// Parses `text`, which must hold one mapping.
pub(crate) fn mapping(text: &str) -> Result<Hash, YamlError> {
    let docs = yaml_rust2::YamlLoader::load_from_str(text).map_err(|e| YamlError {
        message: e.info().to_string(),
        at: Some(char_to_byte(text, e.marker().index())),
    })?;
    match docs.into_iter().next() {
        Some(Yaml::Hash(h)) => Ok(h),
        Some(_) => Err(YamlError::new(
            "the file is not a mapping of keys to values",
        )),
        None => Err(YamlError::new("the file is empty")),
    }
}

/// `yaml-rust2` counts its marker in characters; spans are bytes.
fn char_to_byte(text: &str, chars: usize) -> usize {
    text.char_indices()
        .nth(chars)
        .map_or(text.len(), |(i, _)| i)
}

/// The value under `key`, or `Yaml::BadValue` when there is none.
pub(crate) fn get<'a>(h: &'a Hash, key: &str) -> &'a Yaml {
    h.get(&Yaml::String(key.to_string()))
        .unwrap_or(&Yaml::BadValue)
}

/// A scalar as the text it was written as (`8`, `0.2.1`, `true`), or `None` for anything else.
pub(crate) fn scalar(y: &Yaml) -> Option<String> {
    match y {
        Yaml::String(s) | Yaml::Real(s) => Some(s.clone()),
        Yaml::Integer(i) => Some(i.to_string()),
        Yaml::Boolean(b) => Some(b.to_string()),
        _ => None,
    }
}

/// The scalar under `key`, or empty.
pub(crate) fn text(h: &Hash, key: &str) -> String {
    scalar(get(h, key)).unwrap_or_default()
}

/// A list of scalars under `key` (absent is empty).
pub(crate) fn texts(h: &Hash, key: &str) -> Vec<String> {
    get(h, key)
        .as_vec()
        .map(|v| v.iter().filter_map(scalar).collect())
        .unwrap_or_default()
}

/// A mapping under `key`: `Ok(None)` when absent or null, an error when it is something else.
pub(crate) fn sub<'a>(h: &'a Hash, key: &str) -> Result<Option<&'a Hash>, YamlError> {
    match get(h, key) {
        Yaml::Hash(m) => Ok(Some(m)),
        Yaml::BadValue | Yaml::Null => Ok(None),
        _ => Err(YamlError::new(format!("`{key}` is not a mapping"))),
    }
}

/// Each entry of a mapping, with its key as text.
pub(crate) fn entries(h: &Hash) -> impl Iterator<Item = (String, &Yaml)> {
    h.iter().filter_map(|(k, v)| scalar(k).map(|k| (k, v)))
}

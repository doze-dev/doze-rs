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

/// How deep a document may nest: these files are three or four levels deep.
const MAX_DEPTH: usize = 32;

/// Refuses anchors, aliases and deep nesting before a tree is built: `yaml-rust2` copies each alias whole, so a few
/// hundred bytes of nested aliases ask for gigabytes (Go's yaml.v3 refuses "excessive aliasing"). None of doze's
/// files use them.
#[derive(Default)]
struct Guard {
    depth: usize,
    refused: Option<(String, usize)>,
}

impl yaml_rust2::parser::MarkedEventReceiver for Guard {
    fn on_event(&mut self, ev: yaml_rust2::parser::Event, mark: yaml_rust2::scanner::Marker) {
        use yaml_rust2::parser::Event;
        if self.refused.is_some() {
            return;
        }
        let anchored = match &ev {
            Event::Alias(_) => true,
            Event::Scalar(_, _, a, _) | Event::SequenceStart(a, _) | Event::MappingStart(a, _) => {
                *a != 0
            }
            _ => false,
        };
        match ev {
            Event::SequenceStart(..) | Event::MappingStart(..) => self.depth += 1,
            Event::SequenceEnd | Event::MappingEnd => self.depth = self.depth.saturating_sub(1),
            _ => {}
        }
        if anchored {
            self.refused = Some((
                "anchors and aliases are not allowed here".into(),
                mark.index(),
            ));
        } else if self.depth > MAX_DEPTH {
            self.refused = Some((
                format!("nested deeper than {MAX_DEPTH} levels"),
                mark.index(),
            ));
        }
    }
}

/// Parses `text`, which must hold one mapping.
pub(crate) fn mapping(text: &str) -> Result<Hash, YamlError> {
    let mut guard = Guard::default();
    // A syntax error is the loader's to report below, with its position.
    let _ = yaml_rust2::parser::Parser::new_from_str(text).load(&mut guard, true);
    if let Some((message, at)) = guard.refused {
        return Err(YamlError {
            message,
            at: Some(char_to_byte(text, at)),
        });
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The alias bomb the F1 audit measured (342 bytes asked for 2 GB) is refused before any tree is built.
    #[test]
    fn aliases_and_deep_nesting_are_refused() {
        let mut bomb = String::from("a: &a [x, x, x, x, x, x, x, x, x]\n");
        for i in 0..12 {
            let prev = if i == 0 {
                "a".to_string()
            } else {
                format!("b{}", i - 1)
            };
            bomb += &format!(
                "b{i}: &b{i} [*{prev}, *{prev}, *{prev}, *{prev}, *{prev}, *{prev}, *{prev}, *{prev}, *{prev}]\n"
            );
        }
        let t = std::time::Instant::now();
        let err = mapping(&bomb).unwrap_err();
        assert!(err.message.contains("anchors and aliases"), "{err:?}");
        assert!(t.elapsed() < std::time::Duration::from_secs(1));
        let deep = format!("a: {}{}", "[".repeat(40), "]".repeat(40));
        assert!(
            mapping(&deep)
                .unwrap_err()
                .message
                .contains("nested deeper")
        );
        assert!(mapping("a: {b: [1, 2]}\n").is_ok());
    }
}

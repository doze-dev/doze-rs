//! A module's `meta.yaml`: the documentation the registry site renders and `doze modules docs` prints, as
//! `doze-module-release` (and Go's `dzm meta` before it) writes it from the module's own manifest. It is prose, not
//! signed and never executed; what decides what runs is the signed index.

use super::yaml::{self, YamlError, get, text};
use serde::Serialize;
use yaml_rust2::Yaml;
use yaml_rust2::yaml::Hash;

/// One argument a block takes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Arg {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub required: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub default: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub desc: String,
    /// The first module version that takes it, when it arrived later.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub since: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub until: String,
}

/// A nested block type and its arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlockDoc {
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub label: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub desc: String,
    pub arguments: Vec<Arg>,
}

/// The documentation of one module.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct Meta {
    pub title: String,
    pub tagline: String,
    pub category: String,
    pub engine: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    pub example: String,
    pub description: String,
    pub homepage: String,
    pub arguments: Vec<Arg>,
    pub blocks: Vec<BlockDoc>,
}

// fuzz:parser fuzz_meta
/// Reads a module's documentation.
pub fn parse(src: &str) -> Result<Meta, YamlError> {
    let h = yaml::mapping(src)?;
    let config = yaml::sub(&h, "config")?;
    Ok(Meta {
        title: text(&h, "title"),
        tagline: text(&h, "tagline"),
        category: text(&h, "category"),
        engine: text(&h, "engine"),
        port: get(&h, "port").as_i64().and_then(|p| u16::try_from(p).ok()),
        example: text(&h, "example"),
        description: text(&h, "description"),
        homepage: text(&h, "homepage"),
        arguments: config.map(args).unwrap_or_default(),
        blocks: config
            .and_then(|c| get(c, "blocks").as_vec())
            .map(|bs| bs.iter().filter_map(Yaml::as_hash).map(block).collect())
            .unwrap_or_default(),
    })
}

fn args(h: &Hash) -> Vec<Arg> {
    get(h, "arguments")
        .as_vec()
        .map(|v| v.iter().filter_map(Yaml::as_hash).map(arg).collect())
        .unwrap_or_default()
}

fn arg(a: &Hash) -> Arg {
    Arg {
        name: text(a, "name"),
        ty: text(a, "type"),
        required: get(a, "required").as_bool() == Some(true),
        default: text(a, "default"),
        desc: text(a, "desc"),
        since: text(a, "since"),
        until: text(a, "until"),
    }
}

fn block(b: &Hash) -> BlockDoc {
    BlockDoc {
        name: text(b, "name"),
        label: text(b, "label"),
        desc: text(b, "desc"),
        arguments: args(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The module contract grows (`client`, `detect`, …): a document with fields this doze does not read still parses,
    /// and what it does read is intact. The cache keeps the published bytes as they are, so a newer reader finds them.
    #[test]
    fn fields_this_doze_does_not_read_are_left_alone() {
        let src = "title: Valkey\ntagline: kv\ncategory: cache\nengine: valkey\nport: 6379\nexample: 'valkey \"cache\" {}'\n\
                   client:\n  program: valkey-cli\n  args: [-p, '{port}']\n\
                   detect:\n  packages:\n    npm: [ioredis, redis]\n  images: [valkey/valkey]\n\
                   exampleLabel: cache\nsomethingNew: {nested: [1, 2]}\n";
        let m = parse(src).unwrap();
        assert_eq!(
            (m.title.as_str(), m.engine.as_str(), m.port),
            ("Valkey", "valkey", Some(6379))
        );
    }
}

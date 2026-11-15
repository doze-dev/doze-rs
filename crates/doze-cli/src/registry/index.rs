//! A module's signed release index (`<ns>/<name>/index.yaml`, schema 1), as the registry's signer
//! (doze-registry `scripts/lib.mjs`) writes it and the Go `modindex` reads it. Two signatures: the index's own covers
//! the release metadata (protocol, engine majors, channels, each artifact's sha256 and signature), so a host cannot
//! lie about compatibility or roll a channel back; each artifact's covers what doze would run.

use super::signed;
use super::yaml::{self, YamlError, entries, scalar, sub, text, texts};
use serde::Serialize;
use std::collections::BTreeMap;
use yaml_rust2::Yaml;

/// The only schema this doze reads.
pub const SCHEMA: i64 = 1;

/// One downloadable archive of a release, for one platform.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Artifact {
    pub url: String,
    pub sha256: String,
    pub sig: String,
}

/// One published module version.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Release {
    /// The module protocol this build speaks.
    pub protocol: i64,
    /// The engine majors it runs; empty means no gate.
    pub engines: Vec<String>,
    pub artifacts: BTreeMap<String, Artifact>,
    /// The oldest doze that runs it (outside the signed payload, as Go publishes it); empty for older releases.
    pub doze: String,
}

/// A module's index.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Index {
    pub module: String,
    pub namespace: String,
    pub releases: BTreeMap<String, Release>,
    /// `None` when the file has none: it signs as `null`, as Go's nil map does.
    pub channels: Option<BTreeMap<String, String>>,
    pub signature: String,
}

// fuzz:parser fuzz_index
/// Reads an index. Anything but schema 1 is refused (the module must be re-published for this doze).
pub fn parse(text_: &str) -> Result<Index, YamlError> {
    let h = yaml::mapping(text_)?;
    if yaml::get(&h, "schema").as_i64() != Some(SCHEMA) {
        return Err(YamlError::new(format!(
            "not a schema-{SCHEMA} module index"
        )));
    }
    let module = text(&h, "module");
    if module.is_empty() {
        return Err(YamlError::new("the index names no module"));
    }
    let mut releases = BTreeMap::new();
    for (version, r) in sub(&h, "releases")?.into_iter().flat_map(entries) {
        releases.insert(version.clone(), release(&version, r)?);
    }
    let channels = sub(&h, "channels")?.map(|c| {
        entries(c)
            .filter_map(|(k, v)| scalar(v).map(|v| (k, v)))
            .collect()
    });
    Ok(Index {
        module,
        namespace: text(&h, "namespace"),
        releases,
        channels,
        signature: text(&h, "signature"),
    })
}

fn release(version: &str, r: &Yaml) -> Result<Release, YamlError> {
    let Yaml::Hash(r) = r else {
        return Err(YamlError::new(format!(
            "release {version} is not a mapping"
        )));
    };
    let Some(protocol) = yaml::get(r, "protocol").as_i64() else {
        return Err(YamlError::new(format!(
            "release {version} has no protocol number"
        )));
    };
    let mut artifacts = BTreeMap::new();
    for (triple, a) in sub(r, "artifacts")?.into_iter().flat_map(entries) {
        let Yaml::Hash(a) = a else {
            return Err(YamlError::new(format!(
                "release {version}: artifact {triple} is not a mapping"
            )));
        };
        artifacts.insert(
            triple,
            Artifact {
                url: text(a, "url"),
                sha256: text(a, "sha256"),
                sig: text(a, "sig"),
            },
        );
    }
    Ok(Release {
        protocol,
        engines: texts(r, "engines"),
        artifacts,
        doze: text(r, "doze"),
    })
}

// The signed payload: JSON, keys sorted (the fields below are declared in order), no whitespace, empty optional
// fields left out. Byte for byte what the registry's signer produces (the conformance fixture pins it).
#[derive(Serialize)]
struct Payload<'a> {
    channels: Option<&'a BTreeMap<String, String>>,
    module: &'a str,
    namespace: &'a str,
    releases: BTreeMap<&'a str, PayloadRelease<'a>>,
}

#[derive(Serialize)]
struct PayloadRelease<'a> {
    artifacts: BTreeMap<&'a str, PayloadArtifact<'a>>,
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    engines: &'a [String],
    protocol: i64,
}

#[derive(Serialize)]
struct PayloadArtifact<'a> {
    sha256: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    sig: &'a str,
    url: &'a str,
}

/// The bytes the index signature covers.
pub fn payload(idx: &Index) -> String {
    let releases = idx
        .releases
        .iter()
        .map(|(v, r)| {
            let artifacts = r
                .artifacts
                .iter()
                .map(|(t, a)| {
                    let art = PayloadArtifact {
                        sha256: a.sha256.to_ascii_lowercase(),
                        sig: &a.sig,
                        url: &a.url,
                    };
                    (t.as_str(), art)
                })
                .collect();
            let rel = PayloadRelease {
                artifacts,
                engines: &r.engines,
                protocol: r.protocol,
            };
            (v.as_str(), rel)
        })
        .collect();
    let p = Payload {
        channels: idx.channels.as_ref(),
        module: &idx.module,
        namespace: &idx.namespace,
        releases,
    };
    serde_json::to_string(&p).unwrap_or_default()
}

/// Checks the index's own signature against the registry key.
pub fn verify(idx: &Index, key: &ed25519_dalek::VerifyingKey) -> Result<(), signed::Refused> {
    signed::verify(key, &payload(idx), &idx.signature)
}

impl Index {
    /// The `stable` channel's head, if the index names one.
    pub fn stable(&self) -> Option<&str> {
        self.channels.as_ref()?.get("stable").map(String::as_str)
    }
}

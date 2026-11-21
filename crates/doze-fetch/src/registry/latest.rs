//! The release file: what the newest doze is, two lines of what is new, and whether it fixes a security problem
//! (compat.md rule 7: "a small signed file from the registry … instead of GitHub's API"). Served at
//! `<registry>/latest.yaml` and signed like an index, with the same key:
//!
//! ```yaml
//! schema: 1
//! version: 0.3.0
//! news:
//!   - valkey sleeps with a snapshot and wakes from it
//! security: false
//! signature: <base64 ed25519 over the hex SHA-256 of {"news":[…],"security":false,"version":"0.3.0"}>
//! ```
//!
//! The registry does not publish it yet (doze-registry needs a step in `publish.mjs`); until it does, the check finds
//! nothing and no one is told anything.

use super::signed;
use super::yaml::{self, YamlError, text, texts};
use serde::{Deserialize, Serialize};

/// The path under the registry's root.
pub const PATH: &str = "latest.yaml";

/// What the release file says.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Latest {
    pub version: String,
    pub news: Vec<String>,
    pub security: bool,
}

// Sorted keys, as every signed payload.
#[derive(Serialize)]
struct Payload<'a> {
    news: &'a [String],
    security: bool,
    version: &'a str,
}

/// The bytes the signature covers.
pub fn payload(l: &Latest) -> String {
    let p = Payload {
        news: &l.news,
        security: l.security,
        version: &l.version,
    };
    serde_json::to_string(&p).unwrap_or_default()
}

// fuzz:parser fuzz_latest
/// Reads the release file and checks its signature: an unsigned or wrongly signed file is refused whole.
pub fn parse(src: &str, key: &ed25519_dalek::VerifyingKey) -> Result<Latest, YamlError> {
    let h = yaml::mapping(src)?;
    if yaml::get(&h, "schema").as_i64() != Some(1) {
        return Err(YamlError::new("not a schema-1 release file"));
    }
    let latest = Latest {
        version: text(&h, "version"),
        news: texts(&h, "news").into_iter().take(2).collect(),
        security: yaml::get(&h, "security").as_bool() == Some(true),
    };
    if !super::select::is_doze_version(&latest.version) {
        return Err(YamlError::new("the release file names no version"));
    }
    // The two lines are signed as published; taking two of more would change the payload, so check the file's own.
    let published = Latest {
        news: texts(&h, "news"),
        ..latest.clone()
    };
    signed::verify(key, &payload(&published), &text(&h, "signature"))
        .map_err(|e| YamlError::new(format!("the release file is refused: {e}")))?;
    Ok(latest)
}

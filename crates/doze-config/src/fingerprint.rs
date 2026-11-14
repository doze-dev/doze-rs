//! A block's fingerprint: what it is and what it runs with, hashed, so that a reload can tell
//! which blocks changed (Go's `Fingerprint` and `BlockFingerprints`).
//!
//! The header line is Go's; the spec is hashed from its own canonical text (Go hashes its JSON).
//! Fingerprints are only ever compared with fingerprints from the same build, so the two need
//! not agree.

use crate::model::{Config, Instance, Kind, Spec};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// A block's kind, type and fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockPrint {
    pub kind: Kind,
    pub ty: String,
    pub fingerprint: String,
}

impl Instance {
    pub fn fingerprint(&self) -> String {
        let mut h = Sha256::new();
        h.update(format!(
            "{}|{}|{}|{}|{}|{}|{}\n",
            self.kind().as_str(),
            self.ty,
            self.name,
            self.version,
            self.listen,
            self.port,
            self.enabled
        ));
        let mut deps: Vec<String> = self
            .deps
            .iter()
            .map(|d| format!("{}:{}", d.name, d.condition.as_str()))
            .collect();
        deps.sort();
        h.update(format!("[{}]\n", deps.join(" ")));
        match &self.spec {
            Spec::None => {}
            Spec::Command(c) => h.update(format!("{c:?}")),
            Spec::Module(b) => h.update(format!("{b:?}")),
        }
        hex::encode(h.finalize())
    }
}

impl Config {
    /// Every block's print, by name.
    pub fn block_fingerprints(&self) -> BTreeMap<String, BlockPrint> {
        self.instances
            .iter()
            .map(|d| {
                (
                    d.name.clone(),
                    BlockPrint {
                        kind: d.kind(),
                        ty: d.ty.clone(),
                        fingerprint: d.fingerprint(),
                    },
                )
            })
            .collect()
    }
}

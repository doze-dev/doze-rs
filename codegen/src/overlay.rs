//! Overlays: constraints that exist only in AWS's prose documentation get a machine-readable home here,
//! without touching the vendored model.
//!
//! `{"shapes": {...}, "retarget": {"ns#Struct$member": "ns#NewTarget"}}`: extra shapes, and members re-pointed
//! at them. Every overlay entry is evidence-tagged in its documentation trait (PLAN.md rule 10).

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use indexmap::IndexMap;
use serde::Deserialize;

use crate::model::{Model, Shape};

#[derive(Deserialize, Default)]
pub struct Overlay {
    #[serde(default)]
    shapes: IndexMap<String, Shape>,
    #[serde(default)]
    retarget: BTreeMap<String, String>,
}

impl Overlay {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).context("parsing overlay")
    }

    pub fn apply(self, model: &mut Model) -> Result<()> {
        for (id, shape) in self.shapes {
            model.shapes.insert(id, shape);
        }
        for (path, target) in self.retarget {
            let (shape_id, member) = path
                .split_once('$')
                .with_context(|| format!("retarget key {path}"))?;
            match model.shapes.get_mut(shape_id) {
                Some(Shape::Structure(s) | Shape::Union(s)) => {
                    s.members
                        .get_mut(member)
                        .with_context(|| format!("no member {path}"))?
                        .target = target;
                }
                _ => bail!("retarget: {shape_id} is not a structure"),
            }
        }
        Ok(())
    }
}

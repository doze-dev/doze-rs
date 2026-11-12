//! A bucket's versioning state.

use serde_json::json;

use crate::core::Core;
use crate::errors::{invalid_bucket_state, malformed_xml};
use crate::model::Versioning;
use crate::req::{Answer, Out, Req};

impl Core {
    pub(crate) fn get_versioning(&self, req: &Req<'_>) -> Answer {
        let b = self.require(req.bucket())?;
        Out::json(match b.versioning {
            Versioning::Off => json!({}),
            Versioning::Enabled => json!({"Status": "Enabled"}),
            Versioning::Suspended => json!({"Status": "Suspended"}),
        })
    }

    pub(crate) fn put_versioning(&mut self, req: &Req<'_>) -> Answer {
        let b = self.require(req.bucket())?;
        let status = req
            .get("VersioningConfiguration")
            .and_then(|c| c.get("Status"))
            .and_then(serde_json::Value::as_str);
        let next = match status {
            Some("Enabled") => Versioning::Enabled,
            Some("Suspended") => Versioning::Suspended,
            _ => return Err(malformed_xml()),
        };
        if b.object_lock && next == Versioning::Suspended {
            return Err(invalid_bucket_state(
                "An Object Lock configuration is present on this bucket, so the versioning state cannot be changed.",
            ));
        }
        self.set_versioning(req.bucket(), next);
        Out::empty()
    }

    pub(crate) fn set_versioning(&mut self, bucket: &str, to: Versioning) {
        if let Some(b) = self.buckets.get_mut(bucket) {
            b.versioning = to;
        }
    }
}

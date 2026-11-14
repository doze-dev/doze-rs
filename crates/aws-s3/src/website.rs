//! Website hosting: what a bucket with a website configuration serves for a page request. The rules are the S3 User Guide's
//! ("Configuring an index document", "Configuring a custom error document", "(Optional) Configuring a webpage redirect"):
//!
//! - a request for the root or for a name ending in `/` is for that folder's index document (`<name>` + `IndexDocument.Suffix`);
//! - a request for `photos` when there is no object `photos` but there is `photos/<suffix>` is redirected (302) to `photos/`;
//! - an object with a website redirect location answers 301 to it, and `RedirectAllRequestsTo` answers every request with a 301;
//! - anything else not found is a 404, with the `ErrorDocument` object as its body when the bucket has one.
//!
//! Routing rules are not applied (SCOPE.md cuts them); a configuration that has them is served as if it had none.

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::errors::no_such_bucket;
use crate::get::slices;
use crate::req::{Answer, Out};

/// The redirect a page answers with: the status, and where to (a path is relative to the bucket's website root).
fn redirect(status: u16, location: String) -> Answer {
    Ok(Out {
        json: json!({"Location": location}),
        status: Some(status),
        body: None,
    })
}

impl Core {
    /// The bucket's CORS and website configurations, as stored (`null` where there is none).
    pub(crate) fn configs(&self, bucket: &str) -> Answer {
        let b = self
            .buckets
            .get(bucket)
            .ok_or_else(|| no_such_bucket(bucket))?;
        Out::json(json!({
            "cors": b.configs.get("cors"),
            "website": b.configs.get("website"),
        }))
    }

    /// The answer to a page request for `key` (decoded, without its leading `/`) of `bucket`'s website.
    pub(crate) fn website(&self, bucket: &str, key: &str) -> Answer {
        let b = self
            .buckets
            .get(bucket)
            .ok_or_else(|| no_such_bucket(bucket))?;
        // (memory) the code and message of a bucket that is not a website.
        let config: Value = b
            .configs
            .get("website")
            .and_then(|c| serde_json::from_str(c).ok())
            .ok_or_else(|| {
                ApiError::new(
                    404,
                    "NoSuchWebsiteConfiguration",
                    "The specified bucket does not have a website configuration",
                )
                .with("BucketName", bucket)
            })?;
        if let Some(all) = config.get("RedirectAllRequestsTo") {
            let text = |m: &str| all.get(m).and_then(Value::as_str).unwrap_or_default();
            let scheme = Some(text("Protocol"))
                .filter(|p| !p.is_empty())
                .unwrap_or("http");
            return redirect(301, format!("{scheme}://{}/{key}", text("HostName")));
        }
        let suffix = config
            .pointer("/IndexDocument/Suffix")
            .and_then(Value::as_str)
            .unwrap_or("index.html");
        let wanted = if key.is_empty() || key.ends_with('/') {
            format!("{key}{suffix}")
        } else {
            key.to_owned()
        };
        if let Some(found) = self.web_page(bucket, &wanted, None)? {
            return Ok(found);
        }
        if !key.is_empty()
            && !key.ends_with('/')
            && self.live(bucket, &format!("{key}/{suffix}"))?.is_some()
        {
            return redirect(302, format!("{key}/"));
        }
        let missing = ApiError::new(404, "NoSuchKey", "The specified key does not exist.")
            .with("Key", wanted.as_str());
        match config.pointer("/ErrorDocument/Key").and_then(Value::as_str) {
            Some(doc) => self.web_page(bucket, doc, Some(404))?.ok_or(missing),
            None => Err(missing),
        }
    }

    /// The newest version of `key` that is an object, if there is one.
    fn live(&self, bucket: &str, key: &str) -> Result<Option<crate::model::Version>, ApiError> {
        Ok(self.latest(bucket, key)?.filter(|v| !v.delete_marker))
    }

    /// The object `key` as a page, with `status` (200 when `None`); a redirect when the object names one.
    fn web_page(
        &self,
        bucket: &str,
        key: &str,
        status: Option<u16>,
    ) -> Result<Option<Out>, ApiError> {
        let Some(v) = self.live(bucket, key)? else {
            return Ok(None);
        };
        if let Some(to) = v.meta.website_redirect.clone().filter(|_| status.is_none()) {
            return redirect(301, to).map(Some);
        }
        let headers = self.object_headers(bucket, &v, false);
        Ok(Some(Out {
            json: Value::Object(headers),
            status,
            body: Some(slices(self, &v.segs, 0, v.size)?),
        }))
    }
}

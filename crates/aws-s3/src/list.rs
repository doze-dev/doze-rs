//! ListObjects and ListObjectsV2: the newest version of each key, in key order, cut at a prefix, folded at a delimiter, and paged.

use aws_wire::ApiError;
use doze_store::record;
use serde_json::{Map, Value, json};

use crate::buckets::owner;
use crate::core::{Core, VERSION, VERSIONS};
use crate::model::Version;
use crate::put::algo_name;
use crate::req::{Answer, Out, Req};

/// The most keys a page holds.
pub(crate) const MAX_KEYS: usize = 1000;

/// How a key name travels in the answer.
#[derive(Clone, Copy)]
pub(crate) struct Enc(pub(crate) bool);

impl Enc {
    pub(crate) fn of(req: &Req<'_>) -> Self {
        Self(req.s("EncodingType") == Some("url"))
    }

    pub(crate) fn text(self, s: &str) -> String {
        if self.0 {
            aws_core::uri::encode(s.as_bytes(), false)
        } else {
            s.to_owned()
        }
    }
}

/// One entry of a page.
pub(crate) enum Entry {
    Object(Box<Version>),
    Prefix(String),
}

/// A page of a listing.
pub(crate) struct Page {
    pub(crate) entries: Vec<Entry>,
    /// The last key or prefix returned, when more follow.
    pub(crate) next: Option<String>,
}

/// What a listing asks for.
pub(crate) struct Query<'a> {
    pub(crate) bucket: &'a str,
    pub(crate) prefix: &'a str,
    pub(crate) delimiter: Option<&'a str>,
    /// Keys at or before this are skipped (a prefix that ends in the delimiter skips everything under it).
    pub(crate) after: &'a str,
    pub(crate) max: usize,
}

/// The key a row key holds.
pub(crate) fn key_of<'r>(row: &'r str, bucket: &str) -> Option<&'r str> {
    let rest = row.strip_prefix(bucket)?.strip_prefix('\0')?;
    rest.split('\0').next()
}

/// The folded prefix of `key` under `q`, if it has the delimiter after the prefix.
pub(crate) fn fold(q: &Query<'_>, key: &str) -> Option<String> {
    let delim = q.delimiter.filter(|d| !d.is_empty())?;
    let rest = key.strip_prefix(q.prefix)?;
    let i = rest.find(delim)?;
    Some(format!("{}{}", q.prefix, rest.get(..i + delim.len())?))
}

impl Core {
    /// Walks the keys of a bucket in order from the first row after `after`, calling `each(key, row_bytes)` for every row whose key
    /// starts with the prefix; `each` says whether to go on.
    pub(crate) fn walk(
        &self,
        q: &Query<'_>,
        each: impl FnMut(&str, &[u8]) -> bool,
    ) -> Result<(), ApiError> {
        let start = if q.after.is_empty() || q.after < q.prefix {
            format!("{}\0{}", q.bucket, q.prefix)
        } else {
            format!("{}\0{}\u{1}", q.bucket, q.after)
        };
        self.walk_from(q, &start, each)
    }

    /// [`walk`](Self::walk) from an explicit row.
    pub(crate) fn walk_from(
        &self,
        q: &Query<'_>,
        start: &str,
        mut each: impl FnMut(&str, &[u8]) -> bool,
    ) -> Result<(), ApiError> {
        self.db
            .scan(VERSIONS, start, |row, bytes| {
                let Some(key) = key_of(row, q.bucket) else {
                    return false;
                };
                if !key.starts_with(q.prefix) {
                    return key < q.prefix;
                }
                each(key, bytes)
            })
            .map_err(|e| ApiError::internal(format!("the object store failed: {e}")))?;
        Ok(())
    }

    /// The page of newest versions described by `q`.
    pub(crate) fn page(&self, q: &Query<'_>) -> Result<Page, ApiError> {
        let mut entries: Vec<Entry> = Vec::new();
        let mut last_key: Option<String> = None;
        let mut last_fold: Option<String> = None;
        let mut truncated = false;
        let skip_under = q
            .delimiter
            .filter(|d| q.after.ends_with(d))
            .map(|_| q.after);
        let mut bad = false;
        self.walk(q, |key, bytes| {
            if last_key.as_deref() == Some(key) || skip_under.is_some_and(|s| key.starts_with(s)) {
                return true;
            }
            last_key = Some(key.to_owned());
            if let Some(f) = fold(q, key) {
                if last_fold.as_deref() == Some(f.as_str()) {
                    return true;
                }
                if entries.len() >= q.max {
                    truncated = true;
                    return false;
                }
                last_fold = Some(f.clone());
                entries.push(Entry::Prefix(f));
                return true;
            }
            let Ok(v) = record::decode::<Version>("version", VERSION, bytes) else {
                bad = true;
                return false;
            };
            if v.delete_marker {
                return true;
            }
            if entries.len() >= q.max {
                truncated = true;
                return false;
            }
            entries.push(Entry::Object(Box::new(v)));
            true
        })?;
        if bad {
            return Err(ApiError::internal("a stored object could not be read"));
        }
        let next = truncated
            .then(|| match entries.last() {
                Some(Entry::Object(v)) => Some(v.key.clone()),
                Some(Entry::Prefix(p)) => Some(p.clone()),
                None => None,
            })
            .flatten();
        Ok(Page { entries, next })
    }

    pub(crate) fn list_objects(&self, req: &Req<'_>, v2: bool) -> Answer {
        let bucket = req.bucket();
        self.require(bucket)?;
        let prefix = req.s("Prefix").unwrap_or_default();
        let delimiter = req.s("Delimiter").filter(|d| !d.is_empty());
        let max = req
            .int("MaxKeys")
            .map_or(MAX_KEYS, |m| usize::try_from(m).unwrap_or(0).min(MAX_KEYS));
        let token = req.s("ContinuationToken").and_then(decode_token);
        let after = if v2 {
            token
                .clone()
                .or_else(|| req.s("StartAfter").map(str::to_owned))
        } else {
            req.s("Marker").map(str::to_owned)
        };
        let q = Query {
            bucket,
            prefix,
            delimiter,
            after: after.as_deref().unwrap_or_default(),
            max,
        };
        let page = if max == 0 {
            Page {
                entries: Vec::new(),
                next: None,
            }
        } else {
            self.page(&q)?
        };
        let enc = Enc::of(req);
        let mut out = Map::new();
        out.insert("Name".into(), json!(bucket));
        out.insert("Prefix".into(), json!(enc.text(prefix)));
        out.insert("MaxKeys".into(), json!(max));
        out.insert("IsTruncated".into(), json!(page.next.is_some()));
        if let Some(d) = delimiter {
            out.insert("Delimiter".into(), json!(enc.text(d)));
        }
        if req.s("EncodingType").is_some() {
            out.insert("EncodingType".into(), json!("url"));
        }
        let (contents, prefixes) = split(
            &page,
            enc,
            !v2 || req.flag("FetchOwner") == Some(true),
            req.account(),
        );
        out.insert("Contents".into(), Value::Array(contents));
        out.insert("CommonPrefixes".into(), Value::Array(prefixes));
        if v2 {
            v2_fields(req, &page, enc, &mut out);
        } else {
            out.insert(
                "Marker".into(),
                json!(enc.text(req.s("Marker").unwrap_or_default())),
            );
            if let (Some(next), Some(_)) = (&page.next, delimiter) {
                out.insert("NextMarker".into(), json!(enc.text(next)));
            }
        }
        Out::json(Value::Object(out))
    }
}

fn v2_fields(req: &Req<'_>, page: &Page, enc: Enc, out: &mut Map<String, Value>) {
    let count = page.entries.len();
    out.insert("KeyCount".into(), json!(count));
    if let Some(t) = req.s("ContinuationToken") {
        out.insert("ContinuationToken".into(), json!(t));
    }
    if let Some(next) = &page.next {
        out.insert("NextContinuationToken".into(), json!(encode_token(next)));
    }
    if let Some(s) = req.s("StartAfter") {
        out.insert("StartAfter".into(), json!(enc.text(s)));
    }
}

fn split(page: &Page, enc: Enc, with_owner: bool, account: &str) -> (Vec<Value>, Vec<Value>) {
    let mut contents = Vec::new();
    let mut prefixes = Vec::new();
    for e in &page.entries {
        match e {
            Entry::Prefix(p) => prefixes.push(json!({"Prefix": enc.text(p)})),
            Entry::Object(v) => {
                let mut o = Map::new();
                o.insert("Key".into(), json!(enc.text(&v.key)));
                o.insert("LastModified".into(), json!(v.modified_ms / 1000));
                o.insert("ETag".into(), json!(format!("\"{}\"", v.etag)));
                o.insert("Size".into(), json!(v.size));
                o.insert(
                    "StorageClass".into(),
                    json!(v.meta.storage_class.as_deref().unwrap_or("STANDARD")),
                );
                if let Some(c) = &v.checksum {
                    o.insert("ChecksumAlgorithm".into(), json!(algo_name(Some(c))));
                    o.insert(
                        "ChecksumType".into(),
                        json!(match c.kind {
                            crate::model::CKind::FullObject => "FULL_OBJECT",
                            crate::model::CKind::Composite => "COMPOSITE",
                        }),
                    );
                }
                if with_owner {
                    o.insert("Owner".into(), owner(account));
                }
                contents.push(Value::Object(o));
            }
        }
    }
    (contents, prefixes)
}

fn encode_token(s: &str) -> String {
    aws_core::base64::encode(s.as_bytes())
}

fn decode_token(t: &str) -> Option<String> {
    aws_core::base64::decode(t).and_then(|b| String::from_utf8(b).ok())
}

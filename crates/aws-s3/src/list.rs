//! ListObjects and ListObjectsV2: the newest version of each key, in key order, cut at a prefix, folded at a delimiter, and paged.

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::buckets::owner;
use crate::core::{Core, VERSIONS};
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
    /// The rows read to make it.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) scanned: usize,
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

    /// The page of newest versions described by `q`. Under a delimiter, the keys a folded prefix holds are not read: after a few
    /// rows of one fold the walk starts again past the fold (a bucket with 100,000 keys under one prefix is one row, not 100,000).
    pub(crate) fn page(&self, q: &Query<'_>) -> Result<Page, ApiError> {
        let skip_under = q
            .delimiter
            .filter(|d| q.after.ends_with(d))
            .map(|_| q.after);
        let mut p = Paging::default();
        let mut start = match skip_under {
            Some(fold) => past(q.bucket, fold),
            None if q.after.is_empty() || q.after < q.prefix => {
                format!("{}\0{}", q.bucket, q.prefix)
            }
            None => format!("{}\0{}\u{1}", q.bucket, q.after),
        };
        loop {
            self.walk_from(q, &start, |key, bytes| {
                p.scanned += 1;
                p.take(q, skip_under, key, bytes)
            })?;
            match p.restart.take() {
                Some(fold) => start = past(q.bucket, &fold),
                None => break,
            }
        }
        if p.bad {
            return Err(ApiError::internal("a stored object could not be read"));
        }
        let next = p
            .truncated
            .then(|| match p.entries.last() {
                Some(Entry::Object(v)) => Some(v.key.clone()),
                Some(Entry::Prefix(f)) => Some(f.clone()),
                None => None,
            })
            .flatten();
        Ok(Page {
            entries: p.entries,
            next,
            scanned: p.scanned,
        })
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
                scanned: 0,
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

/// Rows of one folded prefix read before the walk starts again past it: a fold of a few keys is cheaper to read through than to
/// seek past.
pub(crate) const FOLD_ROWS: usize = 16;

/// The row key just past every key under `fold` (a key that has U+10FFFF right after the fold sorts after it; the fold is then
/// met again and skipped as one already returned).
pub(crate) fn past(bucket: &str, fold: &str) -> String {
    format!("{bucket}\0{fold}\u{10FFFF}")
}

/// A page being gathered.
#[derive(Default)]
struct Paging {
    entries: Vec<Entry>,
    last_key: Option<String>,
    last_fold: Option<String>,
    /// Rows of the current fold read since it was returned.
    in_fold: usize,
    truncated: bool,
    bad: bool,
    /// The fold to start the walk again past.
    restart: Option<String>,
    /// The fold the walk last started again past.
    restarted: Option<String>,
    scanned: usize,
}

impl Paging {
    /// Takes one row; answers whether to go on.
    fn take(&mut self, q: &Query<'_>, skip_under: Option<&str>, key: &str, bytes: &[u8]) -> bool {
        if self.last_key.as_deref() == Some(key) || skip_under.is_some_and(|s| key.starts_with(s)) {
            return true;
        }
        self.last_key = Some(key.to_owned());
        if let Some(f) = fold(q, key) {
            if self.last_fold.as_deref() == Some(f.as_str()) {
                self.in_fold += 1;
                // Once per fold: keys past the restart point that still fold here are read through.
                if self.in_fold >= FOLD_ROWS && self.restarted.as_deref() != Some(f.as_str()) {
                    self.restarted = Some(f.clone());
                    self.restart = Some(f);
                    return false;
                }
                return true;
            }
            if self.entries.len() >= q.max {
                self.truncated = true;
                return false;
            }
            self.last_fold = Some(f.clone());
            self.in_fold = 0;
            self.entries.push(Entry::Prefix(f));
            return true;
        }
        let Ok(v) = crate::store::decode_version(bytes) else {
            self.bad = true;
            return false;
        };
        if v.delete_marker {
            return true;
        }
        if self.entries.len() >= q.max {
            self.truncated = true;
            return false;
        }
        self.entries.push(Entry::Object(Box::new(v)));
        true
    }
}

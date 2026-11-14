//! The BSON the module speaks to FerretDB: commands out, replies in. Written here rather than taken from the `bson`
//! crate (PLAN.md D6, measured 2026-11-14 in docs/port/weight.md): a module needs seven element types to write and must
//! only step over the rest when it reads, and the crate cost 447 KB and 42 packages for that.
//!
//! No dependencies, so the fuzz target includes this file by path. Reading never panics, never allocates more than the
//! input's length, and stops at `MAX_DEPTH` nested documents.

// fuzz:parser fuzz_bson

/// One BSON value. The types a reply may carry that the module never looks into are kept as their raw bytes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Bson {
    Double(f64),
    String(String),
    Doc(Doc),
    Array(Vec<Bson>),
    Bool(bool),
    Null,
    Int32(i32),
    Int64(i64),
    /// Any other element (ObjectId, date, binary, decimal, …): its type byte and its bytes as they were.
    Other(u8, Vec<u8>),
}

/// A document: its fields in order (BSON's order is meaningful: a command's name comes first).
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Doc(pub(crate) Vec<(String, Bson)>);

/// Why bytes could not be read as BSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Malformed(pub(crate) &'static str);

impl std::fmt::Display for Malformed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "malformed BSON: {}", self.0)
    }
}

impl std::error::Error for Malformed {}

/// How deep documents and arrays may nest when read (MongoDB's own limit is 100).
pub(crate) const MAX_DEPTH: usize = 100;

impl Doc {
    /// An empty document.
    pub(crate) fn new() -> Doc {
        Doc(Vec::new())
    }

    /// The document with one more field.
    pub(crate) fn with(mut self, key: &str, value: impl Into<Bson>) -> Doc {
        self.0.push((key.to_string(), value.into()));
        self
    }

    /// The first field named `key`.
    pub(crate) fn get(&self, key: &str) -> Option<&Bson> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// A number field as an integer, whatever width it was sent in (servers answer `ok: 1.0` and `n: 1` alike).
    pub(crate) fn int(&self, key: &str) -> Option<i64> {
        match self.get(key)? {
            Bson::Int32(n) => Some(i64::from(*n)),
            Bson::Int64(n) => Some(*n),
            Bson::Double(d) if d.fract() == 0.0 && d.abs() < 9.0e15 => Some(*d as i64),
            _ => None,
        }
    }

    /// A string field.
    pub(crate) fn str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Bson::String(s) => Some(s),
            _ => None,
        }
    }

    /// A document field.
    #[allow(dead_code, reason = "used by Client::all")]
    pub(crate) fn doc(&self, key: &str) -> Option<&Doc> {
        match self.get(key)? {
            Bson::Doc(d) => Some(d),
            _ => None,
        }
    }

    /// An array field.
    pub(crate) fn array(&self, key: &str) -> Option<&[Bson]> {
        match self.get(key)? {
            Bson::Array(a) => Some(a),
            _ => None,
        }
    }

    /// The document as BSON bytes.
    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        write_doc(&mut out, self.0.iter().map(|(k, v)| (k.as_str(), v)));
        out
    }

    /// Reads one document that fills `bytes` exactly.
    pub(crate) fn from_bytes(bytes: &[u8]) -> Result<Doc, Malformed> {
        let mut r = Reader { rest: bytes };
        let doc = r.doc(0)?;
        if !r.rest.is_empty() {
            return Err(Malformed("bytes after the document"));
        }
        Ok(doc)
    }
}

impl From<&str> for Bson {
    fn from(s: &str) -> Bson {
        Bson::String(s.to_string())
    }
}
impl From<String> for Bson {
    fn from(s: String) -> Bson {
        Bson::String(s)
    }
}
impl From<i32> for Bson {
    fn from(n: i32) -> Bson {
        Bson::Int32(n)
    }
}
impl From<bool> for Bson {
    fn from(b: bool) -> Bson {
        Bson::Bool(b)
    }
}
impl From<Doc> for Bson {
    fn from(d: Doc) -> Bson {
        Bson::Doc(d)
    }
}
impl From<Vec<Bson>> for Bson {
    fn from(a: Vec<Bson>) -> Bson {
        Bson::Array(a)
    }
}

fn write_doc<'a>(out: &mut Vec<u8>, fields: impl Iterator<Item = (&'a str, &'a Bson)>) {
    let start = out.len();
    out.extend_from_slice(&[0; 4]);
    for (key, value) in fields {
        out.push(type_byte(value));
        write_cstring(out, key);
        write_value(out, value);
    }
    out.push(0);
    let len = i32::try_from(out.len() - start)
        .unwrap_or(i32::MAX)
        .to_le_bytes();
    if let Some(head) = out.get_mut(start..start + 4) {
        head.copy_from_slice(&len);
    }
}

/// Keys are C strings: a NUL inside one would end it early, so it is left out rather than sent.
fn write_cstring(out: &mut Vec<u8>, s: &str) {
    out.extend(s.bytes().filter(|b| *b != 0));
    out.push(0);
}

fn type_byte(v: &Bson) -> u8 {
    match v {
        Bson::Double(_) => 0x01,
        Bson::String(_) => 0x02,
        Bson::Doc(_) => 0x03,
        Bson::Array(_) => 0x04,
        Bson::Bool(_) => 0x08,
        Bson::Null => 0x0A,
        Bson::Int32(_) => 0x10,
        Bson::Int64(_) => 0x12,
        Bson::Other(t, _) => *t,
    }
}

fn write_value(out: &mut Vec<u8>, v: &Bson) {
    match v {
        Bson::Double(d) => out.extend_from_slice(&d.to_le_bytes()),
        Bson::String(s) => {
            let len = i32::try_from(s.len() + 1).unwrap_or(i32::MAX);
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(s.as_bytes());
            out.push(0);
        }
        Bson::Doc(d) => write_doc(out, d.0.iter().map(|(k, v)| (k.as_str(), v))),
        Bson::Array(a) => {
            let keys: Vec<String> = (0..a.len()).map(|i| i.to_string()).collect();
            write_doc(out, keys.iter().map(String::as_str).zip(a.iter()));
        }
        Bson::Bool(b) => out.push(u8::from(*b)),
        Bson::Null => {}
        Bson::Int32(n) => out.extend_from_slice(&n.to_le_bytes()),
        Bson::Int64(n) => out.extend_from_slice(&n.to_le_bytes()),
        Bson::Other(_, raw) => out.extend_from_slice(raw),
    }
}

struct Reader<'a> {
    rest: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Malformed> {
        if n > self.rest.len() {
            return Err(Malformed("cut short"));
        }
        let (head, tail) = self.rest.split_at(n);
        self.rest = tail;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], Malformed> {
        let mut a = [0; N];
        a.copy_from_slice(self.take(N)?);
        Ok(a)
    }

    fn i32(&mut self) -> Result<i32, Malformed> {
        Ok(i32::from_le_bytes(self.array()?))
    }

    /// A length that must be at least `min` and fit in what is left.
    fn len(&mut self, min: usize) -> Result<usize, Malformed> {
        let n = usize::try_from(self.i32()?).map_err(|_| Malformed("negative length"))?;
        if n < min || n > self.rest.len() + 4 {
            return Err(Malformed("length out of range"));
        }
        Ok(n)
    }

    fn cstring(&mut self) -> Result<String, Malformed> {
        let end = self
            .rest
            .iter()
            .position(|b| *b == 0)
            .ok_or(Malformed("unterminated key"))?;
        let s = std::str::from_utf8(self.take(end)?).map_err(|_| Malformed("key is not UTF-8"))?;
        self.take(1)?;
        Ok(s.to_string())
    }

    fn doc(&mut self, depth: usize) -> Result<Doc, Malformed> {
        if depth >= MAX_DEPTH {
            return Err(Malformed("nested too deep"));
        }
        let len = self.len(5)?;
        let body = self.take(len - 4)?;
        let (fields, end) = body.split_at(body.len() - 1);
        if end != [0] {
            return Err(Malformed("document not terminated"));
        }
        let mut inner = Reader { rest: fields };
        let mut doc = Doc::new();
        while !inner.rest.is_empty() {
            let t = inner.array::<1>()?;
            let key = inner.cstring()?;
            let value = inner.value(t[0], depth)?;
            doc.0.push((key, value));
        }
        Ok(doc)
    }

    fn value(&mut self, t: u8, depth: usize) -> Result<Bson, Malformed> {
        Ok(match t {
            0x01 => Bson::Double(f64::from_le_bytes(self.array()?)),
            0x02 => Bson::String(self.string()?),
            0x03 => Bson::Doc(self.doc(depth + 1)?),
            0x04 => Bson::Array(self.doc(depth + 1)?.0.into_iter().map(|(_, v)| v).collect()),
            0x08 => match self.array::<1>()? {
                [0] => Bson::Bool(false),
                [1] => Bson::Bool(true),
                _ => return Err(Malformed("bool is neither 0 nor 1")),
            },
            0x0A => Bson::Null,
            0x10 => Bson::Int32(self.i32()?),
            0x12 => Bson::Int64(i64::from_le_bytes(self.array()?)),
            other => Bson::Other(other, self.raw(other, depth)?),
        })
    }

    fn string(&mut self) -> Result<String, Malformed> {
        let len = self.len(1)?;
        let bytes = self.take(len)?;
        let (text, nul) = bytes.split_at(len - 1);
        if nul != [0] {
            return Err(Malformed("string not terminated"));
        }
        String::from_utf8(text.to_vec()).map_err(|_| Malformed("string is not UTF-8"))
    }

    /// The bytes of an element the module does not look into, so the reader can step over it.
    fn raw(&mut self, t: u8, depth: usize) -> Result<Vec<u8>, Malformed> {
        let start = self.rest;
        match t {
            0x06 | 0x7F | 0xFF => {}            // undefined, max key, min key
            0x07 => drop(self.take(12)?),       // ObjectId
            0x09 | 0x11 => drop(self.take(8)?), // date, timestamp
            0x13 => drop(self.take(16)?),       // decimal128
            0x0B => drop((self.cstring()?, self.cstring()?)), // regex
            0x0D | 0x0E => drop(self.string()?), // code, symbol
            0x05 => {
                let n = self.len(0)?;
                self.take(n.saturating_add(1))?; // subtype, then the bytes
            }
            0x0C => {
                self.string()?;
                self.take(12)?; // DBPointer
            }
            0x0F => {
                let len = self.len(14)?;
                let mut inner = Reader {
                    rest: self.take(len - 4)?,
                };
                inner.string()?;
                inner.doc(depth + 1)?;
                if !inner.rest.is_empty() {
                    return Err(Malformed("code with scope has bytes left"));
                }
            }
            _ => return Err(Malformed("unknown element type")),
        }
        let used = start.len() - self.rest.len();
        Ok(start.get(..used).unwrap_or_default().to_vec())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn a_hello_is_the_bytes_the_spec_gives() {
        // {"hello": 1} from the BSON spec's own examples, shape-wise: 4-byte length, 0x10, key, i32, NUL.
        let b = Doc::new().with("hello", 1).to_bytes();
        assert_eq!(
            b,
            [
                16, 0, 0, 0, 0x10, b'h', b'e', b'l', b'l', b'o', 0, 1, 0, 0, 0, 0
            ]
        );
        assert_eq!(Doc::from_bytes(&b).unwrap(), Doc::new().with("hello", 1));
    }

    #[test]
    fn every_written_type_reads_back() {
        let d = Doc::new()
            .with("d", Bson::Double(1.5))
            .with("s", "é")
            .with("n", Bson::Null)
            .with("b", true)
            .with("l", Bson::Int64(1 << 40))
            .with(
                "a",
                vec![
                    Bson::Int32(1),
                    Bson::from("x"),
                    Bson::Doc(Doc::new().with("k", 2)),
                ],
            );
        assert_eq!(Doc::from_bytes(&d.to_bytes()).unwrap(), d);
    }

    #[test]
    fn foreign_types_are_stepped_over_and_kept() {
        let mut oid = Doc::new()
            .with("_id", Bson::Other(0x07, vec![7; 12]))
            .to_bytes();
        assert_eq!(
            Doc::from_bytes(&oid).unwrap().0[0].1,
            Bson::Other(0x07, vec![7; 12])
        );
        oid.truncate(oid.len() - 3);
        assert!(Doc::from_bytes(&oid).is_err());
    }

    #[test]
    fn bad_input_is_refused_without_a_panic() {
        assert!(Doc::from_bytes(&[]).is_err());
        assert!(Doc::from_bytes(&[5, 0, 0, 0, 1]).is_err());
        assert!(Doc::from_bytes(&[255, 255, 255, 255, 0]).is_err());
        let mut deep = Doc::new();
        for _ in 0..MAX_DEPTH + 1 {
            deep = Doc::new().with("x", deep);
        }
        assert_eq!(
            Doc::from_bytes(&deep.to_bytes()),
            Err(Malformed("nested too deep"))
        );
    }

    #[test]
    fn numbers_read_as_integers_whatever_their_width() {
        let d = Doc::new()
            .with("ok", Bson::Double(1.0))
            .with("n", Bson::Int64(3))
            .with("x", Bson::Double(0.5));
        assert_eq!(
            (d.int("ok"), d.int("n"), d.int("x")),
            (Some(1), Some(3), None)
        );
    }
}

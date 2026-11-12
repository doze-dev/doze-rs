//! Building a value tree: the response side of the typed views.
//!
//! A service writes its answer with the same generated `Key` constants it reads its request with, so a key of another
//! shape, or a value of the wrong kind for a key, does not compile:
//!
//! ```text
//! let mut out = Object::<SendMessageResponse>::root(&mut arena);
//! out.put(SendMessageResponse::MESSAGE_ID, id).put(SendMessageResponse::MD5_OF_MESSAGE_BODY, md5);
//! let root = out.finish();
//! ```
//!
//! The tree is protocol-neutral: `enc` writes it as JSON, and the other protocols write the same tree. Strings are copied once
//! into the arena's text, blobs into its byte buffer; nothing else allocates once the arena has grown.

use std::marker::PhantomData;

use crate::arena::{Arena, NIL, Node, Val};
use crate::view::Key;
use crate::view::kind::{Blob, Bool, Float, Int, List, Map, Obj, Str, Time};

/// Where the next child of an open container goes.
#[derive(Debug)]
struct Chain {
    first: u32,
    last: u32,
}

impl Chain {
    const fn new() -> Self {
        Self {
            first: NIL,
            last: NIL,
        }
    }

    fn push(&mut self, ar: &mut Arena, member: u32, val: Val) -> u32 {
        let ix = ar.push(Node {
            next: NIL,
            member,
            val,
        });
        if self.first == NIL {
            self.first = ix;
        } else {
            ar.link(self.last, ix);
        }
        self.last = ix;
        ix
    }
}

/// An open structure or union of shape `S`.
#[derive(Debug)]
pub struct Object<'a, S> {
    ar: &'a mut Arena,
    chain: Chain,
    _s: PhantomData<fn() -> S>,
}

impl<'a, S> Object<'a, S> {
    /// Starts the root object of an answer.
    pub fn root(ar: &'a mut Arena) -> Self {
        Self {
            ar,
            chain: Chain::new(),
            _s: PhantomData,
        }
    }

    /// Closes the object and returns the root node `enc::encode` takes.
    #[must_use]
    pub fn finish(self) -> u32 {
        self.ar.push(Node {
            next: NIL,
            member: NIL,
            val: Val::Obj(self.chain.first),
        })
    }

    fn add<K>(&mut self, key: Key<S, K>, val: Val) -> &mut Self {
        self.chain.push(self.ar, key.member(), val);
        self
    }

    /// A string member.
    pub fn put(&mut self, key: Key<S, Str>, value: &str) -> &mut Self {
        let v = self.ar.push_text(value);
        self.add(key, v)
    }

    /// An integer member.
    pub fn put_int(&mut self, key: Key<S, Int>, value: i64) -> &mut Self {
        self.add(key, Val::Int(value))
    }

    /// A floating-point member.
    pub fn put_float(&mut self, key: Key<S, Float>, value: f64) -> &mut Self {
        self.add(key, Val::Float(value))
    }

    /// A timestamp member, in seconds since the epoch (fractions allowed).
    pub fn put_time(&mut self, key: Key<S, Time>, epoch_seconds: f64) -> &mut Self {
        self.add(key, Val::Float(epoch_seconds))
    }

    /// A boolean member.
    pub fn put_bool(&mut self, key: Key<S, Bool>, value: bool) -> &mut Self {
        self.add(key, Val::Bool(value))
    }

    /// A blob member.
    pub fn put_blob(&mut self, key: Key<S, Blob>, value: &[u8]) -> &mut Self {
        let off = u32::try_from(self.ar.bin.len()).unwrap_or(0);
        self.ar.bin.extend_from_slice(value);
        let len = u32::try_from(value.len()).unwrap_or(0);
        self.add(key, Val::Blob(off, len))
    }

    /// A nested structure member, filled in by `fill`.
    pub fn put_object<T>(
        &mut self,
        key: Key<S, Obj<T>>,
        fill: impl FnOnce(&mut Object<'_, T>),
    ) -> &mut Self {
        let first = {
            let mut child = Object::<T> {
                ar: self.ar,
                chain: Chain::new(),
                _s: PhantomData,
            };
            fill(&mut child);
            child.chain.first
        };
        self.add(key, Val::Obj(first))
    }

    /// A list member, filled in by `fill`.
    pub fn put_list<K>(
        &mut self,
        key: Key<S, List<K>>,
        fill: impl FnOnce(&mut Items<'_, K>),
    ) -> &mut Self {
        let first = {
            let mut items = Items::<K> {
                ar: self.ar,
                chain: Chain::new(),
                _k: PhantomData,
            };
            fill(&mut items);
            items.chain.first
        };
        self.add(key, Val::Seq(first))
    }

    /// A map member, filled in by `fill`.
    pub fn put_map<K>(
        &mut self,
        key: Key<S, Map<K>>,
        fill: impl FnOnce(&mut Entries<'_, K>),
    ) -> &mut Self {
        let first = {
            let mut entries = Entries::<K> {
                ar: self.ar,
                chain: Chain::new(),
                _k: PhantomData,
            };
            fill(&mut entries);
            entries.chain.first
        };
        self.add(key, Val::Seq(first))
    }
}

/// An open list whose elements have kind `K`.
#[derive(Debug)]
pub struct Items<'a, K> {
    ar: &'a mut Arena,
    chain: Chain,
    _k: PhantomData<fn() -> K>,
}

impl Items<'_, Str> {
    /// Appends a string.
    pub fn push(&mut self, value: &str) -> &mut Self {
        let v = self.ar.push_text(value);
        self.chain.push(self.ar, NIL, v);
        self
    }
}

impl Items<'_, Int> {
    /// Appends an integer.
    pub fn push(&mut self, value: i64) -> &mut Self {
        self.chain.push(self.ar, NIL, Val::Int(value));
        self
    }
}

impl Items<'_, Blob> {
    /// Appends a blob.
    pub fn push(&mut self, value: &[u8]) -> &mut Self {
        let off = u32::try_from(self.ar.bin.len()).unwrap_or(0);
        self.ar.bin.extend_from_slice(value);
        let len = u32::try_from(value.len()).unwrap_or(0);
        self.chain.push(self.ar, NIL, Val::Blob(off, len));
        self
    }
}

impl Items<'_, Bool> {
    /// Appends a boolean.
    pub fn push(&mut self, value: bool) -> &mut Self {
        self.chain.push(self.ar, NIL, Val::Bool(value));
        self
    }
}

impl<T> Items<'_, Obj<T>> {
    /// Appends a structure, filled in by `fill`.
    pub fn push(&mut self, fill: impl FnOnce(&mut Object<'_, T>)) -> &mut Self {
        let first = {
            let mut child = Object::<T> {
                ar: self.ar,
                chain: Chain::new(),
                _s: PhantomData,
            };
            fill(&mut child);
            child.chain.first
        };
        self.chain.push(self.ar, NIL, Val::Obj(first));
        self
    }
}

/// An open map whose values have kind `K`. Entries are written in the order given.
#[derive(Debug)]
pub struct Entries<'a, K> {
    ar: &'a mut Arena,
    chain: Chain,
    _k: PhantomData<fn() -> K>,
}

impl<K> Entries<'_, K> {
    fn entry(&mut self, name: &str, val: Val) -> &mut Self {
        let k = self.ar.push_text(name);
        self.chain.push(self.ar, NIL, k);
        self.chain.push(self.ar, NIL, val);
        self
    }
}

impl Entries<'_, Str> {
    /// Adds `name` with a string value.
    pub fn insert(&mut self, name: &str, value: &str) -> &mut Self {
        let v = self.ar.push_text(value);
        self.entry(name, v)
    }
}

impl Entries<'_, Int> {
    /// Adds `name` with an integer value.
    pub fn insert(&mut self, name: &str, value: i64) -> &mut Self {
        self.entry(name, Val::Int(value))
    }
}

impl<T> Entries<'_, Obj<T>> {
    /// Adds `name` with a structure value, filled in by `fill`.
    pub fn insert(&mut self, name: &str, fill: impl FnOnce(&mut Object<'_, T>)) -> &mut Self {
        let first = {
            let mut child = Object::<T> {
                ar: self.ar,
                chain: Chain::new(),
                _s: PhantomData,
            };
            fill(&mut child);
            child.chain.first
        };
        self.entry(name, Val::Obj(first))
    }
}

impl<K> Entries<'_, List<K>> {
    /// Adds `name` with a list value, filled in by `fill`.
    pub fn insert(&mut self, name: &str, fill: impl FnOnce(&mut Items<'_, K>)) -> &mut Self {
        let first = {
            let mut items = Items::<K> {
                ar: self.ar,
                chain: Chain::new(),
                _k: PhantomData,
            };
            fill(&mut items);
            items.chain.first
        };
        self.entry(name, Val::Seq(first))
    }
}

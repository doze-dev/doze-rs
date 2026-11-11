//! Typed, borrowing views over the tree. A view is two references and an index; a `Key` is a member index tagged with the shape it
//! belongs to and the kind of value it holds, so `view.get(SendMessageRequest::QUEUE_URL)` is a `&str` and a key of another
//! shape does not compile. There is no code per member: the generated `keys.rs` is constants.

use crate::arena::{Arena, NIL, Val};
use std::marker::PhantomData;

pub mod kind {
    use std::marker::PhantomData;
    #[derive(Clone, Copy, Debug)]
    pub struct Bool;
    #[derive(Clone, Copy, Debug)]
    pub struct Int;
    #[derive(Clone, Copy, Debug)]
    pub struct Float;
    #[derive(Clone, Copy, Debug)]
    pub struct Str;
    #[derive(Clone, Copy, Debug)]
    pub struct Blob;
    #[derive(Clone, Copy, Debug)]
    pub struct Time;
    #[derive(Clone, Copy, Debug)]
    pub struct Other;
    #[derive(Clone, Copy, Debug)]
    pub struct Obj<S>(PhantomData<S>);
    #[derive(Clone, Copy, Debug)]
    pub struct List<K>(PhantomData<K>);
    #[derive(Clone, Copy, Debug)]
    pub struct Map<K>(PhantomData<K>);
}

use kind::{Blob, Bool, Float, Int, List, Map, Obj, Other, Str, Time};

pub trait Shape {
    const INDEX: u32;
}

#[derive(Debug)]
pub struct Key<S, K> {
    member: u32,
    _m: PhantomData<fn() -> (S, K)>,
}

impl<S, K> Clone for Key<S, K> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<S, K> Copy for Key<S, K> {}

impl<S, K> Key<S, K> {
    #[must_use]
    pub const fn new(member: u32) -> Self {
        Self {
            member,
            _m: PhantomData,
        }
    }
}

/// What a value of kind `K` reads as.
pub trait Get<'t>: Sized {
    type Out;
    fn get(ar: &'t Arena, body: &'t str, v: Val) -> Option<Self::Out>;
}

impl<'t> Get<'t> for Str {
    type Out = &'t str;
    fn get(ar: &'t Arena, body: &'t str, v: Val) -> Option<&'t str> {
        ar.str_of(body, v)
    }
}
impl<'t> Get<'t> for Int {
    type Out = i64;
    fn get(_: &'t Arena, _: &'t str, v: Val) -> Option<i64> {
        if let Val::Int(n) = v { Some(n) } else { None }
    }
}
impl<'t> Get<'t> for Float {
    type Out = f64;
    fn get(_: &'t Arena, _: &'t str, v: Val) -> Option<f64> {
        match v {
            Val::Float(f) => Some(f),
            #[allow(clippy::cast_precision_loss)]
            Val::Int(n) => Some(n as f64),
            _ => None,
        }
    }
}
impl<'t> Get<'t> for Time {
    type Out = f64;
    fn get(ar: &'t Arena, body: &'t str, v: Val) -> Option<f64> {
        Float::get(ar, body, v)
    }
}
impl<'t> Get<'t> for Bool {
    type Out = bool;
    fn get(_: &'t Arena, _: &'t str, v: Val) -> Option<bool> {
        if let Val::Bool(b) = v { Some(b) } else { None }
    }
}
impl<'t> Get<'t> for Blob {
    type Out = &'t [u8];
    fn get(ar: &'t Arena, _: &'t str, v: Val) -> Option<&'t [u8]> {
        ar.bytes_of(v)
    }
}
impl<'t> Get<'t> for Other {
    type Out = ();
    fn get(_: &'t Arena, _: &'t str, _: Val) -> Option<()> {
        Some(())
    }
}
impl<'t, S> Get<'t> for Obj<S> {
    type Out = View<'t, S>;
    fn get(ar: &'t Arena, body: &'t str, v: Val) -> Option<View<'t, S>> {
        if let Val::Obj(first) = v {
            Some(View {
                ar,
                body,
                first,
                _s: PhantomData,
            })
        } else {
            None
        }
    }
}
impl<'t, K: Get<'t>> Get<'t> for List<K> {
    type Out = Items<'t, K>;
    fn get(ar: &'t Arena, body: &'t str, v: Val) -> Option<Items<'t, K>> {
        if let Val::Seq(first) = v {
            Some(Items {
                ar,
                body,
                next: first,
                _k: PhantomData,
            })
        } else {
            None
        }
    }
}
impl<'t, K: Get<'t>> Get<'t> for Map<K> {
    type Out = Pairs<'t, K>;
    fn get(ar: &'t Arena, body: &'t str, v: Val) -> Option<Pairs<'t, K>> {
        if let Val::Seq(first) = v {
            Some(Pairs {
                ar,
                body,
                next: first,
                _k: PhantomData,
            })
        } else {
            None
        }
    }
}

#[derive(Debug)]
pub struct Items<'t, K> {
    ar: &'t Arena,
    body: &'t str,
    next: u32,
    _k: PhantomData<fn() -> K>,
}

impl<'t, K: Get<'t>> Iterator for Items<'t, K> {
    type Item = K::Out;
    fn next(&mut self) -> Option<K::Out> {
        let n = self.ar.node(self.next).filter(|_| self.next != NIL)?;
        self.next = n.next;
        K::get(self.ar, self.body, n.val)
    }
}

#[derive(Debug)]
pub struct Pairs<'t, K> {
    ar: &'t Arena,
    body: &'t str,
    next: u32,
    _k: PhantomData<fn() -> K>,
}

impl<'t, K: Get<'t>> Iterator for Pairs<'t, K> {
    type Item = (&'t str, K::Out);
    fn next(&mut self) -> Option<(&'t str, K::Out)> {
        let k = self.ar.node(self.next).filter(|_| self.next != NIL)?;
        let v = self.ar.node(k.next)?;
        self.next = v.next;
        Some((
            self.ar.str_of(self.body, k.val)?,
            K::get(self.ar, self.body, v.val)?,
        ))
    }
}

#[derive(Debug)]
pub struct View<'t, S> {
    ar: &'t Arena,
    body: &'t str,
    first: u32,
    _s: PhantomData<fn() -> S>,
}

impl<S> Clone for View<'_, S> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<S> Copy for View<'_, S> {}

impl<'t, S: Shape> View<'t, S> {
    /// The root returned by `json::decode` for the shape `S`.
    #[must_use]
    pub fn root(ar: &'t Arena, body: &'t str, root: u32) -> Option<Self> {
        match ar.node(root)?.val {
            Val::Obj(first) => Some(Self {
                ar,
                body,
                first,
                _s: PhantomData,
            }),
            _ => None,
        }
    }

    /// The member `key` names, if the request carried it. If a body repeated a member, the last one wins.
    #[must_use]
    pub fn get<K: Get<'t>>(&self, key: Key<S, K>) -> Option<K::Out> {
        let mut at = self.first;
        let mut found = None;
        while let Some(n) = self.ar.node(at).filter(|_| at != NIL) {
            if n.member == key.member {
                found = Some(n.val);
            }
            at = n.next;
        }
        K::get(self.ar, self.body, found?)
    }
}

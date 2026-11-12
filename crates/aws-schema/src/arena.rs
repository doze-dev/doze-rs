//! The value tree: a flat arena of nodes that refer to the request body by offset.
//!
//! A decode touches the allocator only when the arena grows, and not at all when the arena is cleared and reused for the next
//! request. A string with no escape is a range of the body (no copy). A string that had an escape, and a decoded blob, are the
//! two places a copy is needed: the decoded bytes exist nowhere else, so they are written once into `text` or `bin`.
//! The arena holds no lifetime because it holds no pointer; a `View` pairs it with the body to read.

pub const NIL: u32 = u32::MAX;

#[derive(Clone, Copy, Debug)]
pub enum Val {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    /// `body[off..off + len]`, no escapes.
    Str(u32, u32),
    /// A string that contained an escape: `text[off..off + len]`.
    Esc(u32, u32),
    /// A decoded blob: `bin[off..off + len]`.
    Blob(u32, u32),
    /// A list or map: the first child. A map's children alternate key (a string) and value.
    Seq(u32),
    /// A structure or union: the first child; each child records its member index.
    Obj(u32),
}

#[derive(Clone, Copy, Debug)]
pub struct Node {
    pub next: u32,
    pub member: u32,
    pub val: Val,
}

#[derive(Clone, Debug, Default)]
pub struct Arena {
    pub(crate) nodes: Vec<Node>,
    pub(crate) text: String,
    pub(crate) bin: Vec<u8>,
}

impl Arena {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            nodes: Vec::new(),
            text: String::new(),
            bin: Vec::new(),
        }
    }

    /// Forgets the tree and keeps the capacity, so the next request decodes without allocating.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.text.clear();
        self.bin.clear();
    }

    /// Sizes the node vector from the body, once, so a fresh arena does not grow by doubling. About one node per eight body bytes
    /// covers a dense request; the cap keeps a large body from asking for more than a few kilobytes up front.
    pub fn reserve_for(&mut self, body_len: usize) {
        self.nodes.reserve((body_len / 8).min(4096) + 8);
    }

    /// Makes the tree stand alone: every string that points into the body it was decoded from is copied into the arena's own text,
    /// so the tree can be written out after the body is gone (an answer a handler built as JSON and decoded into the output shape).
    pub fn detach(&mut self, body: &str) {
        for n in &mut self.nodes {
            if let Val::Str(o, l) = n.val {
                let s = body.get(o as usize..(o + l) as usize).unwrap_or_default();
                let at = u32::try_from(self.text.len()).unwrap_or(0);
                self.text.push_str(s);
                n.val = Val::Esc(at, l);
            }
        }
    }

    pub fn push(&mut self, n: Node) -> u32 {
        let i = u32::try_from(self.nodes.len()).unwrap_or(NIL - 1);
        self.nodes.push(n);
        i
    }

    #[must_use]
    pub fn node(&self, i: u32) -> Option<&Node> {
        self.nodes.get(i as usize)
    }

    pub(crate) fn link(&mut self, from: u32, to: u32) {
        if let Some(n) = self.nodes.get_mut(from as usize) {
            n.next = to;
        }
    }

    pub fn push_text(&mut self, s: &str) -> Val {
        let o = u32::try_from(self.text.len()).unwrap_or(0);
        self.text.push_str(s);
        Val::Esc(o, u32::try_from(s.len()).unwrap_or(0))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Bytes held, for the allocation report.
    #[must_use]
    pub fn held_bytes(&self) -> usize {
        self.nodes.capacity() * size_of::<Node>() + self.text.capacity() + self.bin.capacity()
    }

    /// The string a node holds, read against the body it was decoded from.
    #[must_use]
    pub fn str_of<'t>(&'t self, body: &'t str, v: Val) -> Option<&'t str> {
        match v {
            Val::Str(o, l) => body.get(o as usize..(o as usize) + (l as usize)),
            Val::Esc(o, l) => self.text.get(o as usize..(o as usize) + (l as usize)),
            _ => None,
        }
    }

    #[must_use]
    pub fn bytes_of(&self, v: Val) -> Option<&[u8]> {
        match v {
            Val::Blob(o, l) => self.bin.get(o as usize..(o as usize) + (l as usize)),
            _ => None,
        }
    }
}

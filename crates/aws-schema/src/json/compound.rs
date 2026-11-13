//! Lists, maps and structures, and the checks that need all of a structure's members.

use super::scalars::out_of_bounds;
use super::{D, MAX_DEPTH, R, Violation};
use crate::arena::{NIL, Node, Val};
use crate::tables::{ConsRef, F_REQUIRED, K_EVENT, K_STREAM, MemberRef, ShapeRef};

impl D<'_> {
    pub(super) fn list(
        &mut self,
        m: &MemberRef<'_>,
        mi: u32,
        c: Option<&ConsRef<'_>>,
        depth: u32,
    ) -> R<Val> {
        let t = self.t;
        let elem = t.member(m.target).ok_or(Violation::Table)?;
        self.expect(b'[')?;
        let (mut first, mut last, mut n) = (NIL, NIL, 0u64);
        self.ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
        } else {
            loop {
                let v = self.value(&elem, m.target, depth + 1)?;
                let ix = self.ar.push(Node {
                    next: NIL,
                    member: NIL,
                    val: v,
                });
                if first == NIL {
                    first = ix;
                } else {
                    self.ar.link(last, ix);
                }
                last = ix;
                n += 1;
                self.ws();
                match self.peek() {
                    Some(b',') => self.pos += 1,
                    Some(b']') => {
                        self.pos += 1;
                        break;
                    }
                    _ => return Err(Violation::Syntax { at: self.pos }),
                }
            }
        }
        if out_of_bounds(c, n) {
            return Err(Violation::Length {
                member: mi,
                value: None,
            });
        }
        Ok(Val::Seq(first))
    }

    pub(super) fn map(
        &mut self,
        m: &MemberRef<'_>,
        mi: u32,
        c: Option<&ConsRef<'_>>,
        depth: u32,
    ) -> R<Val> {
        let t = self.t;
        let key_m = t.member(m.target).ok_or(Violation::Table)?;
        let val_m = t.member(m.target + 1).ok_or(Violation::Table)?;
        self.expect(b'{')?;
        let (mut first, mut last, mut n) = (NIL, NIL, 0u64);
        self.ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
        } else {
            loop {
                self.ws();
                let k = self.str_value(
                    &key_m,
                    m.target,
                    t.cons(key_m.cons)
                        .as_ref()
                        .filter(|_| key_m.cons != 0 && !self.lenient),
                )?;
                self.expect(b':')?;
                let v = self.value(&val_m, m.target + 1, depth + 1)?;
                let kx = self.ar.push(Node {
                    next: NIL,
                    member: NIL,
                    val: k,
                });
                let vx = self.ar.push(Node {
                    next: NIL,
                    member: NIL,
                    val: v,
                });
                self.ar.link(kx, vx);
                if first == NIL {
                    first = kx;
                } else {
                    self.ar.link(last, kx);
                }
                last = vx;
                n += 1;
                self.ws();
                match self.peek() {
                    Some(b',') => self.pos += 1,
                    Some(b'}') => {
                        self.pos += 1;
                        break;
                    }
                    _ => return Err(Violation::Syntax { at: self.pos }),
                }
            }
        }
        if out_of_bounds(c, n) {
            return Err(Violation::Length {
                member: mi,
                value: None,
            });
        }
        Ok(Val::Seq(first))
    }

    pub(super) fn object(&mut self, shape: u32, depth: u32) -> R<Val> {
        if depth > MAX_DEPTH {
            return Err(Violation::TooDeep);
        }
        let t = self.t;
        let sh = t.shape(shape).ok_or(Violation::Table)?;
        self.expect(b'{')?;
        let (mut first, mut last) = (NIL, NIL);
        self.ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
        } else {
            loop {
                self.ws();
                let kv = self.string()?;
                let key = self.text(kv);
                let found = (sh.first..sh.first + sh.count)
                    .find_map(|i| t.member(i).filter(|m| m.wire == key).map(|m| (i, m)));
                self.expect(b':')?;
                if let Some((mi, m)) = found {
                    let v = self.value(&m, mi, depth)?;
                    if !matches!(v, Val::Null) {
                        let ix = self.ar.push(Node {
                            next: NIL,
                            member: mi,
                            val: v,
                        });
                        if first == NIL {
                            first = ix;
                        } else {
                            self.ar.link(last, ix);
                        }
                        last = ix;
                    }
                } else {
                    self.skip(depth + 1)?;
                }
                self.ws();
                match self.peek() {
                    Some(b',') => self.pos += 1,
                    Some(b'}') => {
                        self.pos += 1;
                        break;
                    }
                    _ => return Err(Violation::Syntax { at: self.pos }),
                }
            }
        }
        self.check_members(shape, &sh, first)?;
        Ok(Val::Obj(first))
    }

    pub(super) fn check_members(&self, shape: u32, sh: &ShapeRef<'_>, first: u32) -> R<()> {
        if sh.union {
            let n = std::iter::successors((first != NIL).then_some(first), |&i| {
                self.ar.node(i).map(|n| n.next).filter(|&x| x != NIL)
            })
            .count();
            return if n == 1 {
                Ok(())
            } else {
                Err(Violation::Union { shape })
            };
        }
        for i in sh.first..sh.first + sh.count {
            let Some(m) = self.t.member(i) else {
                return Err(Violation::Table);
            };
            // A streaming member (a payload blob, an event stream) is not carried in the JSON body, so it cannot be missing from it.
            let streamed = matches!(m.kind, K_STREAM | K_EVENT);
            if !self.lenient && m.flags & F_REQUIRED != 0 && !streamed && !self.has_child(first, i)
            {
                return Err(Violation::Missing { shape, member: i });
            }
        }
        Ok(())
    }

    pub(super) fn has_child(&self, first: u32, member: u32) -> bool {
        std::iter::successors((first != NIL).then_some(first), |&i| {
            self.ar.node(i).map(|n| n.next).filter(|&x| x != NIL)
        })
        .any(|i| self.ar.node(i).is_some_and(|n| n.member == member))
    }
}

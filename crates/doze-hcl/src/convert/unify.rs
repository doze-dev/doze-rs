//! cty's `unify`: the one type a set of types can all convert to, preferring the most general.

use super::exists;
use super::sort::sort_types;
use crate::types::{ObjectType, Type};

/// What `unify` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unified {
    /// Every type converts to this one.
    To(Type),
    /// Collections mixed with `Dynamic`: the result is the dynamic type and every value converts to
    /// the dynamic unknown (cty's `unifyAllAsDynamic`).
    AllDynamic,
    /// No common type.
    None,
}

impl Unified {
    /// The unified type (`Dynamic` for `AllDynamic`), if there is one.
    pub fn ty(&self) -> Option<Type> {
        match self {
            Unified::To(t) => Some(t.clone()),
            Unified::AllDynamic => Some(Type::Dynamic),
            Unified::None => None,
        }
    }

    /// The type, with "none" as `Dynamic` (where cty would pass its nil type on).
    pub(crate) fn ty_or_nil(&self) -> Type {
        self.ty().unwrap_or(Type::Dynamic)
    }
}

#[derive(Default)]
struct Counts {
    map: usize,
    list: usize,
    set: usize,
    object: usize,
    tuple: usize,
    dynamic: usize,
}

fn counts(types: &[Type]) -> Counts {
    let mut c = Counts::default();
    for t in types {
        match t {
            Type::Map(_) => c.map += 1,
            Type::List(_) => c.list += 1,
            Type::Set(_) => c.set += 1,
            Type::Object(_) => c.object += 1,
            Type::Tuple(_) => c.tuple += 1,
            Type::Dynamic => c.dynamic += 1,
            _ => {}
        }
    }
    c
}

pub fn unify(types: &[Type], unsafe_: bool) -> Unified {
    if types.is_empty() {
        return Unified::None;
    }
    let c = counts(types);
    let n = types.len();
    let dynamic = c.dynamic > 0;
    if c.map > 0 && c.map + c.dynamic == n {
        return collections(Type::map, types, unsafe_, dynamic);
    }
    if c.map > 0 && c.map + c.object + c.dynamic == n {
        if let Unified::To(t @ Type::Map(_)) = objects_as(types, unsafe_, true) {
            return Unified::To(t);
        }
    } else if c.list > 0 && c.list + c.dynamic == n {
        return collections(Type::list, types, unsafe_, dynamic);
    } else if c.list > 0 && c.list + c.tuple + c.dynamic == n {
        if let Unified::To(t @ Type::List(_)) = objects_as(types, unsafe_, false) {
            return Unified::To(t);
        }
    } else if c.set > 0 && c.set + c.dynamic == n {
        return collections(Type::set, types, unsafe_, dynamic);
    } else if c.object > 0 && c.object + c.dynamic == n {
        return objects(types, unsafe_, dynamic);
    } else if c.tuple > 0 && c.tuple + c.dynamic == n {
        return tuples(types, unsafe_, dynamic);
    } else if c.object > 0 && c.tuple > 0 {
        return Unified::None;
    }
    by_preference(types, unsafe_)
}

/// The first type, in cty's preference order, that every other type converts to.
fn by_preference(types: &[Type], unsafe_: bool) -> Unified {
    for want in sort_types(types) {
        let Some(want) = types.get(want) else {
            continue;
        };
        if types.iter().all(|t| t == want || exists(t, want, unsafe_)) {
            return Unified::To(want.clone());
        }
    }
    Unified::None
}

/// Objects among maps unify to a map of the objects' attributes, then with the maps (cty's
/// `unifyObjectsAsMaps`); tuples among lists likewise (`unifyTuplesAsList`).
fn objects_as(types: &[Type], unsafe_: bool, as_map: bool) -> Unified {
    let structural: Vec<Type> = types
        .iter()
        .filter(|t| {
            if as_map {
                matches!(t, Type::Object(_))
            } else {
                matches!(t, Type::Tuple(_))
            }
        })
        .cloned()
        .collect();
    let Some(collected) = (if as_map {
        to_map(&structural, unsafe_)
    } else {
        to_list(&structural, unsafe_)
    }) else {
        return Unified::None;
    };
    let replaced: Vec<Type> = types
        .iter()
        .map(|t| {
            if structural.contains(t) {
                collected.clone()
            } else {
                t.clone()
            }
        })
        .collect();
    let u = unify(&replaced, unsafe_);
    match &u {
        Unified::To(Type::Map(_)) if as_map => u,
        Unified::To(Type::List(_)) if !as_map => u,
        _ => Unified::None,
    }
}

fn collections(make: fn(Type) -> Type, types: &[Type], unsafe_: bool, dynamic: bool) -> Unified {
    if dynamic {
        return Unified::AllDynamic;
    }
    let elems: Vec<Type> = types.iter().filter_map(|t| t.element().cloned()).collect();
    let Some(e) = unify(&elems, unsafe_).ty() else {
        return Unified::None;
    };
    let ret = make(e);
    if types.iter().all(|t| *t == ret || exists(t, &ret, unsafe_)) {
        Unified::To(ret)
    } else {
        Unified::None
    }
}

fn attrs(t: &Type) -> Option<&ObjectType> {
    if let Type::Object(o) = t {
        Some(o)
    } else {
        None
    }
}

fn objects(types: &[Type], unsafe_: bool, dynamic: bool) -> Unified {
    if dynamic {
        return Unified::AllDynamic;
    }
    let objs: Vec<&ObjectType> = types.iter().filter_map(attrs).collect();
    let Some(first) = objs.first() else {
        return Unified::None;
    };
    let same_names = objs.iter().all(|o| {
        o.attrs.len() == first.attrs.len() && o.attrs.keys().all(|k| first.attrs.contains_key(k))
    });
    if !same_names {
        return to_map(types, unsafe_).map_or(Unified::None, Unified::To);
    }
    let mut out = std::collections::BTreeMap::new();
    for name in first.attrs.keys() {
        let across: Vec<Type> = objs
            .iter()
            .filter_map(|o| o.attrs.get(name).cloned())
            .collect();
        let Some(t) = unify(&across, unsafe_).ty() else {
            return Unified::None;
        };
        out.insert(name.clone(), t);
    }
    let ret = Type::object(out);
    if types.iter().all(|t| *t == ret || exists(t, &ret, unsafe_)) {
        return Unified::To(ret);
    }
    to_map(types, unsafe_).map_or(Unified::None, Unified::To)
}

fn to_map(types: &[Type], unsafe_: bool) -> Option<Type> {
    let atys: Vec<Type> = types
        .iter()
        .filter_map(attrs)
        .flat_map(|o| o.attrs.values().cloned())
        .collect();
    let ret = Type::map(unify(&atys, unsafe_).ty()?);
    types
        .iter()
        .all(|t| *t == ret || exists(t, &ret, unsafe_))
        .then_some(ret)
}

fn tuples(types: &[Type], unsafe_: bool, dynamic: bool) -> Unified {
    if dynamic {
        return Unified::AllDynamic;
    }
    let tups: Vec<&Vec<Type>> = types
        .iter()
        .filter_map(|t| {
            if let Type::Tuple(es) = t {
                Some(es)
            } else {
                None
            }
        })
        .collect();
    let Some(first) = tups.first() else {
        return Unified::None;
    };
    if tups.iter().any(|t| t.len() != first.len()) {
        return to_list(types, unsafe_).map_or(Unified::None, Unified::To);
    }
    let mut out = Vec::with_capacity(first.len());
    for i in 0..first.len() {
        let across: Vec<Type> = tups.iter().filter_map(|t| t.get(i).cloned()).collect();
        let Some(t) = unify(&across, unsafe_).ty() else {
            return Unified::None;
        };
        out.push(t);
    }
    let ret = Type::Tuple(out);
    if types.iter().all(|t| *t == ret || exists(t, &ret, unsafe_)) {
        return Unified::To(ret);
    }
    to_list(types, unsafe_).map_or(Unified::None, Unified::To)
}

fn to_list(types: &[Type], unsafe_: bool) -> Option<Type> {
    let etys: Vec<Type> = types
        .iter()
        .filter_map(|t| {
            if let Type::Tuple(es) = t {
                Some(es.clone())
            } else {
                None
            }
        })
        .flatten()
        .collect();
    let ret = Type::list(unify(&etys, unsafe_).ty()?);
    types
        .iter()
        .all(|t| *t == ret || exists(t, &ret, unsafe_))
        .then_some(ret)
}

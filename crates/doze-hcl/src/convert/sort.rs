//! cty's preference order between types (`sortTypes`, `compareTypes`).

use crate::types::Type;
use std::cmp::Ordering;

/// cty's `sortTypes`: a topological order of the types by "is more general than", ties kept in the
/// order given.
pub(super) fn sort_types(types: &[Type]) -> Vec<usize> {
    let n = types.len();
    let mut edges: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut in_degree = vec![0usize; n];
    for i in 0..n {
        for j in (i + 1)..n {
            let (Some(a), Some(b)) = (types.get(i), types.get(j)) else {
                continue;
            };
            let (from, to) = match compare(a, b) {
                Ordering::Less => (i, j),
                Ordering::Greater => (j, i),
                Ordering::Equal => continue,
            };
            if let (Some(e), Some(d)) = (edges.get_mut(from), in_degree.get_mut(to)) {
                e.push(to);
                *d += 1;
            }
        }
    }
    let mut queue: Vec<usize> = (0..n).filter(|&i| in_degree.get(i) == Some(&0)).collect();
    let mut head = 0;
    while let Some(&i) = queue.get(head) {
        head += 1;
        for &j in edges.get(i).map_or(&[][..], Vec::as_slice) {
            if let Some(d) = in_degree.get_mut(j) {
                *d -= 1;
                if *d == 0 {
                    queue.push(j);
                }
            }
        }
    }
    queue
}

/// cty's `compareTypes`: `Less` when `a` is the more general (preferred) type.
fn compare(a: &Type, b: &Type) -> Ordering {
    match (a, b) {
        (Type::Dynamic, Type::Dynamic) => return Ordering::Equal,
        (Type::Dynamic, _) => return Ordering::Greater,
        (_, Type::Dynamic) => return Ordering::Less,
        _ => {}
    }
    if a.is_primitive() && b.is_primitive() && (*a == Type::String || *b == Type::String) {
        return match (*a == Type::String, *b == Type::String) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Less,
            _ => Ordering::Greater,
        };
    }
    match (a, b) {
        (Type::List(x), Type::List(y))
        | (Type::Set(x), Type::Set(y))
        | (Type::Map(x), Type::Map(y)) => return compare(x, y),
        _ => {}
    }
    let swap = matches!(
        (a, b),
        (Type::Tuple(_), Type::List(_))
            | (Type::Object(_), Type::Map(_))
            | (Type::Set(_), Type::Tuple(_))
            | (Type::Set(_), Type::List(_))
    );
    let (a, b) = if swap { (b, a) } else { (a, b) };
    let flip = |o: Ordering| if swap { o.reverse() } else { o };
    match (a, b) {
        (Type::Tuple(_) | Type::List(_), Type::Set(_))
        | (Type::List(_), Type::Tuple(_))
        | (Type::Map(_), Type::Object(_)) => flip(Ordering::Less),
        (Type::Object(x), Type::Object(y)) => flip(compare_many(
            x.attrs.len() == y.attrs.len() && x.attrs.keys().all(|k| y.attrs.contains_key(k)),
            x.attrs
                .iter()
                .filter_map(|(k, t)| y.attrs.get(k).map(|u| compare(t, u))),
        )),
        (Type::Tuple(x), Type::Tuple(y)) => flip(compare_many(
            x.len() == y.len(),
            x.iter().zip(y).map(|(t, u)| compare(t, u)),
        )),
        _ => Ordering::Equal,
    }
}

fn compare_many(comparable: bool, cmps: impl Iterator<Item = Ordering>) -> Ordering {
    if !comparable {
        return Ordering::Equal;
    }
    let (mut a_super, mut b_super) = (false, false);
    for c in cmps {
        match c {
            Ordering::Less => a_super = true,
            Ordering::Greater => b_super = true,
            Ordering::Equal => {}
        }
    }
    match (a_super, b_super) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

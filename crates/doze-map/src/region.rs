//! Named regions: a layered map's cards grouped by what their names say. A card's region is its
//! instance's `group` when doze.hcl sets one, else its name's stem (`checkout` for `checkout-api`
//! and `checkout_worker_2`); a task's card joins its caller's region. A stem shared by fewer than
//! [`MIN_CARDS`] cards is no region of its own: those cards go to one region named [`OTHER`].
//!
//! A stack gets regions only when it splits into at least [`MIN_REGIONS`] of [`MIN_CARDS`] or more;
//! a stack with no naming habit keeps the plain layered map.

use std::collections::HashMap;

use crate::input::{Flow, Kind};

/// A region needs this many cards.
pub(crate) const MIN_CARDS: usize = 4;
/// A map needs this many regions to be drawn by region.
pub(crate) const MIN_REGIONS: usize = 3;
/// The region of the cards in no region of their own.
pub(crate) const OTHER: &str = "other";

/// The first word of a name, up to its first `-`, `_`, `.` or `/`; `None` for a name with none.
pub(crate) fn stem(name: &str) -> Option<&str> {
    let s = name.split(['-', '_', '.', '/']).next()?;
    (!s.is_empty() && s.len() < name.len()).then_some(s)
}

/// What an instance says its region is: its group, else its name's stem.
fn key<'a>(f: &Flow<'a>, v: usize) -> &'a str {
    let Some(inst) = f.inst(v) else { return "" };
    match inst.group.as_deref().filter(|g| !g.is_empty()) {
        Some(g) => g,
        None => stem(&inst.name).unwrap_or(""),
    }
}

/// The flow cards (`nodes`) by region, in the order the regions are drawn (a region that calls
/// another before it, then the larger first, then by name); `None` when the stack does not split
/// into [`MIN_REGIONS`] regions of [`MIN_CARDS`].
pub(crate) fn regions(f: &Flow<'_>, nodes: &[usize]) -> Option<Vec<(String, Vec<usize>)>> {
    let mut of: HashMap<usize, &str> = HashMap::with_capacity(nodes.len());
    for &v in nodes {
        if f.inst(v).is_some_and(|i| i.kind != Kind::Task) {
            of.insert(v, key(f, v));
        }
    }
    for &v in nodes {
        if !of.contains_key(&v) {
            let caller = f.callers(v).first().and_then(|c| of.get(c)).copied();
            of.insert(v, caller.unwrap_or(""));
        }
    }
    let mut count: HashMap<&str, usize> = HashMap::new();
    for k in of.values() {
        *count.entry(k).or_default() += 1;
    }
    let named = |k: &str| !k.is_empty() && count.get(k).copied().unwrap_or(0) >= MIN_CARDS;
    if count.keys().filter(|k| named(k)).count() < MIN_REGIONS {
        return None;
    }
    let mut at: HashMap<&str, usize> = HashMap::new();
    let mut out: Vec<(String, Vec<usize>)> = Vec::new();
    for &v in nodes {
        let k = of.get(&v).copied().filter(|k| named(k)).unwrap_or(OTHER);
        let i = *at.entry(k).or_insert_with(|| {
            out.push((k.to_string(), Vec::new()));
            out.len() - 1
        });
        if let Some(r) = out.get_mut(i) {
            r.1.push(v);
        }
    }
    order(f, &mut out);
    Some(out)
}

/// Callers' regions before the regions they call (each region's longest chain of calling regions,
/// cycles cut at the number of regions), then the larger, then by name.
fn order(f: &Flow<'_>, rs: &mut [(String, Vec<usize>)]) {
    let mut region: HashMap<usize, usize> = HashMap::new();
    for (i, (_, vs)) in rs.iter().enumerate() {
        for &v in vs {
            region.insert(v, i);
        }
    }
    // In region order, so a cycle between regions is cut the same way every time.
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for (a, (_, vs)) in rs.iter().enumerate() {
        for v in vs {
            for w in f.calls(*v) {
                if let Some(&b) = region.get(w)
                    && a != b
                    && !pairs.contains(&(a, b))
                {
                    pairs.push((a, b));
                }
            }
        }
    }
    let n = rs.len();
    let mut rank = vec![0usize; n];
    for _ in 0..n {
        let mut moved = false;
        for &(a, b) in &pairs {
            let ra = rank.get(a).copied().unwrap_or(0);
            if let Some(rb) = rank.get_mut(b)
                && *rb < ra + 1
                && ra < n
            {
                *rb = ra + 1;
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    let mut keyed: Vec<(usize, (String, Vec<usize>))> = rs
        .iter_mut()
        .enumerate()
        .map(|(i, r)| (rank.get(i).copied().unwrap_or(0), std::mem::take(r)))
        .collect();
    keyed.sort_by(|(ra, a), (rb, b)| {
        ra.cmp(rb)
            .then(b.1.len().cmp(&a.1.len()))
            .then_with(|| a.0.cmp(&b.0))
    });
    for (slot, (_, r)) in rs.iter_mut().zip(keyed) {
        *slot = r;
    }
}

#[cfg(test)]
mod tests {
    use super::{OTHER, regions, stem};
    use crate::input::{Flow, Instance, Kind};

    fn app(name: &str, needs: &[&str]) -> Instance {
        Instance::new(name, Kind::App, needs)
    }

    fn names(f: &Flow<'_>, vs: &[usize]) -> Vec<String> {
        vs.iter().map(|&v| f.name(v).to_string()).collect()
    }

    #[test]
    fn a_stem_is_the_first_word() {
        assert_eq!(stem("checkout-api"), Some("checkout"));
        assert_eq!(stem("checkout_worker_2"), Some("checkout"));
        assert_eq!(stem("data0.sync"), Some("data0"));
        assert_eq!(stem("gateway"), None);
        assert_eq!(stem("-x"), None);
    }

    /// Three regions of four; a stray stem and a bare name go to `other`; a group beats a stem; a
    /// task card joins its caller's region; callers' regions come first.
    #[test]
    fn regions_by_stem_group_and_caller() {
        let mut ins = vec![
            app("gateway", &["shop-api", "acct-api"]),
            app(
                "shop-api",
                &["shop-cart", "shop-price", "shop-stock", "pay-api"],
            ),
            app("shop-cart", &[]),
            app("shop-price", &[]),
            app("shop-stock", &["migrate"]),
            app("acct-api", &["acct-login", "acct-mfa", "acct-users"]),
            app("acct-login", &[]),
            app("acct-mfa", &[]),
            app("acct-users", &[]),
            app("pay-api", &["pay-ledger", "pay-risk", "fraud-check"]),
            app("pay-ledger", &[]),
            app("pay-risk", &[]),
            app("fraud-check", &[]),
            app("misc-x", &["shop-cart"]),
        ];
        // doze.hcl puts fraud-check in the pay group.
        ins[12].group = Some("pay".into());
        let mut t = Instance::new("migrate", Kind::Task, &[]);
        t.busy = true;
        ins.push(t);
        let f = Flow::read(&ins);
        let nodes: Vec<usize> = (0..ins.len()).collect();
        let rs = regions(&f, &nodes).expect("three regions of four");
        let by: Vec<(String, Vec<String>)> = rs
            .iter()
            .map(|(n, vs)| (n.clone(), names(&f, vs)))
            .collect();
        assert_eq!(by[0].0, OTHER, "the gateway calls into the rest: first");
        assert_eq!(by[0].1, ["gateway", "misc-x"]);
        let shop = by.iter().find(|r| r.0 == "shop").unwrap();
        assert!(shop.1.contains(&"migrate".to_string()), "{shop:?}");
        let pay = by.iter().find(|r| r.0 == "pay").unwrap();
        assert_eq!(pay.1.len(), 4);
        // shop calls pay: shop's region comes before pay's.
        let at = |n: &str| by.iter().position(|r| r.0 == n).unwrap();
        assert!(at("shop") < at("pay"));
    }

    /// Two regions of four are not enough; nor are names with no stem.
    #[test]
    fn under_three_regions_is_no_regions() {
        let mut ins: Vec<Instance> = (0..5).map(|i| app(&format!("a-{i}"), &[])).collect();
        ins.extend((0..5).map(|i| app(&format!("b-{i}"), &[])));
        ins.extend((0..3).map(|i| app(&format!("c-{i}"), &[])));
        ins.extend((0..9).map(|i| app(&format!("svc{i}"), &[])));
        let f = Flow::read(&ins);
        let nodes: Vec<usize> = (0..ins.len()).collect();
        assert!(regions(&f, &nodes).is_none());
        ins.push(app("c-3", &[]));
        let f = Flow::read(&ins);
        let nodes: Vec<usize> = (0..ins.len()).collect();
        let rs = regions(&f, &nodes).expect("now c has four");
        assert_eq!(rs.len(), 4, "a, b, c and other");
    }
}

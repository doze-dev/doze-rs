//! doze's own version against the `doze = ">= x.y"` a stack asks for (Go's `dozeversion`).

/// A version made of whole numbers (`0.2`, `v1.4.0`); `dev` is not one.
pub fn released(v: &str) -> bool {
    let v = v.strip_prefix('v').unwrap_or(v);
    !v.is_empty() && v.split('.').all(|p| p.parse::<i64>().is_ok())
}

/// `a` is older than `b`; false unless both are released versions.
pub fn older(a: &str, b: &str) -> bool {
    if !released(a) || !released(b) {
        return false;
    }
    let parts = |v: &str| -> Vec<i64> {
        v.trim_start_matches('v')
            .split('.')
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    let (x, y) = (parts(a), parts(b));
    for i in 0..x.len().max(y.len()) {
        let (p, q) = (
            x.get(i).copied().unwrap_or(0),
            y.get(i).copied().unwrap_or(0),
        );
        if p != q {
            return p < q;
        }
    }
    false
}

/// This doze (`current`) is older than the stack needs.
pub fn needs_newer(current: &str, need: &str) -> bool {
    older(current, need)
}

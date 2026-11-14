//! What the module reads back from MariaDB and from the core's records, with nothing else of the crate in it, so the fuzz
//! target can take it whole: a grant object's name split into account and target, a target as `sql::target` writes it,
//! and the rows of the privilege query (`grants::current`).

// fuzz:parser fuzz_read

/// A grant object's name split back into account and target (`name@host on <target>`): at the first " on " whose right
/// side is a whole target `ok` accepts, so a target with " on " or "@" in its names still splits right.
pub(crate) fn grant_name(name: &str, ok: fn(&str) -> bool) -> Option<(&str, &str, &str)> {
    name.match_indices(" on ").find_map(|(i, _)| {
        let (who, on) = (name.get(..i)?, name.get(i + 4..)?);
        let (user, h) = who.rsplit_once('@')?;
        ok(on).then_some((user, h, on))
    })
}

/// A grant's target as `sql::target` wrote it: `*.*`, `` `db`.* `` or `` `db`.`table` ``, backquotes doubled inside.
/// Anything else is refused: an object's name comes back from the core's records, and only what this module made may
/// reach SQL.
pub(crate) fn target_ok(t: &str) -> bool {
    fn quoted(s: &str) -> Option<&str> {
        let inner = s.strip_prefix('`')?;
        let mut chars = inner.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            if c == '`' {
                if chars.peek().map(|(_, n)| *n) == Some('`') {
                    chars.next();
                } else {
                    return inner.get(i + 1..);
                }
            }
        }
        None
    }
    if t == "*.*" {
        return true;
    }
    let Some(rest) = quoted(t).and_then(|r| r.strip_prefix('.')) else {
        return false;
    };
    rest == "*" || quoted(rest) == Some("")
}

/// The privilege query's rows (`PRIVILEGE_TYPE` tab `IS_GRANTABLE`, one a line): each privilege, and whether any row
/// was grantable.
pub(crate) fn privileges(out: &str) -> (Vec<&str>, bool) {
    let mut privs = Vec::new();
    let mut grantable = false;
    for line in out.lines() {
        let mut cols = line.split('\t');
        let (Some(p), grant) = (cols.next(), cols.next()) else {
            continue;
        };
        grantable |= grant == Some("YES");
        privs.push(p);
    }
    (privs, grantable)
}

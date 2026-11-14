//! Reading an object's name back. Prune is given names from the core's records, not from doze.hcl, so only a name this
//! module could have written may reach SQL: a grant's name is parsed, rendered again, and accepted only if the two
//! agree byte for byte.

/// `"name"`, a double quote doubled inside.
pub(crate) fn ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// What a grant is on: a database, a schema, or one kind of object in a schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum On {
    Database(String),
    Schema(String),
    /// "TABLES", "SEQUENCES" or "FUNCTIONS", and the schema.
    All(&'static str, String),
}

impl On {
    /// The SQL after `ON`.
    pub(crate) fn sql(&self) -> String {
        match self {
            On::Database(d) => format!("DATABASE {}", ident(d)),
            On::Schema(s) => format!("SCHEMA {}", ident(s)),
            On::All(k, s) => format!("ALL {k} IN SCHEMA {}", ident(s)),
        }
    }

    /// The database a statement on this target runs in: database grants in `postgres`, the rest in the instance's.
    pub(crate) fn runs_in<'a>(&self, instance: &'a str) -> &'a str {
        match self {
            On::Database(_) => "postgres",
            _ => instance,
        }
    }

    /// The object name `doze plan` shows, which prune reads back ([`crate::names::grant`]).
    pub(crate) fn name(&self, role: &str) -> String {
        let who = ident(role);
        match self {
            On::Database(d) => format!("{who} on database {}", ident(d)),
            On::Schema(s) => format!("{who} on schema {}", ident(s)),
            On::All(k, s) => format!("{who} on {} in schema {}", k.to_ascii_lowercase(), ident(s)),
        }
    }
}

/// `tables`, `sequences` or `functions` as SQL's object class.
pub(crate) fn kind(objects: &str) -> Option<&'static str> {
    match objects {
        "tables" => Some("TABLES"),
        "sequences" => Some("SEQUENCES"),
        "functions" => Some("FUNCTIONS"),
        _ => None,
    }
}

/// A quoted identifier at the start of `s` (`"a""b"`), unquoted, and the rest of `s`.
fn quoted(s: &str) -> Option<(String, &str)> {
    let inner = s.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = inner.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c != '"' {
            out.push(c);
            continue;
        }
        if chars.peek().map(|(_, n)| *n) == Some('"') {
            out.push('"');
            chars.next();
        } else {
            return Some((out, inner.get(i + 1..)?));
        }
    }
    None
}

// fuzz:parser fuzz_names
/// A grant's name ([`On::name`]) back as its role and target; `None` for anything this module did not write.
pub(crate) fn grant(name: &str) -> Option<(String, On)> {
    let (role, rest) = quoted(name)?;
    let rest = rest.strip_prefix(" on ")?;
    let on = if let Some(r) = rest.strip_prefix("database ") {
        On::Database(whole(r)?)
    } else if let Some(r) = rest.strip_prefix("schema ") {
        On::Schema(whole(r)?)
    } else {
        let (objects, r) = rest.split_once(" in schema ")?;
        On::All(kind(objects)?, whole(r)?)
    };
    (on.name(&role) == name).then_some((role, on))
}

/// `s` is one quoted identifier and nothing after it.
fn whole(s: &str) -> Option<String> {
    quoted(s)
        .filter(|(_, rest)| rest.is_empty())
        .map(|(n, _)| n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grant_name_reads_back_as_written() {
        for on in [
            On::Database("shop".into()),
            On::Schema("a\"b".into()),
            On::All("SEQUENCES", "x in schema \"y".into()),
        ] {
            let name = on.name("r\"o");
            assert_eq!(grant(&name), Some(("r\"o".to_string(), on)), "{name}");
        }
    }

    #[test]
    fn anything_else_is_refused() {
        for bad in [
            "app on database shop",
            "\"app\" on database \"shop\"; DROP ROLE x",
            "\"app\" on views in schema \"s\"",
            "\"app\" on TABLES in schema \"s\"",
            "\"app\" on database \"a\"\"",
            "\"app\"",
            "",
        ] {
            assert_eq!(grant(bad), None, "{bad}");
        }
    }
}

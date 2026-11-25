//! "Create a database", the console's form (protocol minor 2): a name, its owner, the template it is copied from and its
//! encoding. The name is required; an app that does not know forms shows one box for it and sends it as the action's
//! input, which the SDK puts in the name (the form's only required field without a default).

use crate::names::ident;
use crate::psql;
use crate::sql::lit;
use doze_module::Result;
use doze_module::forms::field_error;
use doze_module::proto::msg::blocks::{Field, field};
use doze_module::proto::msg::console::Action;
use doze_module::proto::msg::error::Error;
use doze_module::proto::msg::instance::Instance;
use std::collections::BTreeMap;

/// Its id gives its key (the first letter): `N`, as `c` is cancel's.
pub(crate) const CREATE_DATABASE: &str = "new-database";

/// The role doze's psql connects as, and so the owner a new database gets unless another is named.
const SUPERUSER: &str = "postgres";

/// The longest name PostgreSQL keeps whole (NAMEDATALEN - 1 bytes); a longer one it cuts without saying.
const LONGEST: usize = 63;

/// The action and its form. Not on a table's row: the server holds the database, not a session.
pub(crate) fn create_database() -> Action {
    let f = |id: &str, label: &str, kind: &str| Field {
        id: id.into(),
        label: label.into(),
        kind: kind.into(),
        ..Field::default()
    };
    let choice = |id: &str, label: &str, choices: &[&str], help: &str| Field {
        default: choices.first().map(|c| c.to_string()),
        choices: choices.iter().map(|c| c.to_string()).collect(),
        help: Some(help.into()),
        ..f(id, label, field::CHOICE)
    };
    Action {
        id: CREATE_DATABASE.into(),
        label: "Create a database".into(),
        // The instance's, not a row's: no table's selected row is what it acts on.
        kind: String::new(),
        destructive: false,
        input_hint: Some("name".into()),
        fields: vec![
            Field {
                required: true,
                help: Some("At most 63 bytes.".into()),
                ..f("name", "Name", field::TEXT)
            },
            Field {
                default: Some(SUPERUSER.into()),
                help: Some("A role that exists.".into()),
                ..f("owner", "Owner", field::TEXT)
            },
            choice(
                "template",
                "Template",
                &["template1", "template0"],
                "template0 is the pristine copy: pick it for another encoding.",
            ),
            choice(
                "encoding",
                "Encoding",
                &["UTF8", "SQL_ASCII"],
                "UTF8 unless you know otherwise.",
            ),
        ],
    }
}

/// A value of the form, trimmed; `None` when blank.
fn value<'a>(values: &'a BTreeMap<String, String>, key: &str) -> Option<&'a str> {
    values.get(key).map(|v| v.trim()).filter(|v| !v.is_empty())
}

/// The new database's name: the form's, or the one box of an app without forms.
fn name<'a>(values: &'a BTreeMap<String, String>, input: Option<&'a str>) -> Result<&'a str> {
    let n = value(values, "name").or_else(|| input.map(str::trim).filter(|v| !v.is_empty()));
    match n {
        None => Err(field_error("name", "Name is required")),
        Some(n) if n.len() > LONGEST => Err(field_error(
            "name",
            format!(
                "a database name is at most {LONGEST} bytes; this one is {}",
                n.len()
            ),
        )),
        Some(n) => Ok(n),
    }
}

/// The statement that makes it.
fn statement(name: &str, owner: &str, template: &str, encoding: &str) -> String {
    format!(
        "CREATE DATABASE {} OWNER {} TEMPLATE {} ENCODING {}",
        ident(name),
        ident(owner),
        ident(template),
        lit(encoding)
    )
}

/// The server's refusal, on the field it is about (a name taken since the check, an encoding the template cannot
/// take), in its own words without `ERROR:`.
fn refusal(e: Error) -> Error {
    let words = e
        .message
        .lines()
        .map(|l| l.trim_start_matches("ERROR:").trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if words.contains("already exists") {
        field_error("name", words)
    } else if words.contains("encoding") {
        field_error("encoding", words)
    } else if words.contains("role") && words.contains("does not exist") {
        field_error("owner", words)
    } else if words.contains("template") {
        field_error("template", words)
    } else {
        e
    }
}

/// Makes the database; refuses a name taken or an owner that is no role on the field it is about.
pub(crate) async fn create(
    inst: &Instance,
    input: Option<&str>,
    values: &BTreeMap<String, String>,
) -> Result<String> {
    let name = name(values, input)?;
    let owner = value(values, "owner").unwrap_or(SUPERUSER);
    let template = value(values, "template").unwrap_or("template1");
    let encoding = value(values, "encoding").unwrap_or("UTF8");
    let p = psql::admin(inst)?;
    let taken = format!(
        "SELECT EXISTS (SELECT 1 FROM pg_database WHERE datname = {})",
        lit(name)
    );
    if p.yes("postgres", &taken).await? {
        return Err(field_error(
            "name",
            format!("a database named {name} already exists"),
        ));
    }
    let role = format!(
        "SELECT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = {})",
        lit(owner)
    );
    if !p.yes("postgres", &role).await? {
        return Err(field_error(
            "owner",
            format!("there is no role named {owner}"),
        ));
    }
    p.alone("postgres", &statement(name, owner, template, encoding))
        .await
        .map_err(refusal)?;
    Ok(format!("created database {name}, owned by {owner}"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use doze_module::proto::msg::error::ErrorKind;

    fn vals(kv: &[(&str, &str)]) -> BTreeMap<String, String> {
        kv.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn the_name_comes_from_the_form_or_the_one_box() {
        assert_eq!(name(&vals(&[("name", " shop ")]), None).unwrap(), "shop");
        assert_eq!(name(&vals(&[("name", "")]), Some("old")).unwrap(), "old");
        let e = name(&vals(&[]), Some("  ")).unwrap_err();
        assert_eq!(e.field.as_deref(), Some("name"));
        let long = "x".repeat(64);
        let e = name(&vals(&[("name", &long)]), None).unwrap_err();
        assert!(e.message.contains("at most 63"), "{e}");
    }

    #[test]
    fn names_are_quoted() {
        assert_eq!(
            statement("my \"db\"", "o", "template0", "UTF8"),
            r#"CREATE DATABASE "my ""db""" OWNER "o" TEMPLATE "template0" ENCODING 'UTF8'"#
        );
    }

    #[test]
    fn the_servers_refusal_lands_on_its_field() {
        let e = |m: &str| refusal(Error::new(ErrorKind::Unavailable, m));
        assert_eq!(
            e("ERROR:  database \"x\" already exists").field.as_deref(),
            Some("name")
        );
        let enc = e(
            "ERROR:  new encoding (SQL_ASCII) is incompatible with the encoding of the template database (UTF8)\nHINT:  Use the same encoding as in the template database, or use template0 as template.",
        );
        assert_eq!(enc.field.as_deref(), Some("encoding"));
        assert!(!enc.message.contains("ERROR:"), "{enc}");
        assert_eq!(
            e("ERROR:  role \"x\" does not exist").field.as_deref(),
            Some("owner")
        );
        assert_eq!(e("could not connect").field, None);
    }

    #[test]
    fn an_app_without_forms_gets_one_box_and_it_is_the_name() {
        let a = create_database();
        assert_eq!(a.input_hint.as_deref(), Some("name"));
        let mut v = BTreeMap::new();
        doze_module::forms::one_box(&a.fields, false, Some("shop"), &mut v);
        doze_module::forms::check(&a.fields, false, &mut v).unwrap();
        assert_eq!(v.get("name").map(String::as_str), Some("shop"));
        assert_eq!(v.get("template").map(String::as_str), Some("template1"));
    }
}

//! `doze://` links (Go `internal/desk/link`): `doze://shop` (a stack), `doze://shop/db` (a service in it),
//! `doze://open?dir=/abs/folder&show=db`, `doze://library`, `doze://settings` (and a part of it: `doze://settings/machine`;
//! `doze://settings/updates` is About, asking GitHub now). The scheme and the host are matched
//! without regard to case.

mod follow;

pub(crate) use follow::{follow_args, follow_later};

use std::path::PathBuf;

/// Where a link goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Link {
    /// A stack by name, and a service in it.
    Stack {
        stack: String,
        show: Option<String>,
    },
    /// A stack's folder, and a service in it.
    Open {
        dir: PathBuf,
        show: Option<String>,
    },
    Library,
    /// Settings, at a part when the link names one (`general`, `machine`, `about`, `updates`).
    Settings(Option<String>),
}

/// Whether `s` is a doze link.
pub(crate) fn is(s: &str) -> bool {
    s.get(..7)
        .is_some_and(|p| p.eq_ignore_ascii_case("doze://"))
}

/// Reads a link; the error says what is wrong in words a person can act on.
pub(crate) fn parse(s: &str) -> Result<Link, String> {
    if !is(s) {
        return Err("not a doze:// link".into());
    }
    let rest = s.get(7..).unwrap_or("");
    let (head, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (host, path) = head.split_once('/').unwrap_or((head, ""));
    let path = path.trim_matches('/');
    match host.to_ascii_lowercase().as_str() {
        "library" => Ok(Link::Library),
        "settings" => Ok(Link::Settings(
            path.split('/')
                .next()
                .filter(|s| !s.is_empty())
                .map(str::to_ascii_lowercase),
        )),
        "open" => {
            let dir = param(query, "dir").ok_or("doze://open needs dir=, an absolute folder")?;
            let dir = PathBuf::from(dir);
            if !dir.is_absolute() {
                return Err("doze://open needs dir=, an absolute folder".into());
            }
            Ok(Link::Open {
                dir,
                show: param(query, "show").filter(|s| !s.is_empty()),
            })
        }
        "" => Err("a doze:// link names a stack: doze://shop/db".into()),
        _ => Ok(Link::Stack {
            stack: host.to_owned(),
            show: path
                .split('/')
                .next()
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
        }),
    }
}

/// A query parameter, percent-decoded.
fn param(query: &str, name: &str) -> Option<String> {
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        (k == name).then(|| decode(v))
    })
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        let hex = |j: usize| b.get(j).and_then(|h| (*h as char).to_digit(16));
        match (c, hex(i + 1), hex(i + 2)) {
            (b'%', Some(h), Some(l)) => {
                out.push(u8::try_from(h * 16 + l).unwrap_or(b'?'));
                i += 3;
            }
            (b'+', _, _) => {
                out.push(b' ');
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::{Link, is, parse};
    use std::path::PathBuf;

    #[test]
    fn links_read_as_go_reads_them() {
        assert!(is("DOZE://shop") && !is("http://x"));
        assert_eq!(
            parse("doze://shop").unwrap(),
            Link::Stack {
                stack: "shop".into(),
                show: None
            }
        );
        assert_eq!(
            parse("doze://shop/db/extra").unwrap(),
            Link::Stack {
                stack: "shop".into(),
                show: Some("db".into())
            }
        );
        assert_eq!(parse("doze://Library").unwrap(), Link::Library);
        assert_eq!(parse("doze://settings").unwrap(), Link::Settings(None));
        assert_eq!(
            parse("doze://settings/Updates").unwrap(),
            Link::Settings(Some("updates".into()))
        );
        assert_eq!(
            parse("doze://open?dir=%2Fcode%2Fmy%20shop&show=api").unwrap(),
            Link::Open {
                dir: PathBuf::from("/code/my shop"),
                show: Some("api".into())
            }
        );
        assert!(parse("doze://open?dir=relative").is_err());
        assert!(parse("doze://").unwrap_err().contains("names a stack"));
        assert!(parse("http://shop").is_err());
    }
}

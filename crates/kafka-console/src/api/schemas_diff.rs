//! The diff of two versions of a subject: a line diff of the schemas laid out (one field or key a line, so a changed field
//! is its own line), and what the registry says of the newer one following the older at BACKWARD, FORWARD and FULL,
//! stored nowhere.

use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use axum::response::Response;
use kafka_registry::SchemaInput;
use kafka_registry::format::SchemaType;
use serde::Serialize;

use super::{laid_out, no_subject, off, subject_of};
use crate::api::json;
use crate::form::{get as field, parse};
use crate::state::Shared;

/// One line of the diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DiffLine {
    /// `same`, `add` (only in `to`), `del` (only in `from`).
    pub op: String,
    /// Its line in `from` and in `to`, from 1.
    pub old: Option<usize>,
    pub new: Option<usize>,
    pub text: String,
}

/// The registry's verdict on `to` following `from` at a level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LevelCheck {
    pub level: String,
    pub ok: bool,
    /// The first reason, in the registry's words (`READER_FIELD_MISSING_DEFAULT_VALUE: The field ...`).
    pub reason: Option<String>,
}

/// `GET /api/v1/schemas/subjects/{subject}/diff?from=&to=`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DiffResponse {
    pub subject: String,
    pub from: u32,
    pub to: u32,
    /// Every version that can be compared, newest first.
    pub versions: Vec<u32>,
    pub lines: Vec<DiffLine>,
    pub added: usize,
    pub removed: usize,
    /// The subject's level.
    pub level: String,
    pub checks: Vec<LevelCheck>,
}

/// The most cells the line diff's table may have; past it the two texts are shown as removed and added whole.
const MOST_CELLS: usize = 4_000_000;

/// A line diff by the longest common subsequence.
#[must_use]
pub fn lines(a: &[&str], b: &[&str]) -> Vec<DiffLine> {
    let line = |op: &str, old: Option<usize>, new: Option<usize>, text: &str| DiffLine {
        op: op.to_owned(),
        old,
        new,
        text: text.to_owned(),
    };
    let (n, m) = (a.len(), b.len());
    if n.saturating_mul(m) > MOST_CELLS {
        let mut out: Vec<DiffLine> = a
            .iter()
            .enumerate()
            .map(|(i, t)| line("del", Some(i + 1), None, t))
            .collect();
        out.extend(
            b.iter()
                .enumerate()
                .map(|(j, t)| line("add", None, Some(j + 1), t)),
        );
        return out;
    }
    // lcs[i][j]: the common lines of a[i..] and b[j..], kept as one row-major table.
    let w = m + 1;
    let mut lcs = vec![0u32; (n + 1) * w];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            let v = if a.get(i) == b.get(j) {
                lcs.get((i + 1) * w + j + 1).copied().unwrap_or(0) + 1
            } else {
                let down = lcs.get((i + 1) * w + j).copied().unwrap_or(0);
                let right = lcs.get(i * w + j + 1).copied().unwrap_or(0);
                down.max(right)
            };
            if let Some(c) = lcs.get_mut(i * w + j) {
                *c = v;
            }
        }
    }
    let cell = |i: usize, j: usize| lcs.get(i * w + j).copied().unwrap_or(0);
    let (mut i, mut j, mut out) = (0, 0, Vec::with_capacity(n + m));
    while i < n || j < m {
        match (a.get(i), b.get(j)) {
            (Some(x), Some(y)) if x == y => {
                out.push(line("same", Some(i + 1), Some(j + 1), x));
                i += 1;
                j += 1;
            }
            (Some(x), Some(_)) if cell(i + 1, j) >= cell(i, j + 1) => {
                out.push(line("del", Some(i + 1), None, x));
                i += 1;
            }
            (_, Some(y)) => {
                out.push(line("add", None, Some(j + 1), y));
                j += 1;
            }
            (Some(x), None) => {
                out.push(line("del", Some(i + 1), None, x));
                i += 1;
            }
            (None, None) => break,
        }
    }
    out
}

/// The registry's verdict on `input` following version `from`, at BACKWARD, FORWARD and FULL.
fn checks(
    r: &kafka_registry::Registry,
    subject: &str,
    from: u32,
    input: &SchemaInput,
) -> Vec<LevelCheck> {
    ["BACKWARD", "FORWARD", "FULL"]
        .into_iter()
        .map(|level| {
            let found = r.check(subject, Some(&from.to_string()), input, false, Some(level));
            let reason = match &found {
                Ok((_, f)) => f
                    .first()
                    .and_then(|x| x.details.first())
                    .map(|d| format!("{}: {}", d.kind, d.description)),
                Err(e) => Some(e.message.clone()),
            };
            LevelCheck {
                level: level.to_owned(),
                ok: reason.is_none(),
                reason,
            }
        })
        .collect()
}

pub(super) async fn diff(
    State(s): State<Shared>,
    Path(raw): Path<String>,
    RawQuery(q): RawQuery,
) -> Response {
    let Some(r) = s.source.schemas().registry().cloned() else {
        return off();
    };
    let subject = subject_of(&raw);
    let mut versions = r.versions(&subject, false).unwrap_or_default();
    versions.reverse();
    let Some(&latest) = versions.first() else {
        return no_subject(&subject);
    };
    let f = parse(q.as_deref().unwrap_or(""));
    let pick = |k: &str, or: u32| {
        field(&f, k)
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|v| versions.contains(v))
            .unwrap_or(or)
    };
    let to = pick("to", latest);
    let before = versions.iter().copied().find(|v| *v < to).unwrap_or(to);
    let from = pick("from", before);
    let (Ok(a), Ok(b)) = (
        r.version(&subject, &from.to_string(), false),
        r.version(&subject, &to.to_string(), false),
    ) else {
        return no_subject(&subject);
    };
    let kind = b.schema_type.unwrap_or(SchemaType::Avro);
    let (ta, tb) = (laid_out(kind, &a.schema), laid_out(kind, &b.schema));
    let (la, lb): (Vec<&str>, Vec<&str>) = (ta.lines().collect(), tb.lines().collect());
    let lines = lines(&la, &lb);
    let input = SchemaInput {
        schema: Some(b.schema.clone()),
        schema_type: Some(kind.name().to_owned()),
        references: Some(b.references.clone()),
    };
    let checks = checks(&r, &subject, from, &input);
    json(
        StatusCode::OK,
        &DiffResponse {
            added: lines.iter().filter(|l| l.op == "add").count(),
            removed: lines.iter().filter(|l| l.op == "del").count(),
            level: r.level_for(&subject, None),
            subject,
            from,
            to,
            versions,
            lines,
            checks,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::lines;

    #[test]
    fn a_changed_line_is_one_removed_and_one_added_and_the_rest_is_kept() {
        let d = lines(&["{", "  a", "  b", "}"], &["{", "  a", "  c", "  d", "}"]);
        let ops: Vec<(&str, &str)> = d.iter().map(|l| (l.op.as_str(), l.text.as_str())).collect();
        assert_eq!(
            ops,
            [
                ("same", "{"),
                ("same", "  a"),
                ("del", "  b"),
                ("add", "  c"),
                ("add", "  d"),
                ("same", "}")
            ]
        );
        assert_eq!(
            (d[2].old, d[2].new, d[3].old, d[3].new),
            (Some(3), None, None, Some(3))
        );
        assert!(lines(&[], &[]).is_empty());
    }
}

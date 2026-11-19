//! The functions a query may call that are not aggregates: string, number, time, IP address and general ones, by the names of AWS's
//! "Functions" page of the Insights syntax (memory: the page is not in the model). Their names are taken in any case.

use std::net::IpAddr;

use super::ast::{AggKind, Expr, Period};
use super::eval::eval;
use super::record::Row;
use super::value::Val;

/// The functions, with the fewest and most arguments each takes (`usize::MAX`: any number).
const FUNCTIONS: [(&str, usize, usize); 32] = [
    ("abs", 1, 1),
    ("ceil", 1, 1),
    ("floor", 1, 1),
    ("log", 1, 1),
    ("sqrt", 1, 1),
    ("greatest", 1, usize::MAX),
    ("least", 1, usize::MAX),
    ("concat", 1, usize::MAX),
    ("ltrim", 1, 2),
    ("rtrim", 1, 2),
    ("trim", 1, 2),
    ("strlen", 1, 1),
    ("toupper", 1, 1),
    ("tolower", 1, 1),
    ("substr", 2, 3),
    ("replace", 3, 3),
    ("strcontains", 2, 2),
    ("isempty", 1, 1),
    ("isblank", 1, 1),
    ("ispresent", 1, 1),
    ("coalesce", 1, usize::MAX),
    ("bin", 1, 1),
    ("datefloor", 2, 2),
    ("dateceil", 2, 2),
    ("frommillis", 1, 1),
    ("tomillis", 1, 1),
    ("isvalidip", 1, 1),
    ("isvalidipv4", 1, 1),
    ("isvalidipv6", 1, 1),
    ("isipinsubnet", 2, 2),
    ("isipv4insubnet", 2, 2),
    ("isipv6insubnet", 2, 2),
];

/// The expression a call makes, or why it is refused.
pub(crate) fn checked(name: &str, args: Vec<Expr>) -> Result<Expr, String> {
    if AggKind::of(name).is_some() {
        return Ok(Expr::Agg(name.to_owned(), args));
    }
    let Some((_, least, most)) = FUNCTIONS.iter().find(|(n, _, _)| *n == name) else {
        return Err(format!("unknown function '{name}'"));
    };
    if args.len() < *least || args.len() > *most || args.iter().any(|a| matches!(a, Expr::Star)) {
        return Err(format!("wrong number of arguments to '{name}'"));
    }
    Ok(Expr::Call(name.to_owned(), args))
}

/// The value of a call.
pub(crate) fn call(name: &str, args: &[Expr], row: &Row, slots: &[Val]) -> Val {
    let arg = |i: usize| args.get(i).map_or(Val::Null, |a| eval(a, row, slots));
    match name {
        "bin" => match (args.first(), row.ts()) {
            (Some(Expr::Period(p)), Some(ts)) => Val::Time(floor(ts, *p)),
            _ => Val::Null,
        },
        "datefloor" | "dateceil" => match (arg(0).time(), args.get(1)) {
            (Some(t), Some(Expr::Period(p))) => {
                let f = floor(t, *p);
                Val::Time(if name == "dateceil" && f != t {
                    next(f, *p)
                } else {
                    f
                })
            }
            _ => Val::Null,
        },
        "frommillis" => arg(0).time().map_or(Val::Null, Val::Time),
        #[allow(clippy::cast_precision_loss)]
        "tomillis" => arg(0).time().map_or(Val::Null, |t| Val::Num(t as f64)),
        "ispresent" => Val::Bool(!arg(0).is_null()),
        "isempty" => Val::Bool(arg(0).text().is_empty()),
        "isblank" => Val::Bool(arg(0).text().trim().is_empty()),
        "coalesce" => args
            .iter()
            .map(|a| eval(a, row, slots))
            .find(|v| !v.is_null())
            .unwrap_or(Val::Null),
        _ => text_or_number(name, args, row, slots),
    }
}

fn text_or_number(name: &str, args: &[Expr], row: &Row, slots: &[Val]) -> Val {
    let all: Vec<Val> = args.iter().map(|a| eval(a, row, slots)).collect();
    let text = |i: usize| all.get(i).map(Val::text).unwrap_or_default();
    let num = |i: usize| all.get(i).and_then(Val::num);
    let present = all.first().is_some_and(|v| !v.is_null());
    match name {
        "concat" => Val::Str(all.iter().map(Val::text).collect()),
        "strlen" if present => Val::Num(count(&text(0))),
        "toupper" if present => Val::Str(text(0).to_uppercase()),
        "tolower" if present => Val::Str(text(0).to_lowercase()),
        "trim" | "ltrim" | "rtrim" if present => Val::Str(trim(name, &text(0), all.get(1))),
        "substr" if present => substr(&text(0), num(1), num(2)),
        "replace" if present => Val::Str(text(0).replace(&text(1), &text(2))),
        "strcontains" if present => Val::Num(if text(0).contains(&text(1)) { 1.0 } else { 0.0 }),
        "isvalidip" => Val::Bool(text(0).parse::<IpAddr>().is_ok()),
        "isvalidipv4" => Val::Bool(matches!(text(0).parse::<IpAddr>(), Ok(IpAddr::V4(_)))),
        "isvalidipv6" => Val::Bool(matches!(text(0).parse::<IpAddr>(), Ok(IpAddr::V6(_)))),
        "isipinsubnet" | "isipv4insubnet" | "isipv6insubnet" => {
            Val::Bool(in_subnet(name, &text(0), &text(1)))
        }
        "greatest" | "least" => extreme(name == "greatest", &all),
        _ => number(name, num(0)),
    }
}

fn number(name: &str, x: Option<f64>) -> Val {
    let Some(x) = x else {
        return Val::Null;
    };
    let n = match name {
        "abs" => x.abs(),
        "ceil" => x.ceil(),
        "floor" => x.floor(),
        "log" => x.ln(),
        "sqrt" => x.sqrt(),
        _ => return Val::Null,
    };
    if n.is_finite() {
        Val::Num(n)
    } else {
        Val::Null
    }
}

#[allow(clippy::cast_precision_loss)]
fn count(s: &str) -> f64 {
    s.chars().count() as f64
}

fn trim(name: &str, s: &str, chars: Option<&Val>) -> String {
    let set: Vec<char> = chars.map(Val::text).unwrap_or_default().chars().collect();
    let strip = |c: char| {
        if set.is_empty() {
            c.is_whitespace()
        } else {
            set.contains(&c)
        }
    };
    match name {
        "ltrim" => s.trim_start_matches(strip).to_owned(),
        "rtrim" => s.trim_end_matches(strip).to_owned(),
        _ => s.trim_matches(strip).to_owned(),
    }
}

/// `substr(s, start[, length])`, counting characters from 0. (memory)
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn substr(s: &str, start: Option<f64>, len: Option<f64>) -> Val {
    let start = start.unwrap_or(0.0).max(0.0) as usize;
    let chars = s.chars().skip(start);
    Val::Str(match len {
        Some(l) => chars.take(l.max(0.0) as usize).collect(),
        None => chars.collect(),
    })
}

fn extreme(greatest: bool, all: &[Val]) -> Val {
    let mut best: Option<f64> = None;
    for n in all.iter().filter_map(Val::num) {
        best = Some(best.map_or(n, |b| if greatest { b.max(n) } else { b.min(n) }));
    }
    best.map_or(Val::Null, Val::Num)
}

fn in_subnet(name: &str, ip: &str, cidr: &str) -> bool {
    let (Ok(ip), Some((net, bits))) = (ip.parse::<IpAddr>(), cidr.split_once('/')) else {
        return false;
    };
    let (Ok(net), Ok(bits)) = (net.parse::<IpAddr>(), bits.parse::<u32>()) else {
        return false;
    };
    match (ip, net) {
        (IpAddr::V4(a), IpAddr::V4(b)) if name != "isipv6insubnet" && bits <= 32 => {
            let mask = u32::MAX.checked_shl(32 - bits).unwrap_or(0);
            u32::from(a) & mask == u32::from(b) & mask
        }
        (IpAddr::V6(a), IpAddr::V6(b)) if name != "isipv4insubnet" && bits <= 128 => {
            let mask = u128::MAX.checked_shl(128 - bits).unwrap_or(0);
            u128::from(a) & mask == u128::from(b) & mask
        }
        _ => false,
    }
}

/// The start of the period `t` falls in: a multiple of a fixed length from the epoch, or the first of a run of months.
pub(crate) fn floor(t: i64, p: Period) -> i64 {
    match p {
        Period::Ms(ms) if ms > 0 => t - t.rem_euclid(ms),
        Period::Ms(_) => t,
        Period::Months(n) => {
            let n = n.max(1);
            let (y, m, _) = aws_core::time::civil_from_days(t.div_euclid(86_400_000));
            let months = y * 12 + m - 1;
            let first = months - months.rem_euclid(n);
            aws_core::time::days_from_civil(first.div_euclid(12), first.rem_euclid(12) + 1, 1)
                * 86_400_000
        }
    }
}

/// The start of the period after the one starting at `start`.
fn next(start: i64, p: Period) -> i64 {
    match p {
        Period::Ms(ms) => start.saturating_add(ms.max(1)),
        Period::Months(n) => {
            let (y, m, _) = aws_core::time::civil_from_days(start.div_euclid(86_400_000));
            let months = y * 12 + m - 1 + n.max(1);
            aws_core::time::days_from_civil(months.div_euclid(12), months.rem_euclid(12) + 1, 1)
                * 86_400_000
        }
    }
}

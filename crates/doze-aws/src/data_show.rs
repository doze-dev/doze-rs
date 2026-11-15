//! `doze-aws data` as a person reads it: where the space goes, the biggest first, and what to type to get it back. Pure functions
//! over the report's JSON (the same document `GET /_doze/data` answers), so the server and the local engine print alike.

use serde_json::Value;

use crate::style::Style;

/// Bytes as a person reads them: `812 B`, `4.0 KB`, `86 MB`, `4.2 GB`.
pub(crate) fn size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    #[allow(clippy::cast_precision_loss)]
    let mut v = bytes as f64 / 1024.0;
    let mut unit = 0;
    while v >= 1024.0 && unit + 1 < UNITS.len() {
        v /= 1024.0;
        unit += 1;
    }
    let u = UNITS.get(unit).copied().unwrap_or("TB");
    if v < 10.0 {
        format!("{v:.1} {u}")
    } else {
        format!("{v:.0} {u}")
    }
}

/// A count with thousands separators: `212,418`.
pub(crate) fn count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn u(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(Value::as_u64).unwrap_or(0)
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

fn list<'a>(v: &'a Value, k: &str) -> &'a [Value] {
    v.get(k)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn reclaimable(service: &Value) -> u64 {
    list(service, "files")
        .iter()
        .map(|f| u(f, "reclaimable"))
        .sum()
}

fn by_size(items: &[Value]) -> Vec<&Value> {
    let mut sorted: Vec<&Value> = items.iter().collect();
    sorted.sort_by(|a, b| {
        u(b, "bytes")
            .cmp(&u(a, "bytes"))
            .then_with(|| s(a, "name").cmp(s(b, "name")))
    });
    sorted
}

/// `3 queues` / `1 queue`.
fn things(service: &Value, n: usize) -> String {
    let word = if n == 1 {
        s(service, "one")
    } else {
        s(service, "many")
    };
    if word.is_empty() {
        String::new()
    } else {
        format!("{n} {word}")
    }
}

/// The overview: every service with data, the largest first, then what is worth a look.
pub(crate) fn overview(report: &Value, shown_dir: &str, st: Style) -> String {
    let services = by_size(list(report, "services"));
    let total = u(report, "bytes");
    if services.is_empty() || total == 0 {
        return format!("doze-aws keeps nothing yet in {shown_dir}.\n");
    }
    let free: u64 = services.iter().map(|svc| reclaimable(svc)).sum();
    let mut out = format!("doze-aws keeps {} in {shown_dir}\n", st.bold(&size(total)));
    if free >= 1 << 20 {
        out.push_str(&format!(
            "  {} of it is space earlier deletes left behind: {} gives it back\n",
            size(free),
            st.cyan("doze-aws data compact")
        ));
    }
    out.push('\n');
    let width = services
        .iter()
        .map(|x| s(x, "title").len())
        .max()
        .unwrap_or(0);
    for svc in &services {
        let resources = by_size(list(svc, "resources"));
        let mut line = format!(
            "  {}  {:>7}   {:<16}",
            crate::style::pad(s(svc, "title"), width),
            size(u(svc, "bytes")),
            things(svc, resources.len())
        );
        if let Some(top) = resources.first().filter(|r| u(r, "bytes") > 0) {
            line.push_str(&st.dim(&format!(
                "largest: {} {}",
                s(top, "name"),
                size(u(top, "bytes"))
            )));
        }
        if let Some(e) = svc.get("error").and_then(Value::as_str) {
            line.push_str(&st.red(&format!("could not ask: {e}")));
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out.push_str(&worth_a_look(&services, st));
    out.push_str(&st.dim("\nOne service: doze-aws data <service>   Remove: doze-aws data empty <service> <name>   Start over: doze-aws data reset\n"));
    out
}

/// The notes of every resource, largest first: data a person may not know is there, and what removes it.
fn worth_a_look(services: &[&Value], st: Style) -> String {
    let mut notes: Vec<(u64, String)> = Vec::new();
    for svc in services {
        for r in list(svc, "resources") {
            for n in list(r, "notes") {
                let mut line = format!(
                    "  {} {} ({}): {}",
                    st.yellow("!"),
                    s(r, "name"),
                    s(svc, "title"),
                    s(n, "text")
                );
                if let Some(cmd) = suggestion(s(svc, "name"), r, n.get("prune")) {
                    line.push_str(&format!("\n      {}", st.cyan(&cmd)));
                }
                notes.push((u(n, "bytes"), line));
            }
        }
    }
    if notes.is_empty() {
        return String::new();
    }
    notes.sort_by_key(|n| std::cmp::Reverse(n.0));
    let mut out = String::from("\nWorth a look\n");
    for (_, line) in notes.iter().take(8) {
        out.push_str(line);
        out.push('\n');
    }
    if notes.len() > 8 {
        out.push_str(&st.dim(&format!(
            "  and {} more: doze-aws data <service>\n",
            notes.len() - 8
        )));
    }
    out
}

/// The command a note's prune is, when it has one.
pub(crate) fn suggestion(service: &str, resource: &Value, prune: Option<&Value>) -> Option<String> {
    let p = prune.filter(|p| !p.is_null())?;
    let mut cmd = format!("doze-aws data empty {service}");
    if p.get("unused").and_then(Value::as_bool) == Some(true) {
        return Some(format!("{cmd} --unused"));
    }
    cmd.push(' ');
    cmd.push_str(&quoted(s(resource, "name")));
    if let Some(prefix) = p.get("prefix").and_then(Value::as_str) {
        cmd.push_str(&format!(" --prefix {}", quoted(prefix)));
    }
    if p.get("old_versions").and_then(Value::as_bool) == Some(true) {
        cmd.push_str(" --old-versions");
    }
    if let Some(before) = p.get("before_ms").and_then(Value::as_i64) {
        cmd.push_str(&format!(" --older-than {}", age(now_ms() - before)));
    }
    Some(cmd)
}

fn quoted(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_./:".contains(c))
    {
        s.to_owned()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// An age as `--older-than` reads it, rounded down to the largest whole unit.
fn age(ms: i64) -> String {
    let secs = ms.max(0) / 1000;
    match secs {
        s if s >= 86_400 => format!("{}d", s / 86_400),
        s if s >= 3_600 => format!("{}h", s / 3_600),
        s if s >= 60 => format!("{}m", s / 60),
        s => format!("{s}s"),
    }
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// One service: its resources, the largest first.
pub(crate) fn service(svc: &Value, st: Style) -> String {
    let resources = by_size(list(svc, "resources"));
    let mut out = format!(
        "{} keeps {} on disk",
        st.bold(s(svc, "title")),
        size(u(svc, "bytes"))
    );
    let free = reclaimable(svc);
    if free >= 64 << 10 {
        out.push_str(&format!(
            " ({} of it reclaimable: doze-aws data compact)",
            size(free)
        ));
    }
    out.push('\n');
    if let Some(e) = svc.get("error").and_then(Value::as_str) {
        out.push_str(&st.red(&format!("  It could not say what it holds: {e}\n")));
    }
    if resources.is_empty() {
        if s(svc, "many").is_empty() {
            out.push_str(
                "  Settings and definitions only: `doze-aws data reset <service>` removes them.\n",
            );
        } else {
            out.push_str(&format!("  No {}.\n", s(svc, "many")));
        }
        return out;
    }
    let width = resources
        .iter()
        .map(|r| s(r, "name").len())
        .max()
        .unwrap_or(0)
        .min(60);
    let regions: std::collections::BTreeSet<&str> =
        resources.iter().map(|r| s(r, "region")).collect();
    out.push('\n');
    for r in &resources {
        let region = if regions.len() > 1 {
            format!("  {}", s(r, "region"))
        } else {
            String::new()
        };
        let marks = if list(r, "notes").is_empty() {
            String::new()
        } else {
            format!("  {}", st.yellow("!"))
        };
        out.push_str(&format!(
            "  {}  {:>7}  {} {}{region}{marks}\n",
            crate::style::pad(s(r, "name"), width),
            size(u(r, "bytes")),
            count(u(r, "items")),
            s(svc, "units"),
        ));
    }
    out.push_str(&st.dim(&format!(
        "\nOne {}: doze-aws data {} <name>   Empty one: doze-aws data empty {} <name>\n",
        s(svc, "one"),
        s(svc, "name"),
        s(svc, "name")
    )));
    out
}

/// One resource: its size, and each note with what removes it.
pub(crate) fn resource(svc: &Value, r: &Value, st: Style) -> String {
    let service = s(svc, "name");
    let mut out = format!(
        "{} {} {}  ({})\n  {} in {} {}\n",
        s(svc, "title"),
        s(svc, "one"),
        st.bold(s(r, "name")),
        s(r, "region"),
        size(u(r, "bytes")),
        count(u(r, "items")),
        s(svc, "units"),
    );
    for n in list(r, "notes") {
        out.push_str(&format!("  {} {}", st.yellow("!"), s(n, "text")));
        if u(n, "bytes") > 0 {
            out.push_str(&format!(" ({})", size(u(n, "bytes"))));
        }
        out.push('\n');
        if let Some(cmd) = suggestion(service, r, n.get("prune")) {
            out.push_str(&format!("      {}\n", st.cyan(&cmd)));
        }
    }
    out.push_str(&format!(
        "\n  Empty it (the {} stays):  {}\n",
        s(svc, "one"),
        st.cyan(&format!(
            "doze-aws data empty {service} {}",
            quoted(s(r, "name"))
        ))
    ));
    let prunes: Vec<&str> = list(svc, "prunes")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    if !prunes.is_empty() {
        let flags: Vec<String> = prunes
            .iter()
            .map(|p| match *p {
                "before" => "--older-than <age>".to_owned(),
                "prefix" => "--prefix <p>".to_owned(),
                other => format!("--{other}"),
            })
            .collect();
        out.push_str(&st.dim(&format!(
            "  Only part of it:          {}\n",
            flags.join(", ")
        )));
    }
    out
}

#[cfg(test)]
#[path = "data_show_tests.rs"]
mod tests;

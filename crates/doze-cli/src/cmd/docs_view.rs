//! How a module's page reads on a terminal and in a pipe.

use super::docs::Page;
use crate::paint::{Paint, pad, width};
use crate::registry::meta::Arg;

const WRAP: usize = 80;

impl Page {
    pub(crate) fn human(&self, p: Paint) -> String {
        let m = &self.meta;
        let name = self.module.strip_prefix("doze/").unwrap_or(&self.module);
        let which = if self.pinned { "pinned" } else { "newest" };
        let mut out = format!(
            "{} {} {} · {}\n",
            p.bold(name),
            self.version,
            p.dim(&format!("({which})")),
            m.tagline
        );
        let mut facts = Vec::new();
        if !self.engines.is_empty() {
            facts.push(format!(
                "runs {} {}",
                or(&m.engine, name),
                self.engines.join(", ")
            ));
        }
        if let Some(port) = m.port {
            facts.push(format!("port {port}"));
        }
        if !facts.is_empty() {
            out.push_str(&format!("  {}\n", p.dim(&facts.join(" · "))));
        }
        for line in wrap(&m.description, WRAP - 2) {
            out.push_str(&format!("  {line}\n"));
        }
        if !m.example.is_empty() {
            out.push('\n');
            for line in m.example.lines() {
                out.push_str(format!("  {line}").trim_end());
                out.push('\n');
            }
        }
        out.push_str(&arguments(&m.arguments, "  ", p));
        for b in &m.blocks {
            let label = if b.label.is_empty() {
                String::new()
            } else {
                format!(" \"<{}>\"", b.label)
            };
            out.push_str(&format!(
                "\n  {}{label} {{ … }}  {}\n",
                p.bold(&b.name),
                b.desc
            ));
            out.push_str(&arguments(&b.arguments, "    ", p));
        }
        out.push_str(&format!(
            "\n  next: {} · the whole page: {}\n",
            p.cmd(&format!("doze add {name}")),
            p.cmd(&format!("doze modules docs {name} --web"))
        ));
        out
    }
}

fn or<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a.is_empty() { b } else { a }
}

fn arguments(args: &[Arg], indent: &str, p: Paint) -> String {
    if args.is_empty() {
        return String::new();
    }
    let rows: Vec<(String, String, String, String)> = args
        .iter()
        .map(|a| {
            let need = if a.required {
                "required".to_string()
            } else if a.default.is_empty() {
                String::new()
            } else {
                format!("default {}", a.default)
            };
            let mut desc = a.desc.clone();
            if !a.since.is_empty() {
                desc.push_str(&format!(" (since {})", a.since));
            }
            if !a.until.is_empty() {
                desc.push_str(&format!(" (until {})", a.until));
            }
            (a.name.clone(), a.ty.clone(), need, desc)
        })
        .collect();
    let w = |f: fn(&(String, String, String, String)) -> &String| {
        rows.iter().map(|r| width(f(r))).max().unwrap_or(0) + 2
    };
    let (w0, w1, w2) = (w(|r| &r.0), w(|r| &r.1), w(|r| &r.2));
    let mut out = String::from("\n");
    for (name, ty, need, desc) in &rows {
        let line = format!(
            "{}{}{}{}",
            pad(name, w0),
            p.dim(&pad(ty, w1)),
            pad(need, w2),
            desc
        );
        out.push_str(&format!("{indent}{}\n", line.trim_end()));
    }
    out
}

/// Words joined into lines of at most `w` columns (a longer word gets a line of its own).
fn wrap(text: &str, w: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && width(&line) + 1 + width(word) > w {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

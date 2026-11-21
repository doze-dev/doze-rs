//! Settings › Machine's table (Go `serve.doctor` and `settings.tsx`'s `Doctor`): what doze set up on this machine,
//! part by part, each with the command that puts it in place. The names and the domains come from `doze --json
//! doctor` (the CLI's machine contract); the app's own part is whether a terminal finds `doze`.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::library::Doctor;

/// One part of the machine's setup (Go `view.Check`, with its fix).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Part {
    /// `names`, `domains` or `app`.
    pub section: &'static str,
    pub name: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// The command that puts it in place in a terminal; Repair does the same from the app.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub fix: String,
}

/// Where the `doze` the app runs is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DozeAt {
    /// Where a terminal finds it: on `PATH`, or in `/usr/local/bin` or `/opt/homebrew/bin` (Go `dozeOnPath`).
    pub on_path: Option<PathBuf>,
    /// Found somewhere a terminal does not look (`~/go/bin`, `~/.local/bin`): what Repair links.
    pub elsewhere: Option<PathBuf>,
}

/// Where Repair puts doze so a terminal finds it.
pub const PATH_LINK: &str = "/usr/local/bin/doze";

impl DozeAt {
    /// Looks: `PATH` first (a terminal's), then the places a terminal's `PATH` has; then the places the app also
    /// looks.
    pub fn find(path_var: Option<&std::ffi::OsStr>, user_home: Option<&Path>) -> DozeAt {
        let mut terminal: Vec<PathBuf> = path_var
            .map(|p| std::env::split_paths(p).collect())
            .unwrap_or_default();
        terminal.extend([PATH_LINK, "/opt/homebrew/bin/doze"].map(|p| {
            Path::new(p)
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default()
        }));
        let on_path = terminal
            .into_iter()
            .map(|d| d.join("doze"))
            .find(|p| p.is_file());
        let elsewhere = user_home.and_then(|h| {
            [".local/bin/doze", "go/bin/doze"]
                .iter()
                .map(|p| h.join(p))
                .find(|p| p.is_file())
        });
        DozeAt {
            elsewhere: if on_path.is_some() { None } else { elsewhere },
            on_path,
        }
    }
}

/// The table: the names steps and the domain steps doctor reported, each failing one with its fix, then the app's
/// part. `version` is the app's, as Go's row said it.
pub fn parts(doctor: &Doctor, at: &DozeAt, version: &str) -> Vec<Part> {
    let mut out: Vec<Part> = doctor
        .checks
        .iter()
        .filter(|c| matches!(c.section, "names" | "domains"))
        .map(|c| Part {
            section: c.section,
            name: c.name.clone(),
            ok: c.ok,
            text: c.text.clone(),
            fix: match (c.ok, c.section) {
                (true, _) => String::new(),
                (false, "names") => "doze dns-setup".into(),
                (false, _) => "doze trust".into(),
            },
        })
        .collect();
    out.push(app_part(at, version));
    out
}

/// Needs you's machine parts (Go `MachineItems` reads the same doctor as Settings): doctor's names and domains, each with
/// the command that sets it up (none for names switched off), then the app's own part, whose fix is the `ln -s` that
/// puts doze on the PATH.
pub fn attention_parts(
    doctor: &Doctor,
    at: &DozeAt,
    version: &str,
) -> Vec<crate::attention::MachinePart> {
    let part = |section: &str, name: &str, ok: bool, text: &str, fix: &str| {
        crate::attention::MachinePart {
            section: section.into(),
            name: name.into(),
            ok,
            text: text.into(),
            fix: fix.into(),
        }
    };
    let mut out: Vec<_> = doctor
        .checks
        .iter()
        .map(|c| part(c.section, &c.name, c.ok, &c.text, crate::library::fix_of(c)))
        .collect();
    let app = app_part(at, version);
    out.push(part(app.section, &app.name, app.ok, &app.text, &app.fix));
    out
}

fn app_part(at: &DozeAt, version: &str) -> Part {
    let mut p = Part {
        section: "app",
        name: "doze on PATH".into(),
        ok: true,
        ..Part::default()
    };
    match (&at.on_path, &at.elsewhere) {
        (Some(path), _) => p.text = format!("{} · {version}", path.display()),
        (None, Some(found)) => {
            p.ok = false;
            p.text = "not on your PATH: a terminal and scripts cannot find doze".into();
            p.fix = format!("ln -s {} {PATH_LINK}", found.display());
        }
        (None, None) => {
            p.ok = false;
            p.text = "not installed: the app needs the doze command for stacks".into();
            p.fix = "brew install doze".into();
        }
    }
    p
}

/// The sections in the order the table shows them, with their headings and notes (Go `SECTIONS`, `SECTION_NOTES`).
pub fn sections(parts: &[Part]) -> Vec<(&'static str, &'static str, &'static str)> {
    let mut out = Vec::new();
    for p in parts {
        if !out.iter().any(|(s, _, _)| *s == p.section) {
            let (title, note) = match p.section {
                "names" => ("names", "shared with doze-aws and doze-kafka"),
                "domains" => ("domains", "declared by stacks on this Mac"),
                "app" => ("the app", ""),
                other => (other, ""),
            };
            out.push((p.section, title, note));
        }
    }
    out
}

/// How many parts are not in place.
pub fn to_do(parts: &[Part]) -> usize {
    parts.iter().filter(|p| !p.ok).count()
}

/// Some part is missing and has a fix (Go `fixable`).
pub fn fixable(parts: &[Part]) -> bool {
    parts.iter().any(|p| !p.ok && !p.fix.is_empty())
}

/// The sidebar's word for Machine: "2 to do", "all good".
pub fn side_word(parts: &[Part], left: usize) -> String {
    match to_do(parts) {
        0 if left > 0 => format!("{left} left behind"),
        0 => "all good".into(),
        n => format!("{n} to do"),
    }
}

/// The banner over the table: its line and the line under it (Go `settings.tsx`).
pub fn banner(parts: &[Part], can_repair: bool) -> (String, String) {
    match to_do(parts) {
        0 => (
            "Everything is in place.".into(),
            "The same report as doze doctor.".into(),
        ),
        n => (
            format!(
                "{n} part{} not in place. Each row below says what puts it there.",
                if n == 1 { " is" } else { "s are" }
            ),
            if can_repair {
                "Repair runs the same setup as doze doctor --fix, behind the system's password prompt."
            } else {
                "From a terminal: doze doctor --fix, one sudo; each row's fix is the command."
            }
            .into(),
        ),
    }
}

/// The banner once the look for leftovers is in: with every part in place but stacks' leftovers on the machine, it
/// does not say all is well (an owner's call past Go, whose banner looked at the parts alone). `left` is how many
/// things were left behind, of which `safe` Clean up clears (data is never cleared that way). The third value is
/// whether the banner warns.
pub fn banner_with(
    parts: &[Part],
    can_repair: bool,
    left: usize,
    safe: usize,
) -> (String, String, bool) {
    let n = to_do(parts);
    if n > 0 || left == 0 {
        let (a, b) = banner(parts, can_repair);
        return (a, b, n > 0);
    }
    let things = if left == 1 { "thing" } else { "things" };
    (
        format!("Every part is in place, but stacks left {left} {things} behind."),
        if safe > 0 {
            "Left behind, below, says what each is; Clean up stops the strays and removes stale routes, and data goes only one folder at a time."
        } else {
            "Left behind, below, says what each is; data goes only one folder at a time, after asking."
        }
        .into(),
        true,
    )
}

/// The report Copy report puts on the clipboard (Go `copyReport`).
pub fn report(parts: &[Part]) -> String {
    parts
        .iter()
        .map(|c| {
            let mut line = format!("{} ", if c.ok { "✓" } else { "✗" });
            if !c.section.is_empty() {
                line.push_str(&format!("{} · ", c.section));
            }
            line.push_str(&c.name);
            if !c.text.is_empty() {
                line.push_str(&format!(" · {}", c.text));
            }
            if !c.ok && !c.fix.is_empty() {
                line.push_str(&format!("   fix: {}", c.fix));
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{Check, Doctor};

    #[test]
    fn needs_you_gets_the_app_part_with_its_link_command() {
        let d = Doctor {
            checks: vec![
                check("names", "network", false),
                check("domains", "route a.example", false),
            ],
        };
        let at = DozeAt {
            on_path: None,
            elsewhere: Some(PathBuf::from("/Users/me/go/bin/doze")),
        };
        let p = attention_parts(&d, &at, "v0.2.0");
        let fixes: Vec<(&str, &str)> = p
            .iter()
            .map(|p| (p.section.as_str(), p.fix.as_str()))
            .collect();
        assert_eq!(
            fixes,
            [
                ("names", "doze dns-setup"),
                ("domains", "doze trust"),
                ("app", "ln -s /Users/me/go/bin/doze /usr/local/bin/doze"),
            ]
        );
        assert_eq!(
            p[2].text,
            "not on your PATH: a terminal and scripts cannot find doze"
        );
        let found = DozeAt {
            on_path: Some(PathBuf::from("/opt/homebrew/bin/doze")),
            elsewhere: None,
        };
        let app = attention_parts(&Doctor::default(), &found, "v0.2.0");
        assert!(app[0].ok && app[0].fix.is_empty());
    }

    fn check(section: &'static str, name: &str, ok: bool) -> Check {
        Check {
            name: name.into(),
            ok,
            text: format!("{name} text"),
            section,
        }
    }

    #[test]
    fn each_failing_part_has_its_fix_and_the_report_says_it() {
        let d = Doctor {
            checks: vec![
                check("names", "network", false),
                check("names", "resolver route", true),
                check("domains", "route shop.test", false),
            ],
        };
        let at = DozeAt {
            on_path: None,
            elsewhere: Some("/Users/x/go/bin/doze".into()),
        };
        let p = parts(&d, &at, "v0.2.0");
        assert_eq!(p.len(), 4);
        assert_eq!(p[0].fix, "doze dns-setup");
        assert_eq!(p[1].fix, "");
        assert_eq!(p[2].fix, "doze trust");
        assert_eq!(p[3].fix, "ln -s /Users/x/go/bin/doze /usr/local/bin/doze");
        assert_eq!(to_do(&p), 3);
        assert_eq!(side_word(&p, 0), "3 to do");
        assert!(fixable(&p));
        assert_eq!(
            banner(&p, true).0,
            "3 parts are not in place. Each row below says what puts it there."
        );
        let r = report(&p);
        assert!(r.starts_with(
            "✗ names · network · network text   fix: doze dns-setup\n✓ names · resolver route"
        ));
        assert_eq!(
            sections(&p).iter().map(|s| s.1).collect::<Vec<_>>(),
            ["names", "domains", "the app"]
        );
    }

    #[test]
    fn all_good() {
        let at = DozeAt {
            on_path: Some("/opt/homebrew/bin/doze".into()),
            elsewhere: None,
        };
        let p = parts(&Doctor::default(), &at, "v0.2.0");
        assert_eq!(p[0].text, "/opt/homebrew/bin/doze · v0.2.0");
        assert_eq!(side_word(&p, 0), "all good");
        assert_eq!(side_word(&p, 3), "3 left behind");
        assert_eq!(banner(&p, false).0, "Everything is in place.");
        let (line, _, warn) = banner_with(&p, false, 2, 1);
        assert_eq!(
            line,
            "Every part is in place, but stacks left 2 things behind."
        );
        assert!(warn);
        assert_eq!(banner_with(&p, false, 0, 0).0, "Everything is in place.");
    }
}

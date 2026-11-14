//! The machine line: whether this machine is set up for doze's names and domains, from `doze --json doctor` (the
//! CLI's machine contract; the setup itself is the names crate's, `N5` in the Rust port) and where the `doze` the app
//! runs is.

use serde::{Deserialize, Serialize};

use crate::view::StackView;

/// One part of the machine's setup.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// `names`, `domains` or `app`: how Settings groups them.
    #[serde(skip_serializing_if = "str::is_empty")]
    pub section: &'static str,
}

/// The machine's checks, part by part.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Doctor {
    pub checks: Vec<Check>,
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    data: Option<Data>,
}

#[derive(Deserialize)]
struct Data {
    #[serde(default)]
    checks: Vec<Line>,
}

#[derive(Deserialize)]
struct Line {
    name: String,
    #[serde(default)]
    detail: String,
    #[serde(default)]
    ok: bool,
}

// fuzz:parser fuzz_doctor
/// What `doze --json doctor` printed, read for the machine: its `names: …` checks, and its domain checks. The
/// stack's own checks (its config, its modules, its ports) are the stack page's, not the machine's.
pub fn parse_doctor(json: &str) -> Option<Doctor> {
    let env: Envelope = serde_json::from_str(json).ok()?;
    let checks = env
        .data?
        .checks
        .into_iter()
        .filter_map(|l| {
            let (section, name) = if let Some(n) = l.name.strip_prefix("names: ") {
                ("names", n.to_owned())
            } else if let Some(n) = l.name.strip_prefix("domains: ") {
                ("domains", n.to_owned())
            } else if l.name.starts_with("domain ") || l.name.starts_with("route ") {
                ("domains", l.name.clone())
            } else {
                return None;
            };
            Some(Check {
                name,
                ok: l.ok,
                text: l.detail,
                section,
            })
        })
        .collect();
    Some(Doctor { checks })
}

/// Whether doze's names are switched off here (`DOZE_ZONE=off`, `0`, `false`, `no`; doze-names `Disabled`).
#[must_use]
pub fn zone_off() -> bool {
    std::env::var("DOZE_ZONE").is_ok_and(|z| {
        matches!(
            z.trim().to_lowercase().as_str(),
            "off" | "0" | "false" | "no"
        )
    })
}

/// With the names switched off, the machine's names part is that, and no more (Go `doctor`: "switched off
/// (DOZE_ZONE)"), and the domains are not looked at.
pub fn names_off(d: &mut Doctor) {
    d.checks
        .retain(|c| c.section != "names" && c.section != "domains");
    d.checks.insert(
        0,
        Check {
            name: "names".into(),
            ok: false,
            text: "switched off (DOZE_ZONE)".into(),
            section: "names",
        },
    );
}

/// The command that sets a part up, as Go's doctor names it: `doze dns-setup` for a names step, `doze trust` for a
/// domain; none for the names switched off.
#[must_use]
pub fn fix_of(c: &Check) -> &'static str {
    match c.section {
        _ if c.ok => "",
        "names" if c.text.contains("DOZE_ZONE") => "",
        "names" => "doze dns-setup",
        "domains" => "doze trust",
        _ => "",
    }
}

/// The line itself (Go `machine`): each names step, then one summary of the domains, then routes no stack declares.
pub fn machine(doctor: &Doctor, stacks: &[StackView]) -> Vec<Check> {
    let mut out = Vec::new();
    let (mut domains_ok, mut any_domain) = (true, false);
    let mut stale: Vec<&str> = Vec::new();
    for c in &doctor.checks {
        match c.section {
            "names" => out.push(c.clone()),
            "domains" if c.name.starts_with("stale route ") => {
                stale.push(c.name.trim_start_matches("stale route "));
            }
            "domains" => {
                any_domain = true;
                domains_ok &= c.ok;
            }
            _ => {}
        }
    }
    if any_domain {
        let mut names: Vec<&str> = Vec::new();
        for d in stacks
            .iter()
            .flat_map(|s| s.instances.iter())
            .flat_map(|v| v.domains.iter())
            .map(|d| d.strip_prefix("*.").unwrap_or(d))
        {
            if !names.contains(&d) {
                names.push(d);
            }
        }
        let mut text = names.join(", ");
        if !domains_ok {
            text.push_str(" · not set up yet");
        }
        out.push(Check {
            name: "domains".into(),
            ok: domains_ok,
            text,
            section: "",
        });
    }
    if !stale.is_empty() {
        out.push(Check {
            name: "stale routes".into(),
            ok: false,
            text: format!("{} · no stack declares them", stale.join(", ")),
            section: "",
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Doctor, machine, parse_doctor};

    #[test]
    fn keeps_the_machines_checks() {
        let json = r#"{"ok":false,"data":{"ok":false,"checks":[
            {"name":"config","detail":"x","ok":true},
            {"name":"names: network","detail":"the doze network daemon is not running","ok":false},
            {"name":"names: resolver route","detail":"/etc/resolver/doze → 127.0.0.1:5323","ok":true},
            {"name":"domains: certificate authority","detail":"trusted","ok":true}]}}"#;
        let d = parse_doctor(json).unwrap();
        assert_eq!(d.checks.len(), 3);
        assert_eq!(d.checks[0].section, "names");
        let line = machine(&d, &[]);
        let names: Vec<&str> = line.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["network", "resolver route", "domains"]);
        assert!(machine(&Doctor::default(), &[]).is_empty());
        assert!(parse_doctor("{").is_none());
    }
}

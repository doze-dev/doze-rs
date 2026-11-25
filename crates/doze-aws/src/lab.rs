//! The Lab's switch: `--lab`, on by default, and the one place that decides whether the fault table is installed.
//!
//! The Lab lets whoever can reach the console make the calls apps send to doze-aws slow, fail or drop. So it needs a console to make
//! rules from (off with `--console off`), and it is off by default wherever the console is served beyond this machine, unless
//! `--lab` is asked for plainly (then the start says anyone who can reach the console can inject faults). Today the console is
//! served on loopback only (`console::address` refuses any other); the rule is here for the day it is not, and tested.

use std::net::SocketAddr;

/// How loudly a line of the start is said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Level {
    Info,
    Warn,
}

/// What `--lab` came to.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Plan {
    /// The fault table is installed.
    pub(crate) on: bool,
    /// Why it is not, when it is not.
    pub(crate) off_why: Option<String>,
    /// It is on and the console is served beyond this machine.
    pub(crate) open: bool,
    /// What the start says of it.
    pub(crate) lines: Vec<(Level, String)>,
}

/// A word of `--lab`: `Some(true)` for on, `Some(false)` for off, `None` for no word of it.
pub(crate) fn word(text: &str) -> Option<bool> {
    match text.trim().to_ascii_lowercase().as_str() {
        "" | "on" | "true" | "yes" | "1" => Some(true),
        "off" | "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

/// Decides the Lab. `value` is what `--lab` says and `plainly` whether anyone said it (a default is not asking); `console` is where
/// the console listens, `None` with the console off.
pub(crate) fn plan(value: &str, plainly: bool, console: Option<SocketAddr>) -> Result<Plan, String> {
    let Some(asked) = word(value) else {
        return Err(format!(
            "--lab takes true, false, on or off, not {value:?}; the Lab stays off until it is asked for plainly"
        ));
    };
    let off = |why: String, lines: Vec<(Level, String)>| Plan {
        on: false,
        off_why: Some(why),
        open: false,
        lines,
    };
    let Some(addr) = console else {
        let lines = if plainly && asked {
            vec![(
                Level::Warn,
                "--lab has nothing to do: the Lab's rules are made from the console, which is off".to_owned(),
            )]
        } else {
            Vec::new()
        };
        return Ok(off(
            "The Lab is off because the console is off: its rules are made from the console.".to_owned(),
            lines,
        ));
    };
    if plainly && !asked {
        return Ok(off(
            "The Lab is off: doze-aws was started with --lab=false.".to_owned(),
            Vec::new(),
        ));
    }
    let beyond = !addr.ip().is_loopback();
    if beyond && !(plainly && asked) {
        return Ok(off(
            "The Lab is off because the console is served beyond this machine: --lab=true switches it on.".to_owned(),
            vec![(
                Level::Warn,
                "the Lab is off because the console is served beyond loopback (--lab=true switches it on)".to_owned(),
            )],
        ));
    }
    let line = if beyond {
        (
            Level::Warn,
            "the Lab is on (--lab) and the console is served beyond loopback: anyone who can reach the console can inject faults into the calls apps send here (--lab=false turns it off)".to_owned(),
        )
    } else {
        (
            Level::Info,
            "the Lab is on: rules that slow, fail or drop calls can be made from the console, are kept in memory only, and none exists now".to_owned(),
        )
    };
    Ok(Plan {
        on: true,
        off_why: None,
        open: beyond,
        lines: vec![line],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(addr: &str) -> Option<SocketAddr> {
        Some(addr.parse().unwrap())
    }

    #[test]
    fn the_lab_is_on_by_default_and_off_with_its_flag_or_with_the_console() {
        let on = plan("on", false, at("127.0.0.1:4567")).unwrap();
        assert!(on.on && on.off_why.is_none() && !on.open);
        assert_eq!(on.lines.len(), 1);
        assert!(on.lines[0].1.contains("none exists now"));
        for yes in ["true", "TRUE", "yes", "1", ""] {
            assert!(plan(yes, true, at("127.0.0.1:4567")).unwrap().on, "{yes:?}");
        }
        for no in ["false", "off", "No", "0", "FALSE"] {
            let p = plan(no, true, at("127.0.0.1:4567")).unwrap();
            assert!(!p.on, "{no}");
            assert!(p.off_why.unwrap().contains("--lab=false"));
            assert!(p.lines.is_empty());
        }
        // No console, no Lab, whatever it is asked for; asking for it plainly is said to be pointless.
        let p = plan("on", false, None).unwrap();
        assert!(!p.on && p.lines.is_empty());
        assert!(p.off_why.unwrap().contains("console is off"));
        let p = plan("true", true, None).unwrap();
        assert!(!p.on);
        assert!(p.lines[0].1.contains("nothing to do"));
        let said = plan("maybe", true, at("127.0.0.1:4567")).unwrap_err();
        assert!(said.contains("--lab takes true, false, on or off"), "{said}");
    }

    #[test]
    fn a_console_open_to_other_machines_has_the_lab_off_unless_asked_and_then_says_it_can_inject_faults() {
        for open in ["0.0.0.0:4567", "192.168.1.20:4567", "[::]:4567"] {
            let p = plan("on", false, at(open)).unwrap();
            assert!(!p.on, "{open}");
            assert!(p.off_why.unwrap().contains("--lab=true"));
            assert_eq!(
                p.lines,
                [(
                    Level::Warn,
                    "the Lab is off because the console is served beyond loopback (--lab=true switches it on)".to_owned()
                )]
            );
            // Asked for plainly, it is on, and the start says what that means.
            let p = plan("true", true, at(open)).unwrap();
            assert!(p.on && p.open, "{open}");
            assert_eq!(p.lines[0].0, Level::Warn);
            assert!(p.lines[0].1.contains("anyone who can reach the console can inject faults"));
            // And asked to be off, it says nothing of it.
            let p = plan("false", true, at(open)).unwrap();
            assert!(!p.on && p.lines.is_empty());
        }
        // On this machine alone nobody else can reach it.
        assert!(!plan("on", false, at("[::1]:4567")).unwrap().open);
    }
}

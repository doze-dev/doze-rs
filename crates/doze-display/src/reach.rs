//! What an instance is, what it runs, what it waits on, and where to reach it.

use std::borrow::Cow;

use crate::format::mb;
use crate::look::{Look, awake_now, look};
use crate::model::{Holder, Instance, Kind, Outcome, Snapshot, State};

/// What it runs: the command of an app or a service, the address of a module. (Go: `Runs`.)
#[must_use]
pub fn runs(v: &Instance) -> Cow<'_, str> {
    if v.kind == Kind::Module {
        match address(v) {
            Some(a) => a,
            None => engine_version(v),
        }
    } else {
        short_run(&v.run)
    }
}

/// "postgres 17.2": the engine and its version, without a `builtin` prefix.
pub(crate) fn engine_version(v: &Instance) -> Cow<'_, str> {
    let version = v.version.strip_prefix("builtin").unwrap_or(&v.version);
    match (v.engine.is_empty(), version.is_empty()) {
        (false, true) => Cow::Borrowed(v.engine.trim()),
        (true, false) => Cow::Borrowed(version.trim()),
        _ => Cow::Owned(format!("{} {version}", v.engine).trim().to_owned()),
    }
}

/// Flags of `docker run` and `podman run` that take the next word as their value.
const TAKES_VALUE: &[&str] = &[
    "-p",
    "--publish",
    "-e",
    "--env",
    "--env-file",
    "-v",
    "--volume",
    "--name",
    "--network",
    "--net",
    "-w",
    "--workdir",
    "-u",
    "--user",
    "--entrypoint",
    "--platform",
    "-l",
    "--label",
    "--add-host",
    "-m",
    "--memory",
    "--cpus",
    "-h",
    "--hostname",
    "--mount",
];

/// A command line read the way the board does: what it runs, not the whole line. A container command says which
/// image. (Go: `ShortRun`.)
#[must_use]
pub fn short_run(run: &str) -> Cow<'_, str> {
    let mut words = run.split_whitespace();
    let (Some(tool @ ("docker" | "podman")), Some("run")) = (words.next(), words.next()) else {
        return Cow::Borrowed(run);
    };
    // the image is the first word that is neither a flag nor a flag's value
    while let Some(word) = words.next() {
        if !word.starts_with('-') {
            return Cow::Owned(format!("{tool} run {word}"));
        }
        if TAKES_VALUE.contains(&word) {
            words.next();
        }
    }
    Cow::Owned(format!("{tool} run"))
}

/// The first thing `v` needs that is not up yet. (Go: `FirstUnmet`.)
#[must_use]
pub fn first_unmet<'a>(v: &'a Instance, snap: &Snapshot) -> Option<&'a str> {
    v.needs
        .iter()
        .map(String::as_str)
        .find(|n| {
            snap.find(n).is_some_and(|o| {
                !awake_now(o) && o.task.as_ref().is_none_or(|t| t.outcome != Outcome::Done)
            })
        })
        .filter(|n| !n.is_empty()) // an empty name is no name
}

/// The first dependant holding `v` awake. (Go: `HolderNamed`.)
#[must_use]
pub fn holder_named(v: &Instance) -> Option<&str> {
    v.holders
        .iter()
        .find_map(crate::note::dependant)
        .filter(|n| !n.is_empty())
}

/// The memory column: a figure, a dash while it wakes, `0` asleep, and nothing for a task. (Go: `Mem`.)
#[must_use]
pub fn mem(v: &Instance) -> Cow<'static, str> {
    if v.kind == Kind::Task {
        Cow::Borrowed("")
    } else if v.state == State::Booting {
        Cow::Borrowed("—")
    } else if awake_now(v) {
        Cow::Owned(mb(i64::try_from(v.ram).unwrap_or(i64::MAX)))
    } else {
        Cow::Borrowed("0")
    }
}

/// Where to reach an instance, as a page draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reach<'a> {
    /// The connection string of a module, or the name of an app.
    pub addr: Cow<'a, str>,
    /// `:8080` when the name stands in front of a port, drawn dim after the address.
    pub port: Option<&'a str>,
}

/// Where to reach it: the connection string of a module, the name of an app with the port it listens on behind
/// it. `None` when there is nowhere. (Go: `URL`.)
#[must_use]
pub fn reach(v: &Instance) -> Option<Reach<'_>> {
    fn only(addr: &str) -> Reach<'_> {
        Reach {
            addr: Cow::Borrowed(addr),
            port: None,
        }
    }
    if !v.url.is_empty() {
        Some(only(&v.url))
    } else if !v.resource.is_empty() {
        Some(only(&v.resource))
    } else if !v.domain.is_empty() {
        let port = v
            .bind
            .find(':')
            .filter(|_| !v.bind.starts_with("unix:"))
            .and_then(|i| v.bind.get(i..));
        Some(Reach {
            addr: Cow::Owned(format!("http://{}", v.domain)),
            port,
        })
    } else if !v.endpoint.is_empty() {
        Some(only(&v.endpoint))
    } else {
        None
    }
}

/// Where a client connects: the name and port where there is a name, else the address itself; `None` when there
/// is nothing to connect to. (Go: `Address`.)
#[must_use]
pub fn address(v: &Instance) -> Option<Cow<'_, str>> {
    if v.endpoint.is_empty() {
        return None;
    }
    if v.domain.is_empty() {
        let bare = v.endpoint.strip_prefix("http://").unwrap_or(&v.endpoint);
        return (!bare.is_empty()).then_some(Cow::Borrowed(bare));
    }
    match v.endpoint.split_once(':') {
        Some((_, port)) if !v.endpoint.starts_with("unix:") => {
            Some(Cow::Owned(format!("{}:{port}", v.domain)))
        }
        _ => Some(Cow::Borrowed(&v.domain)),
    }
}

/// What to paste for an instance: the address alone. A page draws `http://api.shop.doze → :8080`, which is a
/// description, not something a browser takes. (Go: `AddressOf`.)
#[must_use]
pub fn address_of(v: &Instance) -> Cow<'_, str> {
    if !v.url.is_empty() {
        Cow::Borrowed(&v.url)
    } else if !v.resource.is_empty() {
        Cow::Borrowed(&v.resource)
    } else if !v.domain.is_empty() {
        Cow::Owned(format!("http://{}", v.domain))
    } else {
        Cow::Borrowed(&v.endpoint)
    }
}

/// One thing an instance waits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Need<'a> {
    /// Its name.
    pub name: &'a str,
    /// How it is, when it is in the stack; an unknown name is drawn as it is.
    pub known: Option<Known>,
}

/// How a thing waited on is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Known {
    /// Its look.
    pub look: Look,
    /// It is a task that has run.
    pub ran: bool,
}

impl Need<'_> {
    /// The glyph drawn before it: its look's, or `✓` when it is up or done; `None` for an unknown name.
    #[must_use]
    pub fn glyph(&self) -> Option<&'static str> {
        self.known.map(|k| match k.look {
            Look::Awake | Look::Done => "✓",
            other => other.glyph(),
        })
    }
}

/// Each thing `v` waits on, with how it is. (Go: `Needs`.)
#[must_use]
pub fn needs<'a>(v: &'a Instance, snap: &Snapshot) -> Vec<Need<'a>> {
    v.needs
        .iter()
        .map(|n| Need {
            name: n,
            known: snap.find(n).map(|o| Known {
                look: look(o),
                ran: o.task.is_some(),
            }),
        })
        .collect()
}

/// How many connections are open to it right now. (Go: `Connections`.)
#[must_use]
pub fn connections(v: &Instance) -> u32 {
    v.holders
        .iter()
        .find_map(|h| {
            if let Holder::Connections(n) = h {
                Some(*n)
            } else {
                None
            }
        })
        .unwrap_or(0)
}

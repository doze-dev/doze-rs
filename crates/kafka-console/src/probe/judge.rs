//! What the three measurements of the advertised-listener check come to, in the board's words. Pure: the dials are
//! made elsewhere, so every case here is a table row.

use crate::format::millis;
use crate::model::{ProbeView, Rule, Span, Verdict};

/// One dial: whether it was answered, how long it took, and (when it was not) why, in a few words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dial {
    pub answered: bool,
    pub ms: u64,
    /// `no such host`, `connection refused`, `timed out`, `connected, but it did not answer ApiVersions`.
    pub why: String,
}

/// What Metadata named as the broker (`host:port`), or nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Named {
    Broker(String),
    /// The call was refused or named no broker, or several.
    Nothing(String),
}

/// What was measured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Measured {
    pub listen: String,
    /// The version of Metadata that was sent.
    pub metadata_version: i16,
    /// The address the broker is configured to tell clients about, `host:port`.
    pub advertised: String,
    /// Whether the listener serves TLS: it is then only dialled, not asked ApiVersions.
    pub tls: bool,
    pub listener: Dial,
    pub metadata: Named,
    /// `None`: the dial of the advertised address did not finish in time.
    pub advertised_dial: Option<Dial>,
}

/// The rules and the verdict of a measurement.
#[must_use]
pub fn judge(m: &Measured) -> ProbeView {
    let same = m.listen == m.advertised;
    let rules = vec![listener_rule(m), metadata_rule(m, same), dial_rule(m)];
    let verdict = if m.advertised_dial.is_none() {
        Verdict::NotChecked
    } else if rules.iter().all(|r| r.ok) {
        Verdict::Reachable
    } else {
        Verdict::NotReachable
    };
    ProbeView {
        verdict,
        rules: if verdict == Verdict::NotChecked {
            Vec::new()
        } else {
            rules
        },
        listen: m.listen.clone(),
        advertised: m.advertised.clone(),
        same,
        metadata_version: m.metadata_version,
    }
}

fn listener_rule(m: &Measured) -> Rule {
    let d = &m.listener;
    let (ok, title, detail, value) = if d.answered {
        let how = if m.tls {
            "and connected (it serves TLS, so ApiVersions was not asked)"
        } else {
            "and asked ApiVersions"
        };
        (
            true,
            "The listener answers",
            vec![Span::text(format!("Dialled {} {how}", m.listen))],
            millis(d.ms),
        )
    } else {
        (
            false,
            "The listener does not answer",
            vec![
                Span::text("Dialled "),
                Span::mono(&m.listen),
                Span::text(format!(": {}", d.why)),
            ],
            "failed".to_owned(),
        )
    };
    Rule {
        ok,
        title: title.to_owned(),
        detail,
        value,
    }
}

fn metadata_rule(m: &Measured, same: bool) -> Rule {
    let (ok, title, detail) = match &m.metadata {
        Named::Broker(named) if *named == m.advertised && same => (
            true,
            "It says where clients should reconnect",
            vec![
                Span::text("Metadata names this broker at "),
                Span::mono(named),
                Span::text(": the advertised host and port equal the listener"),
            ],
        ),
        Named::Broker(named) if *named == m.advertised => (
            true,
            "It says where clients should reconnect",
            vec![
                Span::text("Metadata names this broker at "),
                Span::mono(named),
                Span::text(", not at the address it listens on ("),
                Span::mono(&m.listen),
                Span::text(")"),
            ],
        ),
        Named::Broker(named) => (
            false,
            "It names another address than the one it advertises",
            vec![
                Span::text("Metadata names this broker at "),
                Span::mono(named),
                Span::text(", not "),
                Span::mono(&m.advertised),
            ],
        ),
        Named::Nothing(why) => (
            false,
            "It does not say where clients should reconnect",
            vec![Span::text(format!("Metadata: {why}"))],
        ),
    };
    Rule {
        ok,
        title: title.to_owned(),
        detail,
        value: "Metadata".to_owned(),
    }
}

fn dial_rule(m: &Measured) -> Rule {
    match &m.advertised_dial {
        Some(d) if d.answered => Rule {
            ok: true,
            title: "That address is reachable".to_owned(),
            detail: vec![Span::text(
                "Dialled the advertised address back and got an answer",
            )],
            value: millis(d.ms),
        },
        Some(d) => Rule {
            ok: false,
            title: "That address is not reachable".to_owned(),
            detail: vec![
                Span::text("Dialled "),
                Span::mono(&m.advertised),
                Span::text(format!(": {}", d.why)),
            ],
            value: "failed".to_owned(),
        },
        None => Rule {
            ok: false,
            title: "That address did not answer in time".to_owned(),
            detail: vec![
                Span::text("Dialled "),
                Span::mono(&m.advertised),
                Span::text(": no answer yet"),
            ],
            value: "waiting".to_owned(),
        },
    }
}

#[cfg(test)]
mod tests;

//! The kinds of forced event, in the order of the Lab's cards (the order of what is worth most to a developer), and whether each can
//! be carried out in this run.

use crate::model::LabOnceKind;
use crate::state::Shared;

/// One kind: what it is called, what it does, and the service whose resources it works on.
#[derive(Debug)]
pub struct KindDef {
    /// In addresses and in the history: `fire-schedule`.
    pub id: &'static str,
    pub title: &'static str,
    /// One sentence: what it does.
    pub lede: &'static str,
    /// What its choices are.
    pub noun: &'static str,
    /// The service (its `--services` name) whose resources it acts on; `None` for resting, which is about every service.
    pub service: Option<&'static str>,
}

/// Resting a service is the console's own (the host's register of lazy services); every other kind is a service's
/// (`aws_wire::forced::kind`).
pub const REST: &str = "rest-service";

/// Every kind, in order.
pub static KINDS: [KindDef; 5] = [
    KindDef {
        id: REST,
        title: "Rest a service",
        lede: "Close a service the way its idle timer does, so its next call opens it again and you see what a first call costs.",
        noun: "service",
        service: None,
    },
    KindDef {
        id: aws_wire::forced::kind::FIRE_SCHEDULE,
        title: "Fire a schedule now",
        lede: "Invoke a schedule's target now, by the path its timer takes, without waiting for its time.",
        noun: "schedule",
        service: Some("scheduler"),
    },
    KindDef {
        id: aws_wire::forced::kind::EXPIRE_VISIBILITY,
        title: "Let an in-flight message time out",
        lede: "Bring a queue's in-flight messages back now, as if their visibility timeout had run out.",
        noun: "queue",
        service: Some("sqs"),
    },
    KindDef {
        id: aws_wire::forced::kind::SWEEP_TTL,
        title: "Run the time-to-live sweep",
        lede: "Delete the items of a DynamoDB table whose time to live has passed, now.",
        noun: "table",
        service: Some("dynamodb"),
    },
    KindDef {
        id: aws_wire::forced::kind::COLD_START,
        title: "Make the next call cold",
        lede: "End a function's warm processes, so its next invocation starts a new one and reports an Init Duration.",
        noun: "function",
        service: Some("lambda"),
    },
];

/// The kind called `id`.
#[must_use]
pub fn find(id: &str) -> Option<&'static KindDef> {
    KINDS.iter().find(|k| k.id == id)
}

impl KindDef {
    /// Why this kind cannot be carried out in this run, in words; `None` when it can.
    #[must_use]
    pub fn why_not(&self, s: &Shared) -> Option<String> {
        let Some(peers) = s.facts.peers.as_ref() else {
            return Some("This console was started without the services' bus.".to_owned());
        };
        let Some(service) = self.service else {
            return peers
                .rests()
                .is_none()
                .then(|| "This doze-aws was not given the register of its services.".to_owned());
        };
        let title = s
            .facts
            .services
            .iter()
            .find(|v| v.name == service)
            .map_or(service, |v| v.title);
        if !s
            .facts
            .services
            .iter()
            .any(|v| v.name == service && v.served)
        {
            return Some(format!(
                "{title} is not served in this run (--services), so there is nothing of it to force."
            ));
        }
        let forced = peers
            .targets()
            .into_iter()
            .any(|(name, api)| name == service && api.forced().is_some());
        (!forced).then(|| format!("This build's {title} has no forced events."))
    }

    /// The kind as the index shows it.
    #[must_use]
    pub fn shown(&self, s: &Shared) -> LabOnceKind {
        let why_not = self.why_not(s);
        LabOnceKind {
            id: self.id.to_owned(),
            title: self.title.to_owned(),
            lede: self.lede.to_owned(),
            available: why_not.is_none(),
            why_not,
        }
    }
}

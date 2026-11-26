//! Resting a service: the host's register of lazy services ([`aws_peers::Rests`]) tells a service's actor in a region to close, which is
//! what its idle timer does after `--idle-secs` without a call. The next call opens it again; what is due (a schedule, a timer) still runs
//! and starts it when it is due. Nothing of the service's data is touched: it is closed, not emptied.

use std::time::Instant;

use serde_json::Map;

use crate::model::{LabOnceChoice, LabOnceForm, LabOnceOption};
use crate::resources::bus::Bus;
use crate::resources::{catalog, run};
use crate::state::Shared;

use super::Done;

/// Services with nothing of their own to close: STS answers from IAM's state.
const NO_STATE: &[&str] = &["sts"];

/// The lazy services of `service` (its `--services` name) in `region`: their names and keys in the register, and whether one runs.
fn of(s: &Shared, service: &str, region: &str) -> Vec<(&'static str, String, bool)> {
    let Some(rests) = s.facts.peers.as_ref().and_then(|p| p.rests()) else {
        return Vec::new();
    };
    rests
        .seen()
        .into_iter()
        .filter(|e| s.facts.catalogue_name(e.name) == Some(service))
        // A key-less service (IAM) is the same in every region.
        .filter(|e| e.key == region || e.key.is_empty())
        .map(|e| (e.name, e.key, e.live))
        .collect()
}

/// Whether `service` runs in `region` (a service with no lazy service made there is resting: nothing has called it).
pub(crate) fn running(s: &Shared, service: &str, region: &str) -> bool {
    of(s, service, region).iter().any(|(_, _, live)| *live)
}

/// The form: every served service, with whether it runs in `region`, running ones first.
pub(crate) fn form(s: &Shared, region: &str, chosen: Option<&str>) -> LabOnceForm {
    let mut choices: Vec<LabOnceChoice> = s
        .facts
        .services
        .iter()
        .filter(|v| v.served && !NO_STATE.contains(&v.name))
        .map(|v| {
            let made = of(s, v.name, region);
            let live = made.iter().any(|(_, _, live)| *live);
            LabOnceChoice {
                value: v.name.to_owned(),
                label: v.title.to_owned(),
                detail: made
                    .is_empty()
                    .then(|| format!("Not opened in {region} in this run.")),
                state: Some(if live { "running" } else { "resting" }.to_owned()),
            }
        })
        .collect();
    choices.sort_by_key(|c| c.state.as_deref() != Some("running"));
    let pick = chosen
        .and_then(|c| choices.iter().find(|x| x.value == c))
        .or_else(|| choices.first());
    let preview = pick.map(|c| preview(s, c, region));
    LabOnceForm {
        kind: super::kinds::REST.to_owned(),
        region: region.to_owned(),
        noun: "service".to_owned(),
        empty: choices
            .is_empty()
            .then(|| "This run serves no service that holds anything to close.".to_owned()),
        choices,
        resting: false,
        note: Some(format!(
            "Nothing is called to draw this list: it says which services run in {region} and which rest. A service rests by itself after {} without a call.",
            idle_words(crate::api::home::idle_secs(s))
        )),
        preview,
        option: Some(LabOnceOption {
            id: "time_wake".to_owned(),
            label: "Then call it, and time how long the first call takes".to_owned(),
            default: true,
        }),
    }
}

fn preview(s: &Shared, c: &LabOnceChoice, region: &str) -> String {
    if c.state.as_deref() == Some("running") {
        format!(
            "Close {} in {region} now, as its idle timer would after {}. Its next call opens it again; what is due there still runs, and starts it when it is due.",
            c.label,
            idle_words(crate::api::home::idle_secs(s))
        )
    } else {
        format!(
            "{} is already resting in {region}: it holds no memory and no open files, so forcing it would do nothing.",
            c.label
        )
    }
}

/// `5 minutes`, `90 seconds`.
pub(crate) fn idle_words(secs: u64) -> String {
    if secs >= 120 && secs.is_multiple_of(60) {
        format!("{} minutes", secs / 60)
    } else {
        format!("{secs} seconds")
    }
}

/// Rests `service` in `region`: its lazy services there close, as their idle timers close them. With `time_wake`, the first read of
/// the service after it, and the same read again, are timed.
pub(crate) async fn rest(s: &Shared, region: &str, service: &str, time_wake: bool) -> Done {
    let title = s
        .facts
        .services
        .iter()
        .find(|v| v.name == service)
        .map_or(service, |v| v.title);
    let subject = service.to_owned();
    let Some(rests) = s.facts.peers.as_ref().and_then(|p| p.rests()) else {
        return Done::refused(&subject, "There is no register of the services to rest.");
    };
    let mut closed = false;
    for (name, key, live) in of(s, service, region) {
        if live && rests.rest(name, &key).await == aws_peers::Rested::Closed {
            closed = true;
        }
    }
    if !closed {
        return Done::refused(
            &subject,
            &format!(
                "{title} is already resting in {region}: it holds no memory and no open files. What is due there (a timer, a schedule) still runs, and starts it when it is due."
            ),
        );
    }
    s.once.rested(service, region);
    let mut sentence = format!(
        "Rested {title} in {region}: it closed as it does after {} without a call, and its data stays on disk. Its next call opens it again.",
        idle_words(crate::api::home::idle_secs(s))
    );
    if time_wake {
        sentence.push(' ');
        sentence.push_str(&wake(s, region, service).await);
    }
    Done::ran(&subject, &sentence)
}

/// The first read of the service after it rested, and the same read again, each timed: what waking it costs.
async fn wake(s: &Shared, region: &str, service: &str) -> String {
    let Some((peers, d)) = s
        .facts
        .peers
        .clone()
        .zip(catalog::descriptor(service))
        .filter(|_| s.bus().is_some())
    else {
        return "It has no read the Lab can time.".to_owned();
    };
    let Some(kind) = d.kinds.first() else {
        return "It has no read the Lab can time.".to_owned();
    };
    let bus = Bus::new(peers, &s.facts.account, region, &s.facts.endpoint);
    let (first, again) = (read(&bus, d, kind).await, read(&bus, d, kind).await);
    let op = kind.list.step.op;
    match (first, again) {
        (Ok(a), Ok(b)) => format!(
            "The first call after it ({op}) took {}, which included opening the service; the same call again took {}.",
            ms(a),
            ms(b)
        ),
        (Err(e), _) | (_, Err(e)) => format!(
            "The call to time it ({op}) was refused: {}.",
            e.message.trim_end_matches('.')
        ),
    }
}

/// One timed read of the first kind's list, in microseconds.
async fn read(
    bus: &Bus,
    d: &crate::resources::Descriptor,
    kind: &crate::resources::Kind,
) -> Result<u128, aws_wire::ApiError> {
    let started = Instant::now();
    run::page(bus, d, kind, None, &Map::new()).await?;
    Ok(started.elapsed().as_micros())
}

/// `0.8 ms`, `12 ms`, `1,240 ms`.
fn ms(us: u128) -> String {
    if us >= 10_000 {
        let whole = us / 1000;
        if whole >= 1000 {
            format!("{},{:03} ms", whole / 1000, whole % 1000)
        } else {
            format!("{whole} ms")
        }
    } else if us < 100 {
        "under 0.1 ms".to_owned()
    } else {
        format!("{:.1} ms", us as f64 / 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_is_said_in_milliseconds_with_a_decimal_when_it_is_small() {
        assert_eq!(ms(40), "under 0.1 ms");
        assert_eq!(ms(800), "0.8 ms");
        assert_eq!(ms(9_940), "9.9 ms");
        assert_eq!(ms(12_300), "12 ms");
        assert_eq!(ms(1_240_999), "1,240 ms");
    }

    #[test]
    fn an_idle_time_is_said_in_minutes_when_it_is_whole_ones() {
        assert_eq!(idle_words(300), "5 minutes");
        assert_eq!(idle_words(90), "90 seconds");
        assert_eq!(idle_words(60), "60 seconds");
    }
}

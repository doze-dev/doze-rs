//! Carrying a forced event out: the service's own function behind the check its own call meets, asked as the console (the account's
//! root, which IAM lets do what the person running doze-aws can), and the answer in a sentence. A refusal in words is an answer too:
//! nothing to do, a right missing, a resource that is not there.

use aws_wire::forced::Step;
use serde_json::{Value, json};

use super::kinds::{KindDef, REST};
use super::{Done, rest};
use crate::model::LabOnceDone;
use crate::resources::bus::Bus;
use crate::state::Shared;

/// What a service is asked, from the form's choice.
fn input_of(kind: &str, choice: &str) -> Value {
    match kind {
        aws_wire::forced::kind::FIRE_SCHEDULE => {
            let (group, name) = choice.split_once('/').unwrap_or(("default", choice));
            json!({"group": group, "name": name})
        }
        aws_wire::forced::kind::EXPIRE_VISIBILITY => json!({"queue": choice}),
        aws_wire::forced::kind::SWEEP_TTL => json!({"table": choice}),
        _ => json!({"function": choice}),
    }
}

/// A path segment of the console's addresses: everything but unreserved characters percent-encoded.
fn segment(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(b).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// Where to see what a forced event did: the resource's page, or Live's calls of the service.
pub(crate) fn link(kind: &str, choice: &str) -> Option<String> {
    Some(match kind {
        REST => format!("/live?console=shown&service={}", segment(choice)),
        aws_wire::forced::kind::FIRE_SCHEDULE => {
            format!("/resources/scheduler/schedules/{}", segment(choice))
        }
        aws_wire::forced::kind::EXPIRE_VISIBILITY => {
            format!("/resources/sqs/queues/{}", segment(choice))
        }
        aws_wire::forced::kind::SWEEP_TTL => {
            format!("/resources/dynamodb/tables/{}", segment(choice))
        }
        aws_wire::forced::kind::COLD_START => {
            format!("/resources/lambda/functions/{}", segment(choice))
        }
        _ => return None,
    })
}

/// Carries out `def` for the body's choice in `region`.
///
/// # Errors
/// The words for a request that is not one (no choice named).
pub(crate) async fn force(
    s: &Shared,
    def: &KindDef,
    region: &str,
    body: &Value,
) -> Result<LabOnceDone, String> {
    let choice = body
        .get("choice")
        .and_then(Value::as_str)
        .filter(|c| !c.is_empty() && c.len() <= 256)
        .ok_or_else(|| format!("Choose a {}.", def.noun))?;
    let done = carry_out(s, def, region, choice, body).await;
    s.once
        .add(def.id, &done.sentence, done.ok, &done.subject, region);
    Ok(LabOnceDone {
        ok: done.ok,
        sentence: done.sentence,
        subject: done.subject,
        link: link(def.id, choice),
    })
}

async fn carry_out(s: &Shared, def: &KindDef, region: &str, choice: &str, body: &Value) -> Done {
    if let Some(why) = def.why_not(s) {
        return Done::refused(choice, &why);
    }
    if def.id == REST {
        let time = body
            .get("time_wake")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        return rest::rest(s, region, choice, time).await;
    }
    let (Some(peers), Some(service)) = (s.facts.peers.clone(), def.service) else {
        return Done::refused(choice, "There is no bus to reach the service by.");
    };
    let Some(api) = peers
        .targets()
        .into_iter()
        .find_map(|(name, api)| (name == service).then_some(api))
    else {
        return Done::refused(choice, "The service is not on the bus.");
    };
    let Some(forced) = api.forced() else {
        return Done::refused(choice, "The service has no forced events.");
    };
    let bus = Bus::new(peers, &s.facts.account, region, &s.facts.endpoint);
    match forced
        .force(bus.caller(), def.id, &input_of(def.id, choice))
        .await
    {
        Ok(Step::Done(d)) => Done::ran(&d.subject, &d.sentence),
        Ok(Step::Nothing(why)) => Done::refused(choice, &why),
        Err(e) => Done::refused(choice, &format!("{}: {}", e.code, e.message)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choice_of_a_schedule_names_its_group_and_the_others_name_themselves() {
        assert_eq!(
            input_of("fire-schedule", "reports/nightly"),
            json!({"group": "reports", "name": "nightly"})
        );
        assert_eq!(
            input_of("fire-schedule", "nightly"),
            json!({"group": "default", "name": "nightly"})
        );
        assert_eq!(input_of("expire-visibility", "q"), json!({"queue": "q"}));
        assert_eq!(input_of("sweep-ttl", "t"), json!({"table": "t"}));
        assert_eq!(input_of("cold-start", "f"), json!({"function": "f"}));
    }

    #[test]
    fn a_link_names_the_page_that_shows_the_result_with_the_name_encoded() {
        assert_eq!(
            link("fire-schedule", "default/nightly report").as_deref(),
            Some("/resources/scheduler/schedules/default%2Fnightly%20report")
        );
        assert_eq!(
            link("expire-visibility", "orders.fifo").as_deref(),
            Some("/resources/sqs/queues/orders.fifo")
        );
        assert_eq!(
            link("rest-service", "sqs").as_deref(),
            Some("/live?console=shown&service=sqs")
        );
        assert_eq!(link("nothing", "x"), None);
    }
}

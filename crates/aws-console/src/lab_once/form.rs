//! The forms: what there is to choose from in a region, read through the bus with the calls the Resources space makes, and a sentence of
//! what forcing the choice would do.
//!
//! A service that is resting is described as resting and is not called: a list is a call, and a call starts it. The form is asked again
//! with `look=yes` when the person wants it read anyway.

use serde_json::{Map, Value, json};

use super::{Done, kinds::KindDef, rest};
use crate::model::{LabOnceChoice, LabOnceForm};
use crate::resources::bus::Bus;
use crate::resources::{Descriptor, Kind, catalog, run};
use crate::state::Shared;

/// The most rows of a list read for a form: the first page of the service's own list, cut here.
const MOST: usize = 100;
/// The most tables asked for their time to live: each is a call.
const MOST_TABLES: usize = 50;

fn base(def: &KindDef, region: &str) -> LabOnceForm {
    LabOnceForm {
        kind: def.id.to_owned(),
        region: region.to_owned(),
        noun: def.noun.to_owned(),
        choices: Vec::new(),
        resting: false,
        note: None,
        empty: None,
        preview: None,
        option: None,
    }
}

/// The form of `def` for `region`. `chosen` is the choice the preview is for (else the first); `look` reads a resting service anyway.
pub(crate) async fn build(
    s: &Shared,
    def: &KindDef,
    region: &str,
    chosen: Option<&str>,
    look: bool,
) -> LabOnceForm {
    let mut form = base(def, region);
    if let Some(why) = def.why_not(s) {
        form.empty = Some(why);
        return form;
    }
    let Some(service) = def.service else {
        return rest::form(s, region, chosen);
    };
    let title = title_of(s, service);
    if !look && !rest::running(s, service, region) {
        form.resting = true;
        form.note = Some(format!(
            "{title} is resting in {region}: nothing has called it for a while, so it holds nothing in memory. Listing its {}s is a call, which starts it; it rests again when it has been idle.",
            def.noun
        ));
        return form;
    }
    let Some(peers) = s.facts.peers.clone() else {
        return form;
    };
    let bus = Bus::new(peers, &s.facts.account, region, &s.facts.endpoint);
    let Some((d, kind)) = catalog::descriptor(service).and_then(|d| Some((d, d.kinds.first()?)))
    else {
        form.empty = Some(format!("The console cannot list {title}."));
        return form;
    };
    let cut = match read(&bus, (d, kind)).await {
        Ok((rows, more)) => {
            fill(def, &bus, (kind, rows), &mut form).await;
            more
        }
        Err(e) => {
            form.empty = Some(format!("{title} refused the list: {}", e.message));
            false
        }
    };
    if cut {
        let said = format!("Showing the first {MOST} {}s of {region}.", def.noun);
        form.note = Some(match form.note.take() {
            Some(note) => format!("{note} {said}"),
            None => said,
        });
    }
    form.preview = preview(def, &form, chosen);
    if form.choices.is_empty() && form.empty.is_none() {
        form.empty = Some(format!("There is no {} in {region}.", def.noun));
    }
    form
}

fn title_of<'a>(s: &'a Shared, service: &'a str) -> &'a str {
    s.facts
        .services
        .iter()
        .find(|v| v.name == service)
        .map_or(service, |v| v.title)
}

type Rows = Vec<crate::model::ResourceRow>;

/// The first page of the service's first kind, as rows (the calls the Resources page makes for them too).
async fn read(
    bus: &Bus,
    (d, kind): (&Descriptor, &Kind),
) -> Result<(Rows, bool), aws_wire::ApiError> {
    let (mut items, next) = run::page(bus, d, kind, None, &Map::new()).await?;
    let more = next.is_some() || items.len() > MOST;
    items.truncate(MOST);
    let (rows, _) = run::rows(bus, (d, kind), items, true).await;
    Ok((rows, more))
}

/// The cell of `row` in the column called `title`.
fn cell<'a>(kind: &Kind, row: &'a crate::model::ResourceRow, title: &str) -> Option<&'a str> {
    let at = kind.columns.iter().position(|c| c.title == title)?;
    row.cells.get(at)?.as_deref().filter(|c| *c != "-")
}

fn count(text: Option<&str>) -> u64 {
    text.map(|t| t.chars().filter(char::is_ascii_digit).collect::<String>())
        .and_then(|d| d.parse().ok())
        .unwrap_or(0)
}

/// The choices of each kind, from the rows.
async fn fill(def: &KindDef, bus: &Bus, (kind, rows): (&Kind, Rows), form: &mut LabOnceForm) {
    match def.id {
        aws_wire::forced::kind::EXPIRE_VISIBILITY => {
            let mut found: Vec<(u64, LabOnceChoice)> = rows
                .iter()
                .map(|r| {
                    let held = count(cell(kind, r, "In flight"));
                    let ready = count(cell(kind, r, "Messages available"));
                    let detail = if held == 0 {
                        format!("Nothing in flight; {ready} available.")
                    } else {
                        format!("{held} in flight, {ready} available.")
                    };
                    (held, choice(&r.name, &r.label, Some(detail)))
                })
                .collect();
            found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.value.cmp(&b.1.value)));
            form.choices = found.into_iter().map(|(_, c)| c).collect();
        }
        aws_wire::forced::kind::FIRE_SCHEDULE => {
            form.choices = rows
                .iter()
                .map(|r| {
                    let state = cell(kind, r, "State").unwrap_or("-");
                    let target = cell(kind, r, "Target").unwrap_or("-");
                    choice(&r.name, &r.label, Some(format!("{state}; target {target}")))
                })
                .collect();
        }
        aws_wire::forced::kind::SWEEP_TTL => ttl_tables(bus, &rows, form).await,
        _ => {
            form.choices = rows
                .iter()
                .map(|r| {
                    let runtime = cell(kind, r, "Runtime").map(str::to_owned);
                    choice(&r.name, &r.label, runtime)
                })
                .collect();
        }
    }
}

fn choice(value: &str, label: &str, detail: Option<String>) -> LabOnceChoice {
    LabOnceChoice {
        value: value.to_owned(),
        label: label.to_owned(),
        detail,
        state: None,
    }
}

/// The tables that have time to live on, with the attribute that holds the expiry.
async fn ttl_tables(bus: &Bus, rows: &Rows, form: &mut LabOnceForm) {
    let mut unread = 0;
    for r in rows.iter().take(MOST_TABLES) {
        match bus
            .call(
                "dynamodb",
                "DescribeTimeToLive",
                &json!({"TableName": r.name}),
            )
            .await
        {
            Ok(a) => {
                let d = a.get("TimeToLiveDescription").unwrap_or(&Value::Null);
                if d.get("TimeToLiveStatus").and_then(Value::as_str) == Some("ENABLED") {
                    let attribute = d
                        .get("AttributeName")
                        .and_then(Value::as_str)
                        .unwrap_or("?");
                    form.choices.push(choice(
                        &r.name,
                        &r.label,
                        Some(format!("Expiry in the attribute {attribute}.")),
                    ));
                }
            }
            Err(_) => unread += 1,
        }
    }
    let skipped = rows.len().saturating_sub(MOST_TABLES) + unread;
    form.note = Some(if skipped > 0 {
        format!(
            "Tables with time to live on are listed; {skipped} could not be looked at. A table without it has nothing that expires."
        )
    } else {
        "Tables with time to live on are listed: a table without it has nothing that expires."
            .to_owned()
    });
    if form.choices.is_empty() {
        form.empty = Some(format!("No table in {} has time to live on.", form.region));
    }
}

/// What forcing the choice (else the first) would do.
fn preview(def: &KindDef, form: &LabOnceForm, chosen: Option<&str>) -> Option<String> {
    let pick = chosen
        .and_then(|c| form.choices.iter().find(|x| x.value == c))
        .or_else(|| form.choices.first())?;
    let name = &pick.label;
    Some(match def.id {
        aws_wire::forced::kind::EXPIRE_VISIBILITY => format!(
            "Bring back the messages in flight in {name} now ({}) . Each can be received again at once with the receive count it had, and the receipt handles it was given still name it.",
            pick.detail
                .as_deref()
                .unwrap_or("none are counted")
                .trim_end_matches('.')
        ),
        aws_wire::forced::kind::FIRE_SCHEDULE => format!(
            "Invoke {name}'s target now ({}), by the path its timer takes. A recurring schedule keeps its plan, so its next fire still happens when it was due; a one-time schedule's fire is its one fire.",
            pick.detail
                .as_deref()
                .unwrap_or("no detail")
                .trim_end_matches('.')
        ),
        aws_wire::forced::kind::SWEEP_TTL => format!(
            "Run the time-to-live sweep for {name} now: the items whose expiry has passed are deleted and the others stay. The sweep covers every table of {} that has time to live, as its timer's does.",
            form.region
        ),
        _ => format!(
            "End the warm processes of {name}: its next invocation starts a new one, which runs its initialisation again and reports an Init Duration. The function itself is not changed."
        ),
    })
}

impl Done {
    /// A forced event that ran.
    pub(crate) fn ran(subject: &str, sentence: &str) -> Self {
        Self {
            ok: true,
            sentence: sentence.to_owned(),
            subject: subject.to_owned(),
        }
    }

    /// One that did not, with why in words.
    pub(crate) fn refused(subject: &str, why: &str) -> Self {
        Self {
            ok: false,
            sentence: why.to_owned(),
            subject: subject.to_owned(),
        }
    }
}

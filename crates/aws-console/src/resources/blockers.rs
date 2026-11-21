//! Delete with dependents: what stands in the way of deleting a resource, and the calls that clear it.
//!
//! A kind lists its [`Blocker`]s: a call that lists one sort of dependent (an IAM role's attached policies, an event bus's rules,
//! a bucket's objects), and, when the console can clear them, the call that clears each. `GET …/<name>/blockers` shows them
//! ([`read`]); `DELETE …/<name>?clear=yes` clears every one that can be cleared, then deletes ([`clear`]). A plain delete is
//! the service's own and is refused by the service when something blocks. Every call is the console's, in Live.

use std::future::Future;
use std::pin::Pin;

use aws_wire::ApiError;
use serde_json::{Map, Value};

use super::bus::Bus;
use super::run::{self, Fill};
use super::{Descriptor, Kind, Step, Text, path};
use crate::model::{BlockerGroup, BlockerItem, Blockers, ResourceLink};

/// The most dependents of one sort a page names.
pub const MOST_SHOWN: usize = 50;
/// The most batches of one clearing (a bucket of a million objects is not emptied from a page).
const MOST_ROUNDS: usize = 200;

/// How one sort of dependent is cleared.
#[derive(Debug, Clone, Copy)]
pub enum Clear {
    /// One call for each. The item's members fill `{form:<member>}` (`{form:PolicyArn}`); `said` is what it reports, with
    /// `{name}` the item's name.
    Each { step: Step, said: &'static str },
    /// Batches: the `pick`ed members of up to `size` items are set at `at` of the call's input (S3's `DeleteObjects`; with
    /// `flat`, the one picked member's values alone: EventBridge's `RemoveTargets` takes ids), and the dependents are listed
    /// again until none are left. `said` has `{count}`.
    Batch(Batch),
    /// For each, first clear what depends on it (`under`, listed with the item's members as `{form:<member>}`), then make
    /// the call (a rule's targets, then the rule).
    Cascade {
        under: &'static [Blocker],
        step: Step,
        said: &'static str,
    },
}

/// Clearing in batches ([`Clear::Batch`]).
#[derive(Debug, Clone, Copy)]
pub struct Batch {
    pub step: Step,
    pub at: &'static str,
    pub pick: &'static [&'static str],
    pub size: usize,
    pub flat: bool,
    pub said: &'static str,
}

/// One sort of dependent of a resource.
#[derive(Debug, Clone, Copy)]
pub struct Blocker {
    /// `Attached policies`.
    pub title: &'static str,
    /// The call that lists them, filled for the resource.
    pub list: Step,
    /// Where they are in its answer, and the member of one that is a bare value.
    pub items: &'static str,
    pub item_as: &'static str,
    /// What each is called.
    pub name: Text,
    /// Each links to this resource of the console: its service and kind, and the text that names it (a policy's ARN).
    pub link: Option<(&'static str, &'static str, Text)>,
    /// Items whose value at `.0` is `.1` are left out (a policy's default version cannot be deleted).
    pub except: Option<(&'static str, &'static str)>,
    /// The service refuses the delete while they are there. `false`: they are only left behind, and listed to say so.
    pub blocks: bool,
    pub clear: Option<Clear>,
}

/// What a delete leaves behind.
#[derive(Debug, Clone, Copy)]
pub enum Kept {
    /// A sentence, with `{name}` the resource's name.
    Text(&'static str),
    /// Read from a call's answer: the lines it gives (a stack's resources with `DeletionPolicy: Retain`).
    Read(Step, fn(&Value) -> Vec<String>),
}

/// A call's body, filled for the resource and the enclosing item's members.
struct Ctx<'a> {
    bus: &'a Bus,
    d: &'a Descriptor,
    id: &'a str,
    name: &'a str,
    out: &'a Value,
}

impl Ctx<'_> {
    fn fill<'f>(&'f self, form: &'f Map<String, Value>) -> Fill<'f> {
        Fill {
            id: self.id,
            name: self.name,
            ..Fill::bare(self.bus, self.out, form)
        }
    }

    /// The items of `blocker`, as its answer gives them (each an object).
    async fn items(&self, b: &Blocker, form: &Map<String, Value>) -> Result<Vec<Value>, ApiError> {
        match run::call(self.d, &b.list, &self.fill(form)).await {
            Ok(answer) => Ok(run::items_of(&answer, b.items, b.item_as)),
            Err(_) if b.list.optional => Ok(Vec::new()),
            Err(e) => Err(e),
        }
        .map(|items| {
            items
                .into_iter()
                .filter(|i| {
                    b.except
                        .is_none_or(|(at, eq)| path::text(i, at).is_none_or(|v| v != eq))
                })
                .collect()
        })
    }
}

/// An item's own members as a form: what a clearing call's `{form:<member>}` reads.
fn members(item: &Value) -> Map<String, Value> {
    match item {
        Value::Object(m) => m
            .iter()
            .filter(|(_, v)| v.is_string() || v.is_number() || v.is_boolean())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        _ => Map::new(),
    }
}

fn said(template: &str, name: &str, count: usize) -> String {
    template
        .replace("{name}", name)
        .replace("{count}", &count.to_string())
}

/// What stands in the way of deleting the resource named `name`, and what is left.
///
/// # Errors
/// 404 for a kind the console cannot delete; the service's refusal of a listing.
pub async fn read(
    bus: &Bus,
    (d, kind): (&Descriptor, &Kind),
    name: &str,
) -> Result<Blockers, ApiError> {
    if kind.delete.is_none() {
        return Err(ApiError::new(
            404,
            "NOT_FOUND",
            format!(
                "The console cannot delete {}, so nothing stands in the way.",
                kind.title.to_lowercase()
            ),
        ));
    }
    let (id, out) = run::id_of(bus, d, kind, name).await?;
    let ctx = Ctx {
        bus,
        d,
        id: &id,
        name,
        out: &out,
    };
    let none = Map::new();
    let mut blockers = Vec::new();
    for b in kind.blockers {
        let items = ctx.items(b, &none).await?;
        if items.is_empty() {
            continue;
        }
        let more = items.len() > MOST_SHOWN;
        blockers.push(BlockerGroup {
            title: b.title.to_owned(),
            items: items
                .iter()
                .take(MOST_SHOWN)
                .map(|item| BlockerItem {
                    link: b.link.and_then(|(service, kind, at)| {
                        Some(ResourceLink {
                            service: service.to_owned(),
                            kind: kind.to_owned(),
                            name: at.read(item)?,
                        })
                    }),
                    name: b.name.read(item).unwrap_or_default(),
                })
                .collect(),
            clears: b.clear.is_some(),
            blocks: b.blocks,
            more,
        });
    }
    let mut kept = Vec::new();
    for k in kind.kept {
        match k {
            Kept::Text(t) => kept.push(t.replace("{name}", name)),
            Kept::Read(step, lines) => {
                if let Ok(answer) = run::call(d, step, &ctx.fill(&none)).await {
                    kept.extend(lines(&answer));
                }
            }
        }
    }
    Ok(Blockers {
        blockers,
        kept,
        words: if kind.confirm.is_empty() {
            name.to_owned()
        } else {
            kind.confirm.to_owned()
        },
    })
}

/// Clears every dependent of the resource named `name` that its kind can clear; what was done, one line each.
///
/// # Errors
/// The service's refusal of a listing or a clearing call (what was cleared before stays cleared).
pub async fn clear(
    bus: &Bus,
    (d, kind): (&Descriptor, &Kind),
    name: &str,
) -> Result<Vec<String>, ApiError> {
    if kind.blockers.iter().all(|b| b.clear.is_none()) {
        return Ok(Vec::new());
    }
    let (id, out) = run::id_of(bus, d, kind, name).await?;
    let ctx = Ctx {
        bus,
        d,
        id: &id,
        name,
        out: &out,
    };
    let mut done = Vec::new();
    for b in kind.blockers {
        done.extend(clear_one(&ctx, b, &Map::new()).await?);
    }
    Ok(done)
}

/// Clears one sort of dependent, under the item `form` of the one it hangs from.
fn clear_one<'a>(
    ctx: &'a Ctx<'a>,
    b: &'a Blocker,
    form: &'a Map<String, Value>,
) -> Pin<Box<dyn Future<Output = Result<Vec<String>, ApiError>> + Send + 'a>> {
    Box::pin(async move {
        let Some(how) = &b.clear else {
            return Ok(Vec::new());
        };
        let mut done = Vec::new();
        match how {
            Clear::Each { step, said: words } => {
                for item in ctx.items(b, form).await? {
                    let own = members(&item);
                    run::call(ctx.d, step, &ctx.fill(&own)).await?;
                    done.push(said(words, &b.name.read(&item).unwrap_or_default(), 1));
                }
            }
            Clear::Cascade {
                under,
                step,
                said: words,
            } => {
                for item in ctx.items(b, form).await? {
                    let own = members(&item);
                    for child in *under {
                        done.extend(clear_one(ctx, child, &own).await?);
                    }
                    run::call(ctx.d, step, &ctx.fill(&own)).await?;
                    done.push(said(words, &b.name.read(&item).unwrap_or_default(), 1));
                }
            }
            Clear::Batch(batch) => {
                let count = clear_batch(ctx, b, batch, form).await?;
                if count > 0 {
                    done.push(said(batch.said, "", count));
                }
            }
        }
        Ok(done)
    })
}

/// Lists the dependents again and again, clearing up to `batch.size` a call, until none are left; how many went.
async fn clear_batch(
    ctx: &Ctx<'_>,
    b: &Blocker,
    batch: &Batch,
    form: &Map<String, Value>,
) -> Result<usize, ApiError> {
    let mut count = 0;
    let mut first: Option<Value> = None;
    for _ in 0..MOST_ROUNDS {
        let items: Vec<Value> = ctx
            .items(b, form)
            .await?
            .into_iter()
            .take(batch.size)
            .collect();
        // None left, or the service left the same first one in place (it was refused): no more can be done.
        if items.is_empty() || items.first() == first.as_ref() {
            break;
        }
        first = items.first().cloned();
        let picked: Vec<Value> = items
            .iter()
            .filter_map(|item| {
                if batch.flat {
                    return path::get(item, batch.pick.first()?);
                }
                let members = batch
                    .pick
                    .iter()
                    .filter_map(|p| Some(((*p).to_owned(), path::get(item, p)?)));
                Some(Value::Object(members.collect()))
            })
            .collect();
        let Value::Object(mut body) = run::body(&batch.step, &ctx.fill(form))? else {
            return Err(ApiError::internal("a descriptor's body is not an object"));
        };
        path::put_at(&mut body, batch.at, Value::Array(picked));
        let target = batch.step.service.unwrap_or(ctx.d.target);
        ctx.bus
            .call(target, batch.step.op, &Value::Object(body))
            .await?;
        count += items.len();
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{members, said};

    #[test]
    fn an_items_scalar_members_fill_a_clearing_call() {
        let m = members(&json!({"PolicyArn": "arn:x", "N": 3, "Deep": {"a": 1}, "Ok": true}));
        assert_eq!(m.len(), 3);
        assert!(!m.contains_key("Deep"));
        assert!(members(&json!("bare")).is_empty());
    }

    #[test]
    fn a_report_names_the_item_or_counts_them() {
        assert_eq!(
            said("Detached {name}", "AdminAccess", 1),
            "Detached AdminAccess"
        );
        assert_eq!(
            said("Deleted {count} objects", "", 12),
            "Deleted 12 objects"
        );
    }
}

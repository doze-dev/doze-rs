//! Rows a service lists without an id of its own. SNS lists a subscription that is not yet confirmed as `PendingConfirmation`,
//! the same for every one, and no call takes that as an ARN. Such a row is named by what tells it from the others (its topic,
//! protocol and endpoint), and its detail is read from the list of its topic, where the service does say what it is.

use aws_wire::ApiError;
use serde_json::{Value, json};

use super::bus::Bus;
use super::path;
use super::run::{self, Fill};
use super::{Descriptor, Step};

/// What a name starts with when it names a [`Pending`] row.
const MARK: &str = "pending|";

/// How the rows of a kind that have no id are named and read.
#[derive(Debug)]
pub struct Pending {
    /// Where a listed row's id is (`SubscriptionArn`), and what it is for the rows that have none.
    pub id: &'static str,
    pub value: &'static str,
    /// The members of a listed row that tell one from another, in order; the last is what the row is shown as.
    pub by: &'static [&'static str],
    /// The call that lists them with those members' values (`{out:TopicArn}`), and where its rows are. It takes the page's
    /// `{token}`.
    pub step: Step,
    pub items: &'static str,
    pub token_out: &'static str,
    /// Where the detail's fields find the row (`Attributes`, as the confirmed one's call answers), and the member that says it
    /// is not confirmed (`PendingConfirmation`, which the confirmed one's call answers as `false`).
    pub into: &'static str,
    pub flag: &'static str,
}

impl Pending {
    /// The name of `item` when it is one of these rows: the mark, then each member of [`Pending::by`], divided by `|`.
    #[must_use]
    pub fn name(&self, item: &Value) -> Option<String> {
        if path::text(item, self.id)? != self.value {
            return None;
        }
        let parts: Vec<String> = self
            .by
            .iter()
            .map(|m| path::text(item, m).unwrap_or_default())
            .collect();
        Some(format!("{MARK}{}", parts.join("|")))
    }

    /// What a row of this name is shown as: its last member, said to be pending.
    #[must_use]
    pub fn label(&self, name: &str) -> String {
        let last = self
            .parts(name)
            .and_then(|p| p.last().cloned())
            .unwrap_or_default();
        format!("{last} (pending)")
    }

    /// The members a name holds, in the order of [`Pending::by`]. The last may hold the divider itself.
    fn parts(&self, name: &str) -> Option<Vec<String>> {
        let rest = name.strip_prefix(MARK)?;
        let parts: Vec<String> = rest.splitn(self.by.len(), '|').map(str::to_owned).collect();
        (parts.len() == self.by.len()).then_some(parts)
    }

    /// Whether `name` names one of these rows.
    #[must_use]
    pub fn names(&self, name: &str) -> bool {
        self.parts(name).is_some()
    }

    /// What a detail reads for a row of this name, set in `out` under [`Pending::into`].
    ///
    /// # Errors
    /// The service's refusal, or that no such row is listed any more (it was confirmed or removed).
    pub async fn read(
        &self,
        bus: &Bus,
        d: &Descriptor,
        name: &str,
        out: &mut Value,
    ) -> Result<(), ApiError> {
        let parts = self
            .parts(name)
            .ok_or_else(|| ApiError::new(404, "NOT_FOUND", format!("There is no {name}.")))?;
        if let Value::Object(o) = out {
            for (member, value) in self.by.iter().zip(&parts) {
                o.insert((*member).to_owned(), json!(value));
            }
        }
        let form = serde_json::Map::new();
        let mut token: Option<String> = None;
        for _ in 0..run::PAGES_COUNTED {
            let snapshot = out.clone();
            let fill = Fill {
                token: token.as_deref(),
                ..Fill::bare(bus, &snapshot, &form)
            };
            let answer = run::call(d, &self.step, &fill).await?;
            let rows = path::get(&answer, self.items);
            let found = rows.as_ref().and_then(Value::as_array).and_then(|rows| {
                rows.iter()
                    .find(|row| self.name(row).as_deref() == Some(name))
            });
            if let Some(row) = found {
                let mut row = row.clone();
                if let Value::Object(m) = &mut row {
                    m.insert(self.flag.to_owned(), json!("true"));
                }
                if let Value::Object(o) = out {
                    o.insert("$id".to_owned(), json!(self.value));
                    o.insert(self.into.to_owned(), row);
                }
                return Ok(());
            }
            token = path::text(&answer, self.token_out).filter(|t| !t.is_empty());
            if token.is_none() {
                break;
            }
        }
        Err(ApiError::new(
            404,
            "NOT_FOUND",
            "This subscription is no longer pending: it was confirmed or removed.".to_owned(),
        ))
    }
}

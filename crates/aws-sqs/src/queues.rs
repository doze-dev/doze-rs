//! The operations on queues.

use aws_model_sqs::keys::{
    CreateQueueRequest as Create, CreateQueueResult as Created, DeleteQueueRequest as Delete,
    GetQueueAttributesRequest as GetAttrs, GetQueueAttributesResult as Attrs,
    GetQueueUrlRequest as GetUrl, GetQueueUrlResult as Url, ListQueuesRequest as List,
    ListQueuesResult as Listed, PurgeQueueRequest as Purge, SetQueueAttributesRequest as SetAttrs,
};
use aws_wire::ApiError;
use aws_wire::service::{Call, Reply};

use std::collections::BTreeMap;

use crate::attrs;
use crate::core::{
    Core, empty, invalid_address, invalid_parameter, missing, no_such_queue, reply, request,
};
use crate::fifo::FifoState;
use crate::model::Queue;
use crate::tags;
use crate::url;

/// How long a deleted queue's name, and a purge, are held (the minute AWS holds them).
const WAIT_MS: i64 = 60_000;

/// What a standard queue's name may be, as the message AWS gives says.
const NAME_RULE: &str =
    "Can only include alphanumeric characters, hyphens, or underscores. 1 to 80 in length";
/// What a FIFO queue's name may be.
const FIFO_NAME_RULE: &str = "The name of a FIFO queue can only include alphanumeric characters, hyphens, or underscores, must end with .fifo suffix and be 1 to 80 in length.";

impl Core {
    pub(crate) fn create_queue(&mut self, call: &Call) -> Reply {
        let v = request::<Create>(call)?;
        let name = v
            .get(Create::QUEUE_NAME)
            .ok_or_else(|| missing("QueueName"))?;
        let asked = attrs::parse(v.get(Create::ATTRIBUTES).into_iter().flatten())?;
        // The kind of queue decides what its name may be, and the name is checked before the rest.
        let fifo = asked
            .stored
            .get("FifoQueue")
            .is_some_and(|v| v.eq_ignore_ascii_case("true"));
        check_name(name, fifo)?;
        if let Some(existing) = self.queues.get(name) {
            if let Some(attr) = asked.differs_from(existing) {
                return Err(ApiError::modeled(
                    "QueueNameExists",
                    format!(
                        "A queue already exists with the same name and a different value for attribute {attr}"
                    ),
                ));
            }
        } else {
            attrs::check_kind(&asked, &BTreeMap::new(), fifo, true)?;
            let now = self.now_ms();
            if self.deleted.get(name).is_some_and(|at| now - at < WAIT_MS) {
                return Err(ApiError::modeled(
                    "QueueDeletedRecently",
                    "You must wait 60 seconds after deleting a queue before you can create another with the same name.",
                ));
            }
            let tags = tags::check(
                name,
                &BTreeMap::new(),
                v.get(Create::TAGS).into_iter().flatten(),
            )?;
            let (attrs, stored) = asked.into_attrs();
            let queue = Queue {
                name: name.to_owned(),
                attrs,
                stored,
                created_secs: now / 1000,
                modified_secs: now / 1000,
                messages: Vec::new(),
                tags: tags.into_iter().collect(),
                fifo: FifoState::default(),
                purged_ms: None,
            };
            self.queues.insert(name.to_owned(), queue);
        }
        let queue_url = url::build(&call.caller.endpoint, &call.caller.account_id, name);
        reply::<Created>(|o| {
            o.put(Created::QUEUE_URL, &queue_url);
        })
    }

    pub(crate) fn delete_queue(&mut self, call: &Call) -> Reply {
        let v = request::<Delete>(call)?;
        let queue_url = v
            .get(Delete::QUEUE_URL)
            .ok_or_else(|| missing("QueueUrl"))?;
        let name = self.queue_mut(call, queue_url)?.name.clone();
        self.queues.remove(&name);
        let now = self.now_ms();
        self.deleted.insert(name, now);
        empty()
    }

    pub(crate) fn get_queue_url(&mut self, call: &Call) -> Reply {
        let v = request::<GetUrl>(call)?;
        let name = v
            .get(GetUrl::QUEUE_NAME)
            .ok_or_else(|| missing("QueueName"))?;
        let account = v
            .get(GetUrl::QUEUE_OWNER_AWS_ACCOUNT_ID)
            .unwrap_or(&call.caller.account_id);
        if account != call.caller.account_id {
            return Err(invalid_address(call));
        }
        if !self.queues.contains_key(name) {
            return Err(no_such_queue());
        }
        let queue_url = url::build(&call.caller.endpoint, account, name);
        reply::<Url>(|o| {
            o.put(Url::QUEUE_URL, &queue_url);
        })
    }

    pub(crate) fn get_queue_attributes(&mut self, call: &Call) -> Reply {
        let v = request::<GetAttrs>(call)?;
        let queue_url = v
            .get(GetAttrs::QUEUE_URL)
            .ok_or_else(|| missing("QueueUrl"))?;
        let names: Vec<&str> = v
            .get(GetAttrs::ATTRIBUTE_NAMES)
            .into_iter()
            .flatten()
            .collect();
        let now = self.now_ms();
        let (account, region) = (call.caller.account_id.clone(), call.caller.region.clone());
        let queue = self.queue_mut(call, queue_url)?;
        let found = attrs::render(queue, &account, &region, &names, now);
        if let Some(unknown) = names
            .iter()
            .find(|n| **n != "All" && !found.iter().any(|(f, _)| f == **n))
        {
            return Err(ApiError::new(
                400,
                "InvalidAttributeName",
                format!("Unknown Attribute {unknown}."),
            ));
        }
        reply::<Attrs>(|o| {
            if !found.is_empty() {
                o.put_map(Attrs::ATTRIBUTES, |m| {
                    for (name, value) in &found {
                        m.insert(name, value);
                    }
                });
            }
        })
    }

    pub(crate) fn set_queue_attributes(&mut self, call: &Call) -> Reply {
        let v = request::<SetAttrs>(call)?;
        let queue_url = v
            .get(SetAttrs::QUEUE_URL)
            .ok_or_else(|| missing("QueueUrl"))?;
        let asked = attrs::parse(v.get(SetAttrs::ATTRIBUTES).into_iter().flatten())?;
        let now = self.now_ms();
        let queue = self.queue_mut(call, queue_url)?;
        attrs::check_kind(&asked, &queue.stored, queue.is_fifo(), false)?;
        asked.apply_to(&mut queue.attrs);
        queue.stored.extend(asked.stored);
        queue.modified_secs = now / 1000;
        empty()
    }

    pub(crate) fn list_queues(&mut self, call: &Call) -> Reply {
        let v = request::<List>(call)?;
        let prefix = v.get(List::QUEUE_NAME_PREFIX).unwrap_or("");
        let after = v
            .get(List::NEXT_TOKEN)
            .map(|token| decode_token(token, prefix))
            .transpose()?;
        let limit = match v.get(List::MAX_RESULTS) {
            None => usize::MAX,
            Some(n) if (1..=1000).contains(&n) => usize::try_from(n).unwrap_or(1000),
            Some(_) => {
                return Err(invalid_parameter(
                    "Value for parameter MaxResults is invalid. Reason: MaxResults must be an integer between 1 and 1000.",
                ));
            }
        };
        let mut names: Vec<&str> = self
            .queues
            .keys()
            .map(String::as_str)
            .filter(|n| n.starts_with(prefix) && after.as_deref().is_none_or(|a| *n > a))
            .collect();
        let more = names.len() > limit;
        names.truncate(limit);
        let next = more
            .then(|| names.last().map(|last| encode_token(prefix, last)))
            .flatten();
        let urls: Vec<String> = names
            .iter()
            .map(|n| url::build(&call.caller.endpoint, &call.caller.account_id, n))
            .collect();
        reply::<Listed>(|o| {
            if !urls.is_empty() {
                o.put_list(Listed::QUEUE_URLS, |l| {
                    for u in &urls {
                        l.push(u);
                    }
                });
            }
            if let Some(token) = &next {
                o.put(Listed::NEXT_TOKEN, token);
            }
        })
    }

    pub(crate) fn purge_queue(&mut self, call: &Call) -> Reply {
        let v = request::<Purge>(call)?;
        let queue_url = v.get(Purge::QUEUE_URL).ok_or_else(|| missing("QueueUrl"))?;
        let now = self.now_ms();
        let queue = self.queue_mut(call, queue_url)?;
        if queue.purged_ms.is_some_and(|at| now - at < WAIT_MS) {
            return Err(ApiError::modeled(
                "PurgeQueueInProgress",
                format!(
                    "Only one PurgeQueue operation on {} is allowed every 60 seconds.",
                    queue.name
                ),
            ));
        }
        queue.messages.clear();
        queue.purged_ms = Some(now);
        empty()
    }
}

/// A queue's name for its kind. A standard queue's is 1 to 80 letters, digits, hyphens and underscores; a FIFO queue's is the
/// same with `.fifo` on the end, 80 in all.
fn check_name(name: &str, fifo: bool) -> Result<(), ApiError> {
    let stem = if fifo {
        name.strip_suffix(".fifo")
    } else {
        Some(name)
    };
    let ok = name.chars().count() <= 80
        && stem.is_some_and(|s| {
            !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        });
    if ok {
        Ok(())
    } else if fifo {
        Err(invalid_parameter(FIFO_NAME_RULE))
    } else {
        Err(invalid_parameter(NAME_RULE))
    }
}

/// A paging token binds the page to the request that made it: it carries the prefix and the last name, in hex.
fn encode_token(prefix: &str, last: &str) -> String {
    aws_core::hex::encode(format!("{prefix}\0{last}").as_bytes())
}

/// The last name of a token made for this `prefix`; a token for another request, or not a token, is refused.
fn decode_token(token: &str, prefix: &str) -> Result<String, ApiError> {
    let refused = || {
        invalid_parameter(
            "Invalid NextToken value. If you are passing in NextToken, you must not change the other request parameters.",
        )
    };
    let text = aws_core::hex::decode(token)
        .and_then(|b| String::from_utf8(b).ok())
        .ok_or_else(refused)?;
    let (made_for, last) = text.split_once('\0').ok_or_else(refused)?;
    if made_for != prefix {
        return Err(refused());
    }
    Ok(last.to_owned())
}

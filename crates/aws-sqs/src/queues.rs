//! The operations on queues.

use aws_model_sqs::keys::{
    CreateQueueRequest as Create, CreateQueueResult as Created, DeleteQueueRequest as Delete,
    GetQueueAttributesRequest as GetAttrs, GetQueueAttributesResult as Attrs,
    GetQueueUrlRequest as GetUrl, GetQueueUrlResult as Url, ListQueuesRequest as List,
    ListQueuesResult as Listed, PurgeQueueRequest as Purge, SetQueueAttributesRequest as SetAttrs,
};
use aws_wire::ApiError;
use aws_wire::service::{Call, Reply};

use crate::attrs;
use crate::core::{Core, empty, invalid_parameter, missing, no_such_queue, reply, request};
use crate::model::Queue;
use crate::url;

/// What a queue name may be, as the message AWS gives says.
const NAME_RULE: &str =
    "Can only include alphanumeric characters, hyphens, or underscores. 1 to 80 in length";

impl Core {
    pub(crate) fn create_queue(&mut self, call: &Call) -> Reply {
        let v = request::<Create>(call)?;
        let name = v
            .get(Create::QUEUE_NAME)
            .ok_or_else(|| missing("QueueName"))?;
        let asked = attrs::parse(v.get(Create::ATTRIBUTES).into_iter().flatten())?;
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
            if !valid_name(name) {
                return Err(invalid_parameter(NAME_RULE));
            }
            let now = self.now_ms();
            let (attrs, stored) = asked.into_attrs();
            let queue = Queue {
                name: name.to_owned(),
                attrs,
                stored,
                created_secs: now / 1000,
                modified_secs: now / 1000,
                messages: Vec::new(),
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
        if account != call.caller.account_id || !self.queues.contains_key(name) {
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
        asked.apply_to(&mut queue.attrs);
        queue.stored.extend(asked.stored);
        queue.modified_secs = now / 1000;
        empty()
    }

    pub(crate) fn list_queues(&mut self, call: &Call) -> Reply {
        let v = request::<List>(call)?;
        let prefix = v.get(List::QUEUE_NAME_PREFIX).unwrap_or("");
        let after = v.get(List::NEXT_TOKEN);
        let limit = match v.get(List::MAX_RESULTS) {
            None => usize::MAX,
            Some(n) if (1..=1000).contains(&n) => usize::try_from(n).unwrap_or(1000),
            Some(n) => {
                return Err(invalid_parameter(format!(
                    "Value {n} for parameter MaxResults is invalid. Reason: MaxResults must be an integer between 1 and 1000, if provided."
                )));
            }
        };
        let mut names: Vec<&str> = self
            .queues
            .keys()
            .map(String::as_str)
            .filter(|n| n.starts_with(prefix) && after.is_none_or(|a| *n > a))
            .collect();
        let more = names.len() > limit;
        names.truncate(limit);
        let next = more.then(|| names.last().copied()).flatten();
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
            if let Some(token) = next {
                o.put(Listed::NEXT_TOKEN, token);
            }
        })
    }

    pub(crate) fn purge_queue(&mut self, call: &Call) -> Reply {
        let v = request::<Purge>(call)?;
        let queue_url = v.get(Purge::QUEUE_URL).ok_or_else(|| missing("QueueUrl"))?;
        self.queue_mut(call, queue_url)?.messages.clear();
        empty()
    }
}

/// 1 to 80 letters, digits, hyphens and underscores (`.fifo` is allowed on the end, and is a FIFO queue's business).
fn valid_name(name: &str) -> bool {
    let stem = name.strip_suffix(".fifo").unwrap_or(name);
    (1..=80).contains(&stem.chars().count())
        && stem
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

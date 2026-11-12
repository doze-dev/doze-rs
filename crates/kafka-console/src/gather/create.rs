//! The Create-topic form: its three steps from what a person typed and what the broker says, and the call itself.

use kafka_wire::api::Response;
use kafka_wire::messages::ApiKey;

use super::forms;
use kafka_broker::{ConfigHelp, Limits};

use crate::backend::{Backend, requests::topics_write};
use crate::calls;
use crate::model::{
    Checked, CreateRefused, CreateResult, CreateView, Created, Draft, Errors, Step, Template,
};
use crate::refusal;
use crate::teach;
use crate::validate::{self, Taken};

/// The configs the form sets, in the order the boards draw them.
pub const CLEANUP: &str = "cleanup.policy";
pub const RETENTION: &str = "retention.ms";

/// What a starting point sets: `cleanup.policy` and `retention.ms`.
#[must_use]
pub const fn template_values(t: Template) -> (&'static str, &'static str) {
    match t {
        Template::EventLog => ("delete", "604800000"),
        Template::Compacted => ("compact", "604800000"),
        Template::ShortLived => ("delete", "3600000"),
    }
}

impl Draft {
    /// A form that has not been touched: no name, the broker's default partition count, the event log's values.
    #[must_use]
    pub fn fresh(default_partitions: i32) -> Self {
        let (cleanup, retention) = template_values(Template::EventLog);
        Self {
            name: String::new(),
            partitions: default_partitions.to_string(),
            template: Template::EventLog,
            cleanup: cleanup.to_owned(),
            retention: retention.to_owned(),
        }
    }

    fn configs(&self) -> Vec<(&str, &str)> {
        vec![(CLEANUP, &self.cleanup), (RETENTION, &self.retention)]
    }
}

/// What is wrong with a form as it stands, for the steps before `wanted`.
#[must_use]
pub fn errors(draft: &Draft, wanted: Step, taken: Option<&Taken>, most: Option<i32>) -> Errors {
    let mut e = Errors::default();
    if wanted == Step::Name {
        return e;
    }
    e.name = validate::topic_name(&draft.name)
        .err()
        .or_else(|| match taken {
            Some(Taken::Exists) => Some("A topic has this name already.".to_owned()),
            Some(Taken::Collides(other)) => {
                Some(format!("This name collides with the topic {other}."))
            }
            _ => None,
        });
    e.partitions = validate::partitions(&draft.partitions, most).err();
    if wanted == Step::Review {
        e.retention = validate::retention_ms(&draft.retention).err();
    }
    e
}

/// The step a form lands on: the one asked for, or the first earlier one that has an error.
#[must_use]
pub fn landing(wanted: Step, e: &Errors) -> Step {
    if e.name.is_some() || e.partitions.is_some() {
        Step::Name
    } else if e.retention.is_some() {
        Step::Config
    } else {
        wanted
    }
}

/// What the form is built from besides what was typed: the names that exist, the broker's limits and table, the version.
#[derive(Debug, Clone)]
pub struct Inputs {
    pub names: Vec<String>,
    pub limits: Option<Limits>,
    pub help: Vec<ConfigHelp>,
    pub version: i16,
    pub address: String,
}

/// Reads those from the broker.
pub async fn inputs(b: &Backend, address: &str) -> Inputs {
    Inputs {
        names: forms::topic_names(b).await,
        limits: b.limits(),
        help: b.topic_config_help().unwrap_or_default(),
        version: b.version(ApiKey::CreateTopics),
        address: address.to_owned(),
    }
}

/// The form at `step`, with these errors. Pure: everything it says is in the arguments.
#[must_use]
pub fn view_at(
    draft: Draft,
    step: Step,
    errors: Errors,
    checked: Option<Checked>,
    i: &Inputs,
) -> CreateView {
    let taken = (!draft.name.is_empty())
        .then(|| validate::taken(&draft.name, i.names.iter().map(String::as_str)));
    let find = |key: &str| i.help.iter().find(|h| h.name == key);
    let configs = if step == Step::Name {
        Vec::new()
    } else {
        draft.configs()
    };
    CreateView {
        teach_name: teach::name(taken.as_ref()),
        most: i.limits.map(|l| l.max_partitions),
        teach_partitions: teach::partitions(i.limits),
        teach_cleanup: find(CLEANUP).map(teach::cleanup),
        teach_retention: find(RETENTION).map(teach::retention),
        call: calls::create_topics(i.version, &draft.name, &draft.partitions, &configs, false),
        terminal: calls::create_command(&i.address, &draft.name, &draft.partitions, &configs),
        step,
        errors,
        checked,
        address: i.address.clone(),
        draft,
    }
}

/// Where the form lands when the person moves to `wanted`: that step, or the first earlier one with something to fix.
#[must_use]
pub fn landing_view(draft: Draft, wanted: Step, i: &Inputs) -> CreateView {
    let taken = validate::taken(&draft.name, i.names.iter().map(String::as_str));
    let e = errors(
        &draft,
        wanted,
        Some(&taken),
        i.limits.map(|l| l.max_partitions),
    );
    let step = landing(wanted, &e);
    let shown = match step {
        Step::Name => Errors {
            retention: None,
            ..e
        },
        Step::Config => Errors {
            name: None,
            partitions: None,
            ..e
        },
        Step::Review => Errors::default(),
    };
    view_at(draft, step, shown, None, i)
}

/// Sends the create (or, for "Check only", the same call with `validate_only`) and says what came of it.
pub async fn create(b: &Backend, draft: Draft, validate_only: bool, address: &str) -> CreateResult {
    let Ok(partitions) = validate::partitions(&draft.partitions, None) else {
        let e = Errors {
            partitions: validate::partitions(&draft.partitions, None).err(),
            ..Errors::default()
        };
        return CreateResult::Invalid(Box::new(view_at(
            draft,
            Step::Name,
            e,
            None,
            &inputs(b, address).await,
        )));
    };
    let version = b.version(ApiKey::CreateTopics);
    let sent = topics_write::create_topic(&draft.name, partitions, &draft.configs(), validate_only);
    let answer = b.call(sent).await;
    let most = b.limits().map(|l| l.max_partitions);
    let result = match answer {
        Ok(Response::CreateTopics(r)) => r.topics.into_iter().next(),
        _ => None,
    };
    let Some(res) = result else {
        return refused(
            &draft,
            refusal::create(-1, Some("The broker gave no answer."), most),
            version,
            most,
        );
    };
    if res.error_code != 0 {
        let why = res.error_message.as_ref().map(|m| m.to_string());
        let r = refusal::create(res.error_code, why.as_deref(), most);
        return refused(&draft, r, version, most);
    }
    if validate_only {
        return CreateResult::Checked(Checked {
            partitions: if res.num_partitions > 0 {
                res.num_partitions
            } else {
                partitions
            },
        });
    }
    let configs = res.configs.unwrap_or_default();
    let value = |key: &str, fallback: &str| {
        configs
            .iter()
            .find(|c| c.name.as_str() == key)
            .and_then(|c| c.value.as_ref())
            .map_or_else(|| fallback.to_owned(), ToString::to_string)
    };
    let count = if res.num_partitions > 0 {
        res.num_partitions
    } else {
        partitions
    };
    let all: Vec<i32> = (0..count).collect();
    let start = forms::offsets(b, &draft.name, &all, -2, 0).await;
    let end = forms::offsets(b, &draft.name, &all, -1, 0).await;
    CreateResult::Created(Created {
        empty: !all.is_empty()
            && all
                .iter()
                .all(|p| start.get(p) == Some(&0) && end.get(p) == Some(&0)),
        cleanup: value(CLEANUP, &draft.cleanup),
        retention: value(RETENTION, &draft.retention),
        name: draft.name,
        partitions: count,
        address: address.to_owned(),
    })
}

fn refused(
    draft: &Draft,
    r: crate::model::Refusal,
    version: i16,
    most: Option<i32>,
) -> CreateResult {
    let configs = draft.configs();
    CreateResult::Refused(Box::new(CreateRefused {
        request: calls::create_topics(version, &draft.name, &draft.partitions, &configs, false),
        answer: calls::create_topics_answer(version, &draft.name, &r),
        existing: (r.code == 36).then(|| draft.name.clone()),
        draft: draft.clone(),
        most,
        refusal: r,
    }))
}

#[cfg(test)]
mod tests;

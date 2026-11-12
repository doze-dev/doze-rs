//! The Create-topic form over the larder: the form, and a stand-in for CreateTopics.

use kafka_broker::{ConfigHelp, Limits};

use super::{TOPICS, address};
use crate::calls;
use crate::gather::{self, Inputs};
use crate::model::{
    Checked, CreateRefused, CreateResult, CreateView, Created, Draft, Errors, Step, Template,
};
use crate::{refusal, validate};

/// Kafka's words for the two configs the form teaches (the broker's table, shortened as the board shortens them).
const CLEANUP_DOC: &str = "The \"delete\" policy (which is the default) will discard old segments when their retention time or size limit has been reached. The \"compact\" policy retains the latest value for each key.";
const RETENTION_DOC: &str = "The maximum time we will retain a log before we discard old log segments, when using the \"delete\" policy. If set to -1, no time limit is applied.";

/// What the form is built from, in the larder.
#[must_use]
pub fn inputs() -> Inputs {
    Inputs {
        names: TOPICS.iter().map(|t| t.name.to_owned()).collect(),
        limits: Some(Limits {
            max_partitions: 1000,
            default_partitions: 1,
        }),
        help: vec![
            ConfigHelp {
                name: "cleanup.policy",
                default: "delete".to_owned(),
                doc: CLEANUP_DOC,
            },
            ConfigHelp {
                name: "retention.ms",
                default: "604800000".to_owned(),
                doc: RETENTION_DOC,
            },
        ],
        version: 7,
        address: address(),
    }
}

/// The form of the boards: `orders.retry`, 3 partitions, the event log.
#[must_use]
pub fn draft_board() -> Draft {
    let (cleanup, retention) = gather::template_values(Template::EventLog);
    Draft {
        name: "orders.retry".to_owned(),
        partitions: "3".to_owned(),
        template: Template::EventLog,
        cleanup: cleanup.to_owned(),
        retention: retention.to_owned(),
    }
}

/// The form at `wanted`, or the step an error sends it back to.
#[must_use]
pub fn create_landing(draft: Draft, wanted: Step) -> CreateView {
    gather::landing_view(draft, wanted, &inputs())
}

fn refused(draft: &Draft, r: crate::model::Refusal) -> CreateResult {
    let configs = [
        ("cleanup.policy", draft.cleanup.as_str()),
        ("retention.ms", draft.retention.as_str()),
    ];
    CreateResult::Refused(Box::new(CreateRefused {
        request: calls::create_topics(7, &draft.name, &draft.partitions, &configs, false),
        answer: calls::create_topics_answer(7, &draft.name, &r),
        existing: (r.code == 36).then(|| draft.name.clone()),
        draft: draft.clone(),
        most: Some(1000),
        refusal: r,
    }))
}

/// What CreateTopics answers in the larder: the same refusals, with the broker's words, for the same reasons.
#[must_use]
pub fn create_topic(draft: Draft, validate_only: bool) -> CreateResult {
    let Ok(partitions) = validate::partitions(&draft.partitions, None) else {
        let e = Errors {
            partitions: validate::partitions(&draft.partitions, None).err(),
            ..Errors::default()
        };
        return CreateResult::Invalid(Box::new(gather::create_view(
            draft,
            Step::Name,
            e,
            None,
            &inputs(),
        )));
    };
    let name = draft.name.clone();
    if validate::topic_name(&name).is_err() {
        let words = format!(
            "Topic name is invalid: '{name}' contains one or more characters other than ASCII alphanumerics, '.', '_' and '-'"
        );
        return refused(&draft, refusal::create(17, Some(&words), Some(1000)));
    }
    match validate::taken(&name, TOPICS.iter().map(|t| t.name)) {
        validate::Taken::Exists => {
            return refused(
                &draft,
                refusal::create(
                    36,
                    Some(&format!("Topic '{name}' already exists.")),
                    Some(1000),
                ),
            );
        }
        validate::Taken::Collides(other) => {
            let words = format!("Topic '{name}' collides with existing topic: {other}");
            return refused(&draft, refusal::create(17, Some(&words), Some(1000)));
        }
        validate::Taken::Free => {}
    }
    if partitions > 1000 {
        let words = format!(
            "{partitions} partitions is above this broker's limit of 1000 (--max-partitions)"
        );
        return refused(&draft, refusal::create(37, Some(&words), Some(1000)));
    }
    if validate_only {
        return CreateResult::Checked(Checked { partitions });
    }
    CreateResult::Created(Created {
        name,
        partitions,
        cleanup: draft.cleanup,
        retention: draft.retention,
        empty: true,
        address: address(),
    })
}

/// The refusal of the `topic-create-refused` board: `orders.retry` was made by a client while the form was open.
#[must_use]
pub fn refused_board() -> CreateResult {
    refused(
        &draft_board(),
        refusal::create(36, Some("Topic 'orders.retry' already exists."), Some(1000)),
    )
}

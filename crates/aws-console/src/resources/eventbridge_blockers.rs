//! EventBridge: what stands in the way of deleting a rule (its targets) and a bus (its rules, each with its targets), cleared with
//! EventBridge's own calls.

use super::{Batch, Blocker, Clear, Step, Text};

/// A rule's targets, removed ten at a time (what `RemoveTargets` takes).
pub(super) const TARGETS: Blocker = Blocker {
    title: "Targets",
    list: Step::new(
        "ListTargetsByRule",
        r#"{"Rule":"{name:last}","EventBusName":"{name:head}"}"#,
    ),
    items: "Targets",
    item_as: "Id",
    name: Text::at("Id"),
    link: None,
    except: None,
    blocks: true,
    clear: Some(Clear::Batch(Batch {
        step: Step::new(
            "RemoveTargets",
            r#"{"Rule":"{name:last}","EventBusName":"{name:head}"}"#,
        ),
        at: "Ids",
        pick: &["Id"],
        size: 10,
        flat: true,
        said: "Removed {count} targets",
    })),
};

/// The targets of one rule of a bus, under the rule's own members (`{form:Name}`).
pub(super) const TARGETS_OF_RULE: Blocker = Blocker {
    list: Step::new(
        "ListTargetsByRule",
        r#"{"Rule":"{form:Name}","EventBusName":"{id}"}"#,
    ),
    clear: Some(Clear::Batch(Batch {
        step: Step::new(
            "RemoveTargets",
            r#"{"Rule":"{form:Name}","EventBusName":"{id}"}"#,
        ),
        at: "Ids",
        pick: &["Id"],
        size: 10,
        flat: true,
        said: "Removed {count} targets",
    })),
    ..TARGETS
};

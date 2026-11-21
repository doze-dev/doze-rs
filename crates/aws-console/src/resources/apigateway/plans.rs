//! Usage plans: the throttle and quota of the API keys in them, and the stages they cover. A plan's keys and stages are read and
//! changed by the pages through the kind's operations.

use super::super::Format::{Mono, Number, Text as Plain};
use super::super::{
    Create, Delete, Id, Input, InputKind, Kind, List, Step, Sub, Text, col, field, input, ops,
};

pub(super) const PLANS: Kind = Kind {
    id: "usageplans",
    title: "Usage plans",
    one: "usage plan",
    about: "Who may use which stages, and how fast: a plan groups the API keys you hand to customers with the stages they may call.",
    members: &["usagePlanId"],
    list: List {
        step: Step::new("GetUsagePlans", r#"{"limit":500,"position":"{token}"}"#),
        items: "item",
        item_as: "id",
        token_out: "position",
        id: "id",
        name: Text::at("id"),
        label: Some(Text::at("name")),
        split: None,
    },
    elsewhere: "",
    each: &[],
    columns: &[
        col("ID", "id", Mono),
        col("Description", "description", Plain),
        col("Rate", "throttle.rateLimit", Number),
        col("Burst", "throttle.burstLimit", Number),
        col("Quota", "quota.limit", Number),
        col("Period", "quota.period", Plain),
    ],
    id_of: Id::Name,
    detail: &[Step::new("GetUsagePlan", r#"{"usagePlanId":"{id}"}"#)],
    fields: &[
        field("Name", "name", Plain),
        field("ID", "id", Mono),
        field("Description", "description", Plain),
        field("Rate (requests a second)", "throttle.rateLimit", Number),
        field("Burst (requests)", "throttle.burstLimit", Number),
        field("Quota (requests)", "quota.limit", Number),
        field("Quota period", "quota.period", Plain),
    ],
    subs: &[Sub {
        title: "API keys",
        step: Step::new("GetUsagePlanKeys", r#"{"usagePlanId":"{id}","limit":500}"#),
        items: "item",
        item_as: "id",
        columns: &[col("Name", "name", Plain), col("ID", "id", Mono)],
        link: None,
        more: "position",
        add: None,
    }],
    create: Some(Create {
        inputs: &[
            Input {
                required: true,
                placeholder: "partners-basic",
                ..input("name", "Name", "", InputKind::Text)
            },
            input("description", "Description", "", InputKind::Text),
        ],
        steps: &[Step::new(
            "CreateUsagePlan",
            r#"{"name":"{form:name}","description":"{form:description}"}"#,
        )],
        name: Text::at("id"),
        key: "name",
    }),
    delete: Some(Delete {
        verb: "Delete",
        step: Step::new("DeleteUsagePlan", r#"{"usagePlanId":"{id}"}"#),
        words: "Deleting removes the plan. Its API keys stay, but are no longer limited by it.",
    }),
    reveal: None,
    deep: &[],
    // The create with its throttle and quota, the plan's stages (a patch), its keys, and the keys and APIs the pages choose from.
    ops: &[
        ops::free("GetUsagePlans"),
        ops::free("GetUsagePlan"),
        ops::free("CreateUsagePlan"),
        ops::free("UpdateUsagePlan"),
        ops::free("GetUsagePlanKeys"),
        ops::free("CreateUsagePlanKey"),
        ops::free("DeleteUsagePlanKey"),
        ops::free("GetApiKeys"),
        ops::free("CreateApiKey"),
        ops::free("GetRestApis"),
        ops::free("GetStages"),
    ],
    ..Kind::BARE
};

//! Lambda's functions: the list, the page of one, and every call its tabs make.

use super::super::Format::{Bytes, Json, Mono, Number, Seconds, Tail, Text as Plain, Time};
use super::super::{
    Blocker, Deep, Delete, Field, Id, Kept, Kind, LinkTo, List, Shape, Step, Sub, Tags, Text,
    Write, col, deep, field, op, ops, or, says,
};

/// What every call of its pages names: this function.
const FUNCTION: &str = r#"{"FunctionName":"{id}"}"#;
/// What a tag call names: this function, by its ARN.
const ARN: &str = r#"{"Resource":"arn:aws:lambda:{region}:{account}:function:{id}"}"#;

/// A mapping is this function's when `ListEventSourceMappings` on it lists its UUID: the page changes only its own.
static MAPPING_OF_THIS_FUNCTION: deep::Owned = deep::Owned {
    member: "UUID",
    list: "ListEventSourceMappings",
    bind: FUNCTION,
    items: "EventSourceMappings",
    at: "UUID",
};

pub(super) const FUNCTIONS: Kind = Kind {
    id: "functions",
    title: "Event Functions",
    one: "function",
    about: "Functions, their runtimes, and what triggers them.",
    members: &["FunctionName"],
    list: List {
        step: Step::new("ListFunctions", r#"{"MaxItems":50,"Marker":"{token}"}"#),
        items: "Functions",
        item_as: "FunctionName",
        token_out: "NextMarker",
        id: "FunctionName",
        name: Text::at("FunctionName"),
        label: None,
        split: None,
    },
    elsewhere: "",
    each: &[],
    // The first four are the list's; the rest are read by the pages (the layers a function uses, its size).
    columns: &[
        col("Description", "Description", Plain),
        col("Package type", "PackageType", Plain),
        col("Runtime", "Runtime", Plain),
        col("Last modified", "LastModified", Time),
        col("Handler", "Handler", Mono),
        col("Memory (MB)", "MemorySize", Number),
        col("Code size", "CodeSize", Bytes),
        col("Layers", "Layers", Json),
    ],
    id_of: Id::Name,
    detail: &[
        Step::new("GetFunction", r#"{"FunctionName":"{id}"}"#),
        Step::new("GetFunctionUrlConfig", r#"{"FunctionName":"{id}"}"#).optional(),
    ],
    fields: &[
        field("ARN", "Configuration.FunctionArn", Mono),
        field("Description", "Configuration.Description", Plain),
        field("State", "Configuration.State", Plain),
        field("Runtime", "Configuration.Runtime", Mono),
        field("Handler", "Configuration.Handler", Mono),
        field("Package type", "Configuration.PackageType", Plain),
        field("Architecture", "Configuration.Architectures.0", Mono),
        field("Memory (MB)", "Configuration.MemorySize", Number),
        field("Ephemeral storage (MB)", "Configuration.EphemeralStorage.Size", Number),
        field("Timeout", "Configuration.Timeout", Seconds),
        field("Code size", "Configuration.CodeSize", Bytes),
        Field {
            link: &[LinkTo {
                service: "iam",
                kind: "roles",
                when: None,
            }],
            ..field("Execution role", "Configuration.Role", Mono)
        },
        // GetFunctionUrlConfig answers ResourceNotFoundException for a function with no URL.
        or("Function URL", "FunctionUrl", Mono, "None"),
        field("Function URL auth", "AuthType", Plain),
        field("Function URL CORS", "Cors", Json),
        field("Function URL ID", "FunctionUrl", Mono),
        field("Layers", "Configuration.Layers", Json),
        field(
            "Environment variables",
            "Configuration.Environment.Variables",
            Json,
        ),
        field(
            "Dead-letter target",
            "Configuration.DeadLetterConfig.TargetArn",
            Mono,
        ),
        field(
            "Reserved concurrency",
            "Concurrency.ReservedConcurrentExecutions",
            Number,
        ),
        field("Last modified", "Configuration.LastModified", Time),
    ],
    subs: &[Sub {
        title: "Event source mappings",
        step: Step::new(
            "ListEventSourceMappings",
            r#"{"FunctionName":"{id}","MaxItems":100}"#,
        ),
        items: "EventSourceMappings",
        item_as: "UUID",
        columns: &[
            col("Source", "EventSourceArn", Tail),
            col("State", "State", Plain),
            col("Batch size", "BatchSize", Number),
            col("Mapping", "UUID", Mono),
        ],
        link: None,
        more: "NextMarker",
        add: None,
    }],
    create: None,
    delete: Some(Delete {
        verb: "Delete",
        step: Step::new("DeleteFunction", r#"{"FunctionName":"{id}"}"#),
        words: "Deleting removes the function, its versions and aliases. Its event source mappings stop delivering to it.",
    }),
    reveal: None,
    deep: &[
        // The code is read at `…/code` (the zip, as the console's API) and sent with UpdateFunctionCode.
        Deep {
            id: "code",
            title: "Code",
            ops: &[op("UpdateFunctionCode", FUNCTION)],
        },
        // Invoke is synchronous from here, its payload and log tail as text (`deep/invoke.rs`).
        Deep {
            id: "test",
            title: "Test",
            ops: &[op("Invoke", FUNCTION)],
        },
        // The four metrics are read at CloudWatch; the streams of its log group through that group's own page.
        Deep {
            id: "monitor",
            title: "Monitor",
            ops: &[],
        },
        Deep {
            id: "configuration",
            title: "Configuration",
            ops: &[
                op("GetFunctionConfiguration", FUNCTION),
                op("UpdateFunctionConfiguration", FUNCTION),
                // Triggers: a mapping, changed only when it is this function's.
                op("CreateEventSourceMapping", FUNCTION),
                deep::op_owned("UpdateEventSourceMapping", FUNCTION, &MAPPING_OF_THIS_FUNCTION),
                // Permissions: the resource-based policy.
                op("GetPolicy", FUNCTION),
                op("AddPermission", FUNCTION),
                op("RemovePermission", FUNCTION),
                // The function URL and the destinations of its asynchronous invocations.
                op("GetFunctionUrlConfig", FUNCTION),
                op("CreateFunctionUrlConfig", FUNCTION),
                op("UpdateFunctionUrlConfig", FUNCTION),
                op("DeleteFunctionUrlConfig", FUNCTION),
                op("GetFunctionEventInvokeConfig", FUNCTION),
                op("UpdateFunctionEventInvokeConfig", FUNCTION),
                // Update refuses a function with no configuration yet (it has nothing to change): the page puts the first.
                op("PutFunctionEventInvokeConfig", FUNCTION),
            ],
        },
        Deep {
            id: "aliases",
            title: "Aliases",
            ops: &[
                op("ListAliases", FUNCTION),
                op("CreateAlias", FUNCTION),
                op("UpdateAlias", FUNCTION),
                op("DeleteAlias", FUNCTION),
            ],
        },
        Deep {
            id: "versions",
            title: "Versions",
            ops: &[
                op("ListVersionsByFunction", FUNCTION),
                op("PublishVersion", FUNCTION),
            ],
        },
    ],
    ops: &[ops::free("CreateFunction")],
    tags: Some(Tags::new(
        Step::new("ListTags", ARN),
        "Tags",
        Shape::Map,
        Write::Delta {
            add: op("TagResource", ARN),
            at: "Tags",
            remove: op("UntagResource", ARN),
            keys: "TagKeys",
        },
    )),
    // Lambda deletes a function with its versions and aliases, and leaves the mappings that fed it: all are listed so the person
    // knows what goes and what stays, and none is cleared by the console.
    blockers: &[
        Blocker {
            title: "Versions",
            list: Step::new("ListVersionsByFunction", FUNCTION),
            items: "Versions",
            item_as: "Version",
            name: Text::at("Version"),
            link: None,
            except: Some(("Version", "$LATEST")),
            blocks: false,
            clear: None,
        },
        Blocker {
            title: "Aliases",
            list: Step::new("ListAliases", FUNCTION),
            items: "Aliases",
            item_as: "Name",
            name: Text::at("Name"),
            link: None,
            except: None,
            blocks: false,
            clear: None,
        },
        Blocker {
            title: "Event source mappings",
            list: Step::new("ListEventSourceMappings", FUNCTION),
            items: "EventSourceMappings",
            item_as: "UUID",
            name: Text::tail("EventSourceArn"),
            link: None,
            except: None,
            blocks: false,
            clear: None,
        },
    ],
    kept: &[
        Kept::Text("Its log group /aws/lambda/{name} and the logs in it stay."),
        Kept::Text(
            "Event source mappings are not deleted with a function: remove them from the trigger's page.",
        ),
    ],
    confirm: "confirm",
    page: says::lambda::FUNCTION,
    ..Kind::BARE
};

//! Lambda: functions. The console does not create one: a function needs its code, which a deploy tool or the AWS CLI sends.

use super::Format::{Bytes, Json, Mono, Number, Seconds, Tail, Text as Plain, Time};
use super::{
    Blocker, Deep, Delete, Descriptor, Field, Id, Kept, Kind, LinkTo, List, Shape, Step, Sub, Tags,
    Text, Write, col, field, op, or,
};

/// What every call of its pages names: this function.
const FUNCTION: &str = r#"{"FunctionName":"{id}"}"#;
/// What a tag call names: this function, by its ARN.
const ARN: &str = r#"{"Resource":"arn:aws:lambda:{region}:{account}:function:{id}"}"#;

pub(super) static LAMBDA: Descriptor = Descriptor {
    service: "lambda",
    target: "lambda",
    kinds: &[Kind {
        id: "functions",
        title: "Functions",
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
        elsewhere: "A function needs its code, so the console does not create one: deploy it with your own tool, or with aws lambda create-function.",
        each: &[],
        columns: &[
            col("Runtime", "Runtime", Mono),
            col("Handler", "Handler", Mono),
            col("Memory (MB)", "MemorySize", Number),
            col("Code size", "CodeSize", Bytes),
            col("Last modified", "LastModified", Time),
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
            // Invoke is synchronous from here, its payload and log tail as text (`deep/invoke.rs`).
            Deep {
                id: "test",
                title: "Test",
                ops: &[op("Invoke", FUNCTION)],
            },
            Deep {
                id: "configuration",
                title: "Configuration",
                ops: &[
                    op("GetFunctionConfiguration", FUNCTION),
                    op("UpdateFunctionConfiguration", FUNCTION),
                ],
            },
            Deep {
                id: "versions",
                title: "Versions and aliases",
                ops: &[
                    op("ListVersionsByFunction", FUNCTION),
                    op("ListAliases", FUNCTION),
                ],
            },
            // The Live record's Invoke calls, and the streams of its log group, read through that group's own page.
            Deep {
                id: "invocations",
                title: "Invocations",
                ops: &[],
            },
        ],
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
        // Lambda deletes a function and leaves the mappings that fed it: they are listed so the person knows, and are not cleared.
        blockers: &[Blocker {
            title: "Event source mappings",
            list: Step::new("ListEventSourceMappings", FUNCTION),
            items: "EventSourceMappings",
            item_as: "UUID",
            name: Text::tail("EventSourceArn"),
            link: None,
            except: None,
            blocks: false,
            clear: None,
        }],
        kept: &[
            Kept::Text("Its log group /aws/lambda/{name} and the logs in it stay."),
            Kept::Text(
                "Event source mappings are not deleted with a function: remove them from the trigger's page.",
            ),
        ],
        ..Kind::BARE
    }],
};

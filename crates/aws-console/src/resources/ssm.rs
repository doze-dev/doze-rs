//! Systems Manager Parameter Store: parameters. A `SecureString` is hidden until the person reveals it.

use super::Format::{Mono, Number, Text as Plain, Time};
use super::ssm_ops::{OPS, filters};
use super::{
    Apply, Create, Delete, Descriptor, Field, Id, Input, InputKind, Kind, List, Options, Param,
    ParamKind, Reveal, Shape, Step, Tags, Text, Write, col, field, input, op, says,
};

/// What a tag call names: this parameter.
const PARAMETER: &str = r#"{"ResourceType":"Parameter","ResourceId":"{id}"}"#;

pub(super) static SSM: Descriptor = Descriptor {
    service: "ssm",
    target: "ssm",
    kinds: &[Kind {
        id: "parameters",
        title: "Parameters",
        one: "parameter",
        about: "Configuration and secrets as named values. A SecureString stays hidden until you reveal it.",
        members: &["Name"],
        list: List {
            step: Step::new(
                "DescribeParameters",
                r#"{"MaxResults":50,"NextToken":"{token}"}"#,
            ),
            items: "Parameters",
            item_as: "Name",
            token_out: "NextToken",
            id: "Name",
            name: Text::at("Name"),
            label: None,
            split: None,
        },
        elsewhere: "",
        each: &[],
        columns: &[
            col("Type", "Type", Plain),
            col("Tier", "Tier", Plain),
            col("Version", "Version", Number),
            col("Last modified", "LastModifiedDate", Time),
            col("Description", "Description", Plain),
            col("KMS key", "KeyId", Mono),
        ],
        id_of: Id::Name,
        detail: &[
            Step::new("GetParameter", r#"{"Name":"{id}","WithDecryption":false}"#),
            Step::new(
                "DescribeParameters",
                r#"{"ParameterFilters":[{"Key":"Name","Option":"Equals","Values":["{id}"]}]}"#,
            )
            .optional(),
        ],
        fields: &[
            field("ARN", "Parameter.ARN", Mono),
            field("Name", "Parameter.Name", Mono),
            field("Type", "Parameter.Type", Plain),
            Field {
                hide_when: Some(("Parameter.Type", "SecureString")),
                ..field("Value", "Parameter.Value", Mono)
            },
            field("Description", "Parameters.0.Description", Plain),
            field("Data type", "Parameter.DataType", Mono),
            field("Tier", "Parameters.0.Tier", Plain),
            field("Version", "Parameter.Version", Number),
            field("Key", "Parameters.0.KeyId", Mono),
            field("Last modified", "Parameter.LastModifiedDate", Time),
        ],
        subs: &[],
        create: Some(Create {
            inputs: &[
                Input {
                    required: true,
                    placeholder: "/larder/prod/db-url",
                    ..input(
                        "Name",
                        "Name",
                        "A name, or a path of names with slashes (/app/env/key).",
                        InputKind::Text,
                    )
                },
                Input {
                    required: true,
                    value: "String",
                    ..input(
                        "Type",
                        "Type",
                        "SecureString is encrypted with KMS and hidden in the console until revealed.",
                        InputKind::Select(&["String", "StringList", "SecureString"]),
                    )
                },
                Input {
                    required: true,
                    ..input(
                        "Value",
                        "Value",
                        "Up to 4 KB. A StringList is values separated by commas.",
                        InputKind::Textarea,
                    )
                },
                input("Description", "Description", "", InputKind::Text),
            ],
            steps: &[Step::new(
                "PutParameter",
                r#"{"Name":"{form:Name}","Type":"{form:Type}","Value":"{form:Value}","Description":"{form:Description}"}"#,
            )],
            name: Text::at(""),
            key: "Name",
        }),
        delete: Some(Delete {
            verb: "Delete",
            step: Step::new("DeleteParameter", r#"{"Name":"{id}"}"#),
            words: "Deleting removes the parameter and every version of it.",
        }),
        reveal: Some(Reveal {
            label: "Reveal value",
            step: Step::new("GetParameter", r#"{"Name":"{id}","WithDecryption":true}"#),
            path: "Parameter.Value",
        }),
        deep: &[],
        tags: Some(Tags::new(
            Step::new("ListTagsForResource", PARAMETER),
            "TagList",
            Shape::KEY_VALUE,
            Write::Delta {
                add: op("AddTagsToResource", PARAMETER),
                at: "Tags",
                remove: op("RemoveTagsFromResource", PARAMETER),
                keys: "TagKeys",
            },
        )),
        ops: OPS,
        page: says::ssm::PARAMETER,
        // The list's filter tokens (`Name:BeginsWith:/larder|Type:Equals:String`) are DescribeParameters' filters.
        params: &[Param {
            key: "filter",
            label: "Filter",
            input: ParamKind::Text,
            options: Options::None,
            default: "",
            apply: Apply::With(filters),
        }],
        ..Kind::BARE
    }],
};

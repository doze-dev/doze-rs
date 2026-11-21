//! Secrets Manager: secrets. The value is never shown until the person clicks Reveal, which is one `GetSecretValue` (Live shows
//! it).

use super::Format::{Flag, Mono, Text as Plain, Time};
use super::{
    Apply, Create, Deep, Delete, Descriptor, Field, Id, Input, InputKind, Kind, LinkTo, List,
    Options, Param, ParamKind, Reveal, Shape, Step, Tags, Text, Write, col, field, input, op,
};

/// What a tag call names: this secret.
const SECRET: &str = r#"{"SecretId":"{id}"}"#;

pub(super) static SECRETS: Descriptor = Descriptor {
    service: "secretsmanager",
    target: "secretsmanager",
    kinds: &[Kind {
        id: "secrets",
        title: "Secrets",
        one: "secret",
        about: "Secrets and their rotation. A value is shown only when you reveal it.",
        members: &["SecretId"],
        list: List {
            // A secret marked for deletion is listed too: it is still there, and its page is where its deletion is cancelled.
            step: Step::new(
                "ListSecrets",
                r#"{"MaxResults":100,"IncludePlannedDeletion":true,"NextToken":"{token}"}"#,
            ),
            items: "SecretList",
            item_as: "Name",
            token_out: "NextToken",
            id: "ARN",
            name: Text::at("Name"),
            label: None,
            split: None,
        },
        elsewhere: "",
        each: &[],
        columns: &[
            col("Description", "Description", Plain),
            col("Rotation", "RotationEnabled", Flag("On", "Off")),
            col("Last changed", "LastChangedDate", Time),
            col("Last retrieved", "LastAccessedDate", Time),
            col("Marked for deletion", "DeletedDate", Time),
        ],
        id_of: Id::Name,
        detail: &[Step::new("DescribeSecret", r#"{"SecretId":"{id}"}"#)],
        fields: &[
            field("ARN", "ARN", Mono),
            field("Description", "Description", Plain),
            Field {
                hide_when: Some(("", "")),
                empty: "Not readable while the secret is marked for deletion.",
                ..field("Value", "$hidden", Mono)
            },
            Field {
                link: &[LinkTo {
                    service: "kms",
                    kind: "keys",
                    when: None,
                }],
                ..field("Encryption key", "KmsKeyId", Mono)
            },
            field("Rotation", "RotationEnabled", Flag("On", "Off")),
            field("Rotation function", "RotationLambdaARN", Mono),
            field(
                "Rotation schedule",
                "RotationRules.ScheduleExpression",
                Mono,
            ),
            field("Last rotated", "LastRotatedDate", Time),
            field("Last changed", "LastChangedDate", Time),
            field("Last retrieved", "LastAccessedDate", Time),
            // DescribeSecret says when the deletion was asked for. The end of the recovery window is only in DeleteSecret's answer.
            Field {
                retires: true,
                ..field("Marked for deletion", "DeletedDate", Time)
            },
            field("Created", "CreatedDate", Time),
        ],
        subs: &[],
        create: Some(Create {
            inputs: &[
                Input {
                    required: true,
                    placeholder: "larder/prod/stripe-key",
                    ..input(
                        "Name",
                        "Name",
                        "Letters, digits and /_+=.@- . Slashes make a path.",
                        InputKind::Text,
                    )
                },
                Input {
                    required: true,
                    placeholder: r#"{"api_key":"..."}"#,
                    ..input(
                        "SecretString",
                        "Value",
                        "Text or JSON, up to 64 KB. Encrypted with the account's aws/secretsmanager key.",
                        InputKind::Textarea,
                    )
                },
                input("Description", "Description", "", InputKind::Text),
            ],
            steps: &[Step::new(
                "CreateSecret",
                r#"{"Name":"{form:Name}","SecretString":"{form:SecretString}","Description":"{form:Description}"}"#,
            )],
            name: Text::at("Name"),
            key: "Name",
        }),
        delete: Some(Delete {
            verb: "Schedule deletion",
            step: Step::new(
                "DeleteSecret",
                r#"{"SecretId":"{id}","RecoveryWindowInDays":7}"#,
            ),
            words: "The secret is disabled now and deleted after 7 days, AWS's shortest recovery window. Until then its name stays taken and RestoreSecret brings it back.",
        }),
        reveal: Some(Reveal {
            label: "Reveal value",
            step: Step::new("GetSecretValue", r#"{"SecretId":"{id}"}"#),
            path: "SecretString|SecretBinary",
        }),
        // Cancelling a deletion is drawn on the page itself when the secret is marked; the tab is never listed.
        deep: &[Deep {
            id: "restore",
            title: "Cancel deletion",
            ops: &[op("RestoreSecret", r#"{"SecretId":"{id}"}"#)],
        }],
        // A secret's tags come with its description.
        tags: Some(Tags::new(
            Step::new("DescribeSecret", SECRET),
            "Tags",
            Shape::KEY_VALUE,
            Write::Delta {
                add: op("TagResource", SECRET),
                at: "Tags",
                remove: op("UntagResource", SECRET),
                keys: "TagKeys",
            },
        )),
        // ListSecrets' `name` filter matches the beginning of a name.
        params: &[Param {
            key: "name",
            label: "Name",
            input: ParamKind::Text,
            options: Options::None,
            default: "",
            apply: Apply::Template(r#"{"Filters":[{"Key":"name","Values":["{value}"]}]}"#),
        }],
        ..Kind::BARE
    }],
};

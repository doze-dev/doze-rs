//! KMS: keys.

use super::Format::{Flag, Mono, Text as Plain, Time};
use super::{
    Create, Delete, Descriptor, Id, Input, InputKind, Kind, List, Step, Sub, Text, col, field,
    input,
};

const DESCRIBE: Step = Step::new("DescribeKey", r#"{"KeyId":"{id}"}"#);

pub(super) static KMS: Descriptor = Descriptor {
    service: "kms",
    target: "kms",
    rest: false,
    kinds: &[Kind {
        id: "keys",
        title: "Keys",
        one: "key",
        about: "Customer and AWS managed keys. A key scheduled for deletion cannot be used.",
        members: &["KeyId", "TargetKeyId"],
        list: List {
            step: Step::new("ListKeys", r#"{"Limit":100,"Marker":"{token}"}"#),
            items: "Keys",
            item_as: "KeyId",
            token_out: "NextMarker",
            id: "KeyId",
            name: Text::at("KeyId"),
            label: None,
        },
        each: &[DESCRIBE],
        columns: &[
            col("Description", "KeyMetadata.Description", Plain),
            col("State", "KeyMetadata.KeyState", Plain),
            col("Spec", "KeyMetadata.KeySpec", Mono),
            col("Usage", "KeyMetadata.KeyUsage", Mono),
            col("Managed by", "KeyMetadata.KeyManager", Plain),
            col("Created", "KeyMetadata.CreationDate", Time),
        ],
        id_of: Id::Name,
        detail: &[DESCRIBE],
        fields: &[
            field("Key ID", "KeyMetadata.KeyId", Mono),
            field("ARN", "KeyMetadata.Arn", Mono),
            field("Description", "KeyMetadata.Description", Plain),
            field("State", "KeyMetadata.KeyState", Plain),
            field("Enabled", "KeyMetadata.Enabled", Flag("Yes", "No")),
            field("Key spec", "KeyMetadata.KeySpec", Mono),
            field("Key usage", "KeyMetadata.KeyUsage", Mono),
            field("Managed by", "KeyMetadata.KeyManager", Plain),
            field("Origin", "KeyMetadata.Origin", Plain),
            field("Multi-Region", "KeyMetadata.MultiRegion", Flag("Yes", "No")),
            field("Deletion date", "KeyMetadata.DeletionDate", Time),
            field("Created", "KeyMetadata.CreationDate", Time),
        ],
        subs: &[Sub {
            title: "Aliases",
            step: Step::new("ListAliases", r#"{"KeyId":"{id}"}"#),
            items: "Aliases",
            item_as: "AliasName",
            columns: &[
                col("Alias", "AliasName", Mono),
                col("ARN", "AliasArn", Mono),
            ],
            link: None,
            more: "Truncated",
        }],
        create: Some(Create {
            inputs: &[
                input(
                    "Description",
                    "Description",
                    "What the key is for.",
                    InputKind::Text,
                ),
                Input {
                    required: true,
                    value: "SYMMETRIC_DEFAULT",
                    ..input(
                        "KeySpec",
                        "Key spec",
                        "SYMMETRIC_DEFAULT encrypts and decrypts; the others sign, verify or make MACs.",
                        InputKind::Select(&[
                            "SYMMETRIC_DEFAULT",
                            "RSA_2048",
                            "RSA_3072",
                            "RSA_4096",
                            "ECC_NIST_P256",
                            "ECC_NIST_P384",
                            "ECC_SECG_P256K1",
                            "HMAC_256",
                        ]),
                    )
                },
                Input {
                    required: true,
                    value: "ENCRYPT_DECRYPT",
                    ..input(
                        "KeyUsage",
                        "Key usage",
                        "Must suit the spec: HMAC keys make MACs (GENERATE_VERIFY_MAC), ECC keys sign (SIGN_VERIFY).",
                        InputKind::Select(&[
                            "ENCRYPT_DECRYPT",
                            "SIGN_VERIFY",
                            "GENERATE_VERIFY_MAC",
                        ]),
                    )
                },
            ],
            steps: &[Step::new(
                "CreateKey",
                r#"{"Description":"{form:Description}","KeySpec":"{form:KeySpec}","KeyUsage":"{form:KeyUsage}"}"#,
            )],
            name: Text::at("KeyMetadata.KeyId"),
            key: "Description",
        }),
        delete: Some(Delete {
            verb: "Schedule deletion",
            step: Step::new(
                "ScheduleKeyDeletion",
                r#"{"KeyId":"{id}","PendingWindowInDays":7}"#,
            ),
            words: "The key is disabled now and deleted after 7 days, KMS's shortest waiting period. Until then CancelKeyDeletion brings it back; afterwards what it encrypted can never be decrypted.",
        }),
        reveal: None,
    }],
};

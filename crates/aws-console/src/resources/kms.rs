//! KMS: keys (customer managed ones listed; AWS managed ones when asked) and aliases.

use super::Format::{Flag, Mono, Text as Plain, Time};
use super::kms_ops::{ALIAS_OPS, KEY_OPS};
use super::{
    Create, Delete, Descriptor, Field, Id, Input, InputKind, Kind, LinkTo, List, Shape, Split,
    Step, Sub, Tags, Text, Write, col, field, input, op, says, state,
};

/// Who manages a key, as the console says it.
const MANAGER: super::Format = super::Format::Named(&[("CUSTOMER", "Customer"), ("AWS", "AWS")]);

const DESCRIBE: Step = Step::new("DescribeKey", r#"{"KeyId":"{id}"}"#);
/// What a tag call names: this key.
const KEY: &str = r#"{"KeyId":"{id}"}"#;

pub(super) static KMS: Descriptor = Descriptor {
    service: "kms",
    target: "kms",
    kinds: &[KEYS, ALIASES],
};

const KEYS: Kind = Kind {
    id: "keys",
    title: "Keys",
    one: "key",
    about: "Your keys; the keys AWS services made for you are shown when asked. A key scheduled for deletion cannot be used.",
    members: &["KeyId", "TargetKeyId"],
    list: List {
        step: Step::new("ListKeys", r#"{"Limit":100,"Marker":"{token}"}"#),
        items: "Keys",
        item_as: "KeyId",
        token_out: "NextMarker",
        id: "KeyId",
        name: Text::at("KeyId"),
        label: None,
        split: Some(Split {
            path: "KeyMetadata.KeyManager",
            keep: "CUSTOMER",
            others: "AWS managed keys",
        }),
    },
    elsewhere: "",
    each: &[DESCRIBE],
    columns: &[
        col("Description", "KeyMetadata.Description", Plain),
        state("State", "KeyMetadata.KeyState", Plain),
        col("Spec", "KeyMetadata.KeySpec", Mono),
        col("Usage", "KeyMetadata.KeyUsage", Mono),
        col("Managed by", "KeyMetadata.KeyManager", MANAGER),
        col("Created", "KeyMetadata.CreationDate", Time),
        // When a key scheduled for deletion goes: the list says it beside the key.
        col("Deletion date", "KeyMetadata.DeletionDate", Time),
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
        field("Managed by", "KeyMetadata.KeyManager", MANAGER),
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
        add: None,
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
                    InputKind::Select(&["ENCRYPT_DECRYPT", "SIGN_VERIFY", "GENERATE_VERIFY_MAC"]),
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
    deep: &[],
    ops: KEY_OPS,
    tags: Some(Tags::new(
        Step::new("ListResourceTags", KEY),
        "Tags",
        Shape::Pairs("TagKey", "TagValue"),
        Write::Delta {
            add: op("TagResource", KEY),
            at: "Tags",
            remove: op("UntagResource", KEY),
            keys: "TagKeys",
        },
    )),
    page: says::kms::KEY,
    ..Kind::BARE
};

const ALIASES: Kind = Kind {
    id: "aliases",
    title: "Aliases",
    one: "alias",
    about: "Friendly names for keys. A call can name a key by its alias (alias/orders).",
    members: &["AliasName"],
    list: List {
        step: Step::new("ListAliases", r#"{"Limit":100,"Marker":"{token}"}"#),
        items: "Aliases",
        item_as: "AliasName",
        token_out: "NextMarker",
        id: "AliasName",
        name: Text::at("AliasName"),
        label: None,
        split: None,
    },
    elsewhere: "",
    each: &[],
    columns: &[
        col("Key", "TargetKeyId", Mono),
        col("Created", "CreationDate", Time),
    ],
    id_of: Id::Name,
    // KMS has no call that reads one alias; DescribeKey takes an alias and answers its key.
    detail: &[DESCRIBE],
    fields: &[
        field("Alias", "$id", Mono),
        Field {
            link: &[LinkTo {
                service: "kms",
                kind: "keys",
                when: None,
            }],
            ..field("Key", "KeyMetadata.KeyId", Mono)
        },
        field("Key description", "KeyMetadata.Description", Plain),
        field("Managed by", "KeyMetadata.KeyManager", MANAGER),
        field("Key state", "KeyMetadata.KeyState", Plain),
    ],
    subs: &[],
    create: Some(Create {
        inputs: &[
            Input {
                required: true,
                placeholder: "alias/orders",
                ..input(
                    "AliasName",
                    "Alias",
                    "Begins with alias/; alias/aws/ is AWS's own.",
                    InputKind::Text,
                )
            },
            Input {
                required: true,
                placeholder: "1234abcd-12ab-34cd-56ef-1234567890ab",
                ..input(
                    "TargetKeyId",
                    "Key",
                    "The key's ID or ARN.",
                    InputKind::Text,
                )
            },
        ],
        steps: &[Step::new(
            "CreateAlias",
            r#"{"AliasName":"{form:AliasName}","TargetKeyId":"{form:TargetKeyId}"}"#,
        )],
        name: Text::at(""),
        key: "AliasName",
    }),
    delete: Some(Delete {
        verb: "Delete",
        step: Step::new("DeleteAlias", r#"{"AliasName":"{id}"}"#),
        words: "Only the name goes: the key and what it encrypted are untouched. Calls that name the key by this alias stop working.",
    }),
    reveal: None,
    deep: &[],
    ops: ALIAS_OPS,
    ..Kind::BARE
};

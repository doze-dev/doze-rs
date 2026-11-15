//! The "larder" world of the Access boards (`design/boards/access/`): the open broker of `access-open` and the secured one of
//! `access-secured` (SCRAM, eight ACLs, TLS), as view-models, so the pages can be rendered, compared with their boards and
//! served with no broker.

use kafka_auth::authz::{Rules, operation, resource};

use crate::access_words::{Switches, warnings};
use crate::acl_words::after_delete;
use crate::gather::access_dialogs::{
    AclFields, create_call, create_view, delete_call, entry_of, input_of, missing,
};
use crate::model::{
    AccessAcls, AccessAuth, AccessAuthz, AccessConsole, AccessFrom, AccessTls, AccessUser,
    AccessView, AclCreateResult, AclCreateView, AclDeleteResult, AclDeleteView, AclEntry,
    AclPattern, AclPermission,
};

/// The version of CreateAcls and DeleteAcls the boards draw.
const VERSION: i16 = 3;
const LISTEN: &str = "127.0.0.1:9092";

fn acl(
    kind: i8,
    name: &str,
    pattern: AclPattern,
    who: &str,
    op: i8,
    perm: AclPermission,
) -> AclEntry {
    AclEntry {
        kind,
        name: name.to_owned(),
        pattern,
        principal: who.to_owned(),
        host: "*".to_owned(),
        operation: op,
        permission: perm,
    }
}

/// The eight ACLs of `access-secured`, in the order the board lists them.
#[must_use]
pub fn acls() -> Vec<AclEntry> {
    use AclPattern::{Literal, Prefixed};
    use AclPermission::{Allow, Deny};
    vec![
        acl(
            resource::TOPIC,
            "orders",
            Literal,
            "User:orders",
            operation::WRITE,
            Allow,
        ),
        acl(
            resource::TOPIC,
            "orders",
            Literal,
            "User:payments",
            operation::READ,
            Allow,
        ),
        acl(
            resource::TOPIC,
            "payments.",
            Prefixed,
            "User:payments",
            operation::WRITE,
            Allow,
        ),
        acl(
            resource::TOPIC,
            "inventory.levels",
            Literal,
            "User:catalog",
            operation::WRITE,
            Allow,
        ),
        acl(
            resource::GROUP,
            "payments-workers",
            Literal,
            "User:payments",
            operation::READ,
            Allow,
        ),
        acl(
            resource::TRANSACTIONAL_ID,
            "orders-eos",
            Literal,
            "User:orders",
            operation::WRITE,
            Allow,
        ),
        acl(
            resource::CLUSTER,
            "kafka-cluster",
            Literal,
            "User:orders",
            operation::IDEMPOTENT_WRITE,
            Allow,
        ),
        acl(
            resource::TOPIC,
            "customer.profiles",
            Literal,
            "User:report-job",
            operation::READ,
            Deny,
        ),
    ]
}

fn user(name: &str, from: AccessFrom, m: &[&str], sup: bool) -> AccessUser {
    AccessUser {
        name: name.to_owned(),
        from,
        mechanisms: m.iter().map(|s| (*s).to_owned()).collect(),
        iterations: m
            .iter()
            .filter(|s| s.starts_with("SCRAM"))
            .map(|s| ((*s).to_owned(), 4096))
            .collect(),
        super_user: sup,
    }
}

fn console(listen: &str) -> AccessConsole {
    AccessConsole {
        principal: "User:__console".to_owned(),
        listens: listen.to_owned(),
        beyond_loopback: false,
        allow_hosts: vec![],
    }
}

/// The page: the open broker, or with `secured` the one with security on.
#[must_use]
pub fn page(secured: bool) -> AccessView {
    let rows = if secured { acls() } else { vec![] };
    let allow = rows
        .iter()
        .filter(|e| e.permission == AclPermission::Allow)
        .count();
    let sw = Switches {
        sasl: secured,
        tls: secured,
        default_allow: !secured,
        acls_stored: secured,
        listener_beyond_loopback: false,
        console_beyond_loopback: false,
        console_unprotected: false,
        console_addr: "127.0.0.1:9093",
    };
    let scram = ["SCRAM-SHA-256", "SCRAM-SHA-512"];
    AccessView {
        listen: LISTEN.to_owned(),
        auth: AccessAuth {
            on: secured,
            mechanisms: if secured {
                scram.map(str::to_owned).to_vec()
            } else {
                vec![]
            },
            flag_users: usize::from(secured),
            stored_users: if secured { 3 } else { 0 },
            failed_login: secured.then(|| "waits 100 ms".to_owned()),
            plain: false,
        },
        authz: AccessAuthz {
            default_allow: !secured,
            acls: rows.len(),
            allow,
            deny: rows.len() - allow,
            super_users: if secured {
                vec!["User:admin".to_owned()]
            } else {
                vec![]
            },
        },
        tls: AccessTls {
            on: secured,
            cert: secured.then(|| "~/larder/tls/broker.pem".to_owned()),
            listener: LISTEN.to_owned(),
            loopback: true,
        },
        users: if secured {
            vec![
                user("admin", AccessFrom::Flags, &scram, true),
                user("payments", AccessFrom::Stored, &["SCRAM-SHA-256"], false),
                user("orders", AccessFrom::Stored, &["SCRAM-SHA-512"], false),
                user("catalog", AccessFrom::Stored, &["SCRAM-SHA-256"], false),
            ]
        } else {
            vec![]
        },
        acls: AccessAcls {
            rows,
            more: 0,
            refused: None,
        },
        console: console("127.0.0.1:9093"),
        warnings: warnings(&sw),
        quotas: quotas(secured),
    }
}

fn part(kind: &str, name: Option<&str>) -> crate::model::AccessQuotaPart {
    crate::model::AccessQuotaPart {
        kind: kind.to_owned(),
        name: name.map(str::to_owned),
    }
}

fn quota(
    parts: Vec<crate::model::AccessQuotaPart>,
    key: &str,
    value: f64,
) -> crate::model::AccessQuota {
    let named = |kind: &str| {
        parts
            .iter()
            .find(|p| p.kind == kind)
            .map(|p| p.name.as_deref())
    };
    crate::model::AccessQuota {
        applies: crate::quota_words::applies(named("user"), named("client-id"), named("ip")),
        enforced: named("ip").is_none(),
        key: key.to_owned(),
        value: crate::quota_words::value_text(value),
        limit: crate::quota_words::limit(key, value),
        parts,
    }
}

/// The larder's quotas: on the secured broker the payments service is held to 1 MiB/s and has just been muted, and every
/// other client id to 5 MiB/s of fetches; the open broker has none.
fn quotas(secured: bool) -> crate::model::AccessQuotas {
    if !secured {
        return crate::model::AccessQuotas::default();
    }
    let client = |key: &str, user: &str, id: &str, rate: f64, limit: f64, muted: i64| {
        crate::model::AccessQuotaClient {
            key: key.to_owned(),
            user: user.to_owned(),
            client_id: id.to_owned(),
            rate: crate::quota_words::rate(key, rate),
            limit: Some(crate::quota_words::limit(key, limit)),
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a small percentage"
            )]
            share: Some((rate / limit * 100.0).round() as u32),
            muted_ms: muted,
            last_mute_ms: if muted > 0 { 840 } else { 0 },
        }
    };
    crate::model::AccessQuotas {
        rows: vec![
            quota(
                vec![part("client-id", None)],
                "consumer_byte_rate",
                5_242_880.0,
            ),
            quota(
                vec![part("user", Some("payments"))],
                "producer_byte_rate",
                1_048_576.0,
            ),
            quota(
                vec![
                    part("client-id", Some("reports")),
                    part("user", Some("orders")),
                ],
                "request_percentage",
                25.0,
            ),
        ],
        clients: vec![
            client(
                "producer_byte_rate",
                "payments",
                "",
                1_131_520.0,
                1_048_576.0,
                612,
            ),
            client(
                "consumer_byte_rate",
                "",
                "ledger",
                412_000.0,
                5_242_880.0,
                0,
            ),
        ],
        refused: None,
    }
}

/// What a submit of the larder's Set quota or Remove quota comes to: Kafka's own refusal for a value it would refuse, else done.
///
/// # Errors
/// The refusal of a value of zero or less, in Kafka's words.
pub fn quota_alter(p: &crate::gather::quotas::QuotaPost) -> Result<(), crate::model::Refusal> {
    match p.value {
        Some(v) if v <= 0.0 => Err(crate::refusal::refusal(
            42,
            Some(&format!("Quota {} must be greater than 0", p.key)),
        )),
        _ => Ok(()),
    }
}

/// The Create ACL dialog of the board: a Write allowed for `User:orders` on the topic `orders` unless the form says otherwise.
#[must_use]
pub fn create_form(f: &AclFields<'_>) -> AclCreateView {
    create_view(VERSION, input_of(f), None)
}

/// What a submit of the larder's Create ACL dialog comes to: made, unless a field is missing.
#[must_use]
pub fn create(f: &AclFields<'_>) -> AclCreateResult {
    let input = input_of(f);
    let e = entry_of(&input);
    match missing(&e) {
        Some(err) => AclCreateResult::Invalid(Box::new(create_view(VERSION, input, Some(err)))),
        None => AclCreateResult::Created,
    }
}

fn rules() -> Rules {
    Rules {
        default_allow: false,
        super_users: ["User:admin".to_owned(), "User:__console".to_owned()]
            .into_iter()
            .collect(),
    }
}

/// The Delete ACL dialog for an ACL of the larder (gone, if it is not one of the eight).
#[must_use]
pub fn delete_form(target: AclEntry) -> AclDeleteView {
    let all = acls();
    let call = delete_call(VERSION, &target);
    if all.contains(&target) {
        AclDeleteView {
            after: after_delete(&all, &target, &rules()),
            entry: target,
            call,
            gone: false,
            refused: None,
        }
    } else {
        AclDeleteView {
            entry: target,
            after: vec![],
            call,
            gone: true,
            refused: None,
        }
    }
}

/// What a submit of the larder's Delete dialog comes to.
#[must_use]
pub fn delete(target: AclEntry) -> AclDeleteResult {
    let v = delete_form(target);
    if v.gone {
        AclDeleteResult::Gone(Box::new(v))
    } else {
        AclDeleteResult::Deleted
    }
}

/// The call the Create board draws (used by the board test).
#[must_use]
pub fn create_call_of(e: &AclEntry) -> crate::model::Lines {
    create_call(VERSION, e)
}

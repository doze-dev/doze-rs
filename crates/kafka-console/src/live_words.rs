//! What the Live page says about one request that Rust decides: which kind of answer it was (detail, refused, fenced, too
//! large), the rules behind a refusal checked against what the broker says now, and the commands offered to copy.
//!
//! The rules are shown against what the broker says NOW (read when the request is opened): a topic can have changed since the
//! request. What the recorder kept of the request itself (never a record) is what the per-request lines come from. Pure; a name
//! from a request is hostile text, so a command quotes it for a shell.

use crate::format::{bytes, number};
use crate::model::{WireClass, WireDetail};

/// What the broker says about the topic now, read when the request is opened.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "RequestLimits"))]
pub struct Limits {
    pub cleanup: Option<String>,
    /// `max.message.bytes` in force, and whether the topic sets it (else the broker's value applies).
    pub max_message_bytes: Option<(u64, bool)>,
}

/// What became of a request, and so what its sheet shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "RequestKind"))]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Detail,
    Refused,
    Fenced,
    TooLarge,
}

impl Kind {
    /// The kind of a request, by what became of it.
    #[must_use]
    pub fn of(d: &WireDetail) -> Self {
        match (d.row.class, d.row.code) {
            (WireClass::Done | WireClass::Told, _) => Self::Detail,
            (_, 47 | 90) => Self::Fenced,
            (_, 10) => Self::TooLarge,
            _ => Self::Refused,
        }
    }
}

/// One line of the rules list: passed or not, what was checked, the detail, and the tag at the right.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "RequestRule"))]
pub struct Rule {
    pub ok: bool,
    pub title: String,
    pub sub: String,
    pub tag: String,
}

fn topic_of(d: &WireDetail) -> Option<&str> {
    d.topics.first().map(|(t, _)| t.as_str())
}

fn denied_rules(d: &WireDetail) -> Vec<Rule> {
    vec![
        Rule {
            ok: true,
            title: format!("The login is {}", d.principal),
            sub: "the connection's principal, from its login".into(),
            tag: "who".into(),
        },
        Rule {
            ok: false,
            title: "An ACL allows this".into(),
            sub: d.plain.clone(),
            tag: "denied".into(),
        },
    ]
}

fn invalid_record_rules(d: &WireDetail, limits: &Limits) -> Vec<Rule> {
    let mut out = Vec::new();
    if let (Some(t), Some(c)) = (topic_of(d), &limits.cleanup) {
        out.push(Rule {
            ok: true,
            title: "The topic's cleanup policy, as of now".into(),
            sub: format!("{t} cleanup.policy={c}"),
            tag: "config".into(),
        });
    }
    let said = d
        .record_note
        .as_ref()
        .or(d.note.as_ref())
        .cloned()
        .unwrap_or_else(|| d.words.clone());
    out.push(Rule {
        ok: false,
        title: "Every record passes the topic's rules".into(),
        sub: said,
        tag: "refused".into(),
    });
    out
}

/// The rules of a refusal: the ACL's two lines, a record's, or the plain rule and Kafka's words.
#[must_use]
pub fn refused_rules(d: &WireDetail, limits: &Limits) -> Vec<Rule> {
    if d.row.class == WireClass::Denied {
        denied_rules(d)
    } else if d.row.code == 87 {
        invalid_record_rules(d, limits)
    } else {
        vec![Rule {
            ok: false,
            title: d.plain.clone(),
            sub: d.words.clone(),
            tag: "refused".into(),
        }]
    }
}

/// The two rules of a fenced producer: the broker knows it, and the epoch it sent is not current.
#[must_use]
pub fn fenced_rules(d: &WireDetail) -> Vec<Rule> {
    let sent = d.producer;
    let current = d
        .story
        .iter()
        .rev()
        .find(|s| s.api == "InitProducerId" && s.class == WireClass::Done)
        .map(|s| s.id);
    vec![
        Rule {
            ok: true,
            title: "The broker knows this producer".into(),
            sub: current.map_or_else(
                || "from the transactional id's last InitProducerId".to_owned(),
                |_| "its newest InitProducerId is above".to_owned(),
            ),
            tag: sent.map_or_else(String::new, |(p, _)| format!("producer {p}")),
        },
        Rule {
            ok: false,
            title: "The request's epoch is current".into(),
            sub: sent.map_or_else(String::new, |(_, e)| {
                format!("it sent {e}, an epoch the broker has since replaced")
            }),
            tag: sent.map_or_else(String::new, |(_, e)| format!("epoch {e}")),
        },
    ]
}

/// The rules of a batch over the size limit, against the limit as the broker says it now.
#[must_use]
pub fn too_large_rules(d: &WireDetail, limits: &Limits) -> Vec<Rule> {
    let sent = u64::from(d.request_bytes);
    let topic = topic_of(d).unwrap_or("the topic");
    match limits.max_message_bytes {
        Some((limit, topic_sets)) => vec![
            Rule {
                ok: false,
                title: "Batch size is within the limit".into(),
                sub: format!(
                    "{} B sent, {} B over (the whole request, as of the limit now)",
                    number(i64::try_from(sent).unwrap_or(0)),
                    number(i64::try_from(sent.saturating_sub(limit)).unwrap_or(0))
                ),
                tag: format!("{} B", number(i64::try_from(limit).unwrap_or(0))),
            },
            Rule {
                ok: true,
                title: "Where the limit comes from".into(),
                sub: if topic_sets {
                    format!("{topic} sets its own max.message.bytes")
                } else {
                    format!("{topic} sets none: the broker value applies")
                },
                tag: if topic_sets { "topic" } else { "broker" }.into(),
            },
        ],
        None => vec![Rule {
            ok: false,
            title: "Batch size is within the limit".into(),
            sub: format!("{} sent; the limit could not be read now", bytes(sent)),
            tag: "limit".into(),
        }],
    }
}

/// A word for a shell: plain when it is made of safe characters, otherwise in single quotes.
#[must_use]
pub fn shell_word(s: &str) -> String {
    let safe = !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-:/=@%+,".contains(c));
    if safe {
        s.to_owned()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// The two ways to read the one record a Fetch asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchCommands {
    pub kcat: String,
    pub console: String,
}

/// `kcat` and the Kafka console consumer, reading one record at `offset` of a partition.
#[must_use]
pub fn fetch_commands(addr: &str, topic: &str, partition: i32, offset: i64) -> FetchCommands {
    let (addr, topic) = (shell_word(addr), shell_word(topic));
    FetchCommands {
        kcat: format!("kcat -C -b {addr} -t {topic} \\\n  -p {partition} -o {offset} -c 1"),
        console: format!(
            "kafka-console-consumer.sh --bootstrap-server {addr} \\\n  --topic {topic} --partition {partition} --offset {offset} --max-messages 1"
        ),
    }
}

/// `kcat` producing one keyed record to a topic as the client that was refused.
#[must_use]
pub fn keyed_produce(addr: &str, topic: &str, client: &str) -> String {
    format!(
        "echo \"key:value\" | kcat -P -K: \\\n  -b {} -t {} \\\n  -X client.id={}",
        shell_word(addr),
        shell_word(topic),
        shell_word(client)
    )
}

/// `kcat` sending one record to `orders` as `client`: the first-request page's next step.
#[must_use]
pub fn first_produce(addr: &str, client: &str) -> String {
    format!(
        "echo hi | kcat -P -b {} -t orders -X client.id={}",
        shell_word(addr),
        shell_word(client)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_name_is_left_alone_and_a_hostile_one_is_quoted() {
        assert_eq!(shell_word("payments.settled"), "payments.settled");
        assert_eq!(shell_word("a b"), "'a b'");
        assert_eq!(shell_word("x'; rm -rf /; '"), "'x'\\''; rm -rf /; '\\'''");
        assert_eq!(shell_word(""), "''");
        assert_eq!(shell_word("$(id)"), "'$(id)'");
    }

    /// Ids a client may send that a shell would act on if they were pasted bare.
    const HOSTILE: [&str; 9] = [
        "x; open -a Calculator #",
        "$(touch /tmp/doze-pwned)",
        "`touch /tmp/doze-pwned`",
        "a'b",
        "a\"b",
        "line\nnext",
        "two words",
        "x | sh",
        "''",
    ];

    #[test]
    fn the_first_request_command_quotes_the_client_id() {
        assert_eq!(
            first_produce("127.0.0.1:9092", "admin-cli"),
            "echo hi | kcat -P -b 127.0.0.1:9092 -t orders -X client.id=admin-cli"
        );
        assert_eq!(
            first_produce("127.0.0.1:9092", "x; open -a Calculator #"),
            "echo hi | kcat -P -b 127.0.0.1:9092 -t orders -X client.id='x; open -a Calculator #'"
        );
        for id in HOSTILE {
            let c = first_produce("127.0.0.1:9092", id);
            assert!(c.ends_with(&format!("client.id={}", shell_word(id))), "{c}");
        }
    }

    /// A real shell reads the words of the command after `kcat` back to exactly what they were: the id is one word, and
    /// nothing in it runs.
    #[cfg(unix)]
    #[test]
    fn a_shell_reads_every_hostile_id_back_as_one_word_and_runs_none_of_it() {
        for id in HOSTILE {
            let c = first_produce("127.0.0.1:9092", id);
            let (_, args) = c.split_once("kcat ").unwrap();
            let out = std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(format!("set -- {args}; printf '%s\\0' \"$@\""))
                .output()
                .unwrap();
            assert!(out.status.success(), "{id:?}: {c}");
            let words: Vec<&[u8]> = out
                .stdout
                .split(|b| *b == 0)
                .filter(|w| !w.is_empty())
                .collect();
            let want = format!("client.id={id}");
            assert_eq!(
                words,
                [
                    &b"-P"[..],
                    b"-b",
                    b"127.0.0.1:9092",
                    b"-t",
                    b"orders",
                    b"-X",
                    want.as_bytes()
                ],
                "{id:?}: {c}"
            );
        }
        assert!(!std::path::Path::new("/tmp/doze-pwned").exists());
    }

    #[test]
    fn the_fetch_commands_are_the_boards() {
        let c = fetch_commands("127.0.0.1:9092", "payments.settled", 1, 7402);
        assert_eq!(
            c.kcat,
            "kcat -C -b 127.0.0.1:9092 -t payments.settled \\\n  -p 1 -o 7402 -c 1"
        );
        assert!(
            c.console
                .contains("--partition 1 --offset 7402 --max-messages 1")
        );
    }
}

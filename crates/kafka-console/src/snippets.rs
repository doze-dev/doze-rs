//! The client snippets of the Connect page, with the real address in them. The clients, their order and their code are
//! the board's (`connect`); only the address is data.

use crate::model::{ClientSnippet, Tok};

type Line = Vec<(Tok, String)>;

fn plain(s: &str) -> Line {
    vec![(Tok::Plain, s.to_owned())]
}

fn mixed(parts: &[(Tok, &str)]) -> Line {
    parts.iter().map(|(t, s)| (*t, (*s).to_owned())).collect()
}

/// The five snippets for a bootstrap address.
#[must_use]
pub fn for_address(addr: &str) -> Vec<ClientSnippet> {
    use Tok::{Comment, Keyword, Plain, Str};
    let quoted = format!("\"{addr}\"");
    let single = format!("'{addr}'");
    vec![
        ClientSnippet {
            name: "franz-go",
            tag: "go",
            code: vec![
                mixed(&[(Keyword, "cl"), (Plain, ", err := kgo.NewClient(")]),
                mixed(&[
                    (Plain, "    kgo.SeedBrokers("),
                    (Str, &quoted),
                    (Plain, "),"),
                ]),
                mixed(&[
                    (Plain, "    kgo.ClientID("),
                    (Str, "\"payments\""),
                    (Plain, "),"),
                ]),
                plain(")"),
            ],
        },
        ClientSnippet {
            name: "sarama",
            tag: "go",
            code: vec![
                plain("cfg := sarama.NewConfig()"),
                mixed(&[(Plain, "cfg.ClientID = "), (Str, "\"orders\"")]),
                plain("client, err := sarama.NewClient("),
                mixed(&[(Plain, "    []string{"), (Str, &quoted), (Plain, "}, cfg)")]),
            ],
        },
        ClientSnippet {
            name: "librdkafka and kcat",
            tag: "c · py",
            code: vec![
                mixed(&[(Comment, "# kcat")]),
                plain(&first_command(addr)),
                Vec::new(),
                mixed(&[(Comment, "# librdkafka properties")]),
                plain(&format!("bootstrap.servers={addr}")),
                plain("client.id=fulfilment"),
            ],
        },
        ClientSnippet {
            name: "kafkajs",
            tag: "ts",
            code: vec![
                plain("const kafka = new Kafka({"),
                mixed(&[
                    (Plain, "  clientId: "),
                    (Str, "'notifications'"),
                    (Plain, ","),
                ]),
                mixed(&[(Plain, "  brokers: ["), (Str, &single), (Plain, "],")]),
                plain("})"),
            ],
        },
        ClientSnippet {
            name: "Java client",
            tag: "java",
            code: vec![
                plain(&format!("bootstrap.servers={addr}")),
                plain("client.id=my-app"),
                plain("acks=all"),
                plain("enable.idempotence=true"),
            ],
        },
    ]
}

/// The command the Home card suggests: ask the broker what it has, naming the client.
#[must_use]
pub fn first_command(addr: &str) -> String {
    format!(
        "kcat -b {} -X client.id=admin-cli -L",
        crate::live_words::shell_word(addr)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The code of a snippet as one text, as the Copy button copies it.
    fn text(s: &ClientSnippet) -> String {
        s.code
            .iter()
            .map(|line| line.iter().map(|(_, t)| t.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_address_is_in_every_snippet_and_the_clients_are_the_boards() {
        let all = for_address("127.0.0.1:9092");
        let names: Vec<_> = all.iter().map(|s| s.name).collect();
        assert_eq!(
            names,
            [
                "franz-go",
                "sarama",
                "librdkafka and kcat",
                "kafkajs",
                "Java client"
            ]
        );
        for s in &all {
            assert!(text(s).contains("127.0.0.1:9092"), "{}", s.name);
        }
    }

    #[test]
    fn the_code_is_the_boards_to_the_character() {
        let all = for_address("127.0.0.1:9092");
        assert_eq!(
            text(&all[0]),
            "cl, err := kgo.NewClient(\n    kgo.SeedBrokers(\"127.0.0.1:9092\"),\n    kgo.ClientID(\"payments\"),\n)"
        );
        assert_eq!(
            text(&all[2]),
            "# kcat\nkcat -b 127.0.0.1:9092 -X client.id=admin-cli -L\n\n# librdkafka properties\nbootstrap.servers=127.0.0.1:9092\nclient.id=fulfilment"
        );
        assert_eq!(
            text(&all[3]),
            "const kafka = new Kafka({\n  clientId: 'notifications',\n  brokers: ['127.0.0.1:9092'],\n})"
        );
    }

    #[test]
    fn another_address_is_substituted_everywhere() {
        for s in for_address("[::1]:19092") {
            assert!(!text(&s).contains("127.0.0.1"), "{}", s.name);
            assert!(text(&s).contains("[::1]:19092"), "{}", s.name);
        }
    }
}

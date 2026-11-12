//! The commands a drawer offers to copy. Pure; a name from a request is hostile text, so it is quoted for a shell.

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
pub(super) fn keyed_produce(addr: &str, topic: &str, client: &str) -> String {
    format!(
        "echo \"key:value\" | kcat -P -K: \\\n  -b {} -t {} \\\n  -X client.id={}",
        shell_word(addr),
        shell_word(topic),
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

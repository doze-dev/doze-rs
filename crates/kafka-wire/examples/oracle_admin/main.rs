//! The admin APIs of debt K3 sent to real Apache Kafka and to doze-kafka, their answers compared field by field:
//! DescribeTopicPartitions, ElectLeaders, AlterPartitionReassignments, ListPartitionReassignments, AlterReplicaLogDirs,
//! ListConfigResources, UpdateFeatures and the four delegation-token APIs, and who may use each.
//!
//! ```sh
//! cargo run -p kafka-wire --example oracle_admin -- <kafka sasl host:port> <doze sasl host:port> <kafka plain> <doze plain>
//! ```
//!
//! Both brokers fresh, each with a SASL listener that offers PLAIN to `admin` (a super user), `alice` and `bob` (passwords
//! `<name>-pw`), ACLs on with everything allowed that no ACL covers, and the delegation-token secret key set; and a
//! PLAINTEXT listener (doze: another broker) for the refusals a connection without SASL gets. The setup used:
//! `docs/port/behaviours/admin.md` ("The oracle of the admin APIs"). What must differ is normalised before the comparison:
//! topic ids, token ids, HMACs and times (kept as their differences), the request text of a CLUSTER_AUTHORIZATION_FAILED
//! (kept as the operation), the log directory's path, Kafka's internal topics, and the order of entries Kafka keeps in hash
//! maps. Exits 1 if any step differs that is not written down.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::too_many_arguments,
    reason = "a command-line oracle: a step that cannot be made is a loud failure"
)]

mod conn;
mod steps;
mod tokens;

use conn::{Conn, Pair, Tally};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [kafka, doze, kafka_plain, doze_plain] = args.as_slice() else {
        eprintln!(
            "usage: oracle_admin <kafka sasl> <doze sasl> <kafka plaintext> <doze plaintext>"
        );
        std::process::exit(2);
    };
    let as_user = |user: &str| Pair {
        kafka: Conn::login(kafka, user, &format!("{user}-pw")),
        doze: Conn::login(doze, user, &format!("{user}-pw")),
    };
    let mut t = Tally::default();
    let mut admin = as_user("admin");
    steps::fixture(&mut admin);
    steps::describe_topic_partitions(&mut admin, &mut t);
    steps::elect_leaders(&mut admin, &mut t);
    steps::reassignments(&mut admin, &mut t);
    steps::log_dirs(&mut admin, &mut t);
    steps::list_config_resources(&mut admin, &mut t);
    steps::features(&mut admin, &mut t);
    tokens::tokens(&as_user, &mut t);
    let mut plain = Pair {
        kafka: Conn::open(kafka_plain),
        doze: Conn::open(doze_plain),
    };
    tokens::not_over_plaintext(&mut plain, &mut t);
    steps::refused(&mut admin, &mut as_user("bob"), &mut t);
    println!(
        "{} steps, {} differences, {} written down",
        t.steps, t.differences, t.known
    );
    std::process::exit(i32::from(t.differences > 0));
}

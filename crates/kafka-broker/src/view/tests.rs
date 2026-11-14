use std::path::Path;

use kafka_auth::Principal;
use kafka_storage::sim::SimFs;

use super::*;
use crate::Config;

fn fs() -> SimFs {
    let fs = SimFs::new();
    fs.create_dir(Path::new("/")).ok();
    fs
}

fn console() -> Caller {
    Caller::new(Principal::console(), Some("127.0.0.1".parse().unwrap()))
}

#[tokio::test]
async fn a_new_data_directory_has_no_last_stop_and_a_clean_stop_is_remembered() {
    let fs = fs();
    let first = Broker::open(fs.clone(), Config::new("/data".into())).unwrap();
    let seen = first.console_view().instance(&console()).unwrap();
    assert_eq!(seen.last_stop, LastStop::None);
    assert_eq!(seen.open_partitions, 0);
    assert_eq!(seen.data_dir, Path::new("/data"));
    assert!(seen.acl_default_allow);
    first.shutdown().await.unwrap();
    drop(first);
    let again = Broker::open(fs.clone(), Config::new("/data".into())).unwrap();
    let seen = again.console_view().instance(&console()).unwrap();
    assert_eq!(seen.last_stop, LastStop::Clean);
}

#[tokio::test]
async fn a_run_after_a_stop_that_vouched_for_nothing_says_unclean() {
    let fs = fs();
    let first = Broker::open(fs.clone(), Config::new("/data".into())).unwrap();
    // No clean stop is recorded: the process "died".
    drop(first);
    let again = Broker::open(fs.clone(), Config::new("/data".into())).unwrap();
    let seen = again.console_view().instance(&console()).unwrap();
    assert_eq!(seen.last_stop, LastStop::Unclean);
}

#[tokio::test]
async fn the_view_obeys_describe_on_the_cluster() {
    let mut config = Config::new("/data".into());
    config.acl_default_allow = false;
    let broker = Broker::open(fs(), config).unwrap();
    let nobody = Caller::anonymous(Some("127.0.0.1".parse().unwrap()));
    assert_eq!(broker.console_view().instance(&nobody), None);
    // The console is a super user of every broker, whatever the default says (SEC-241).
    assert!(broker.console_view().instance(&console()).is_some());
}

#[tokio::test]
async fn the_create_form_is_taught_from_the_brokers_own_table_and_only_to_who_may_create() {
    let broker = Broker::open(fs(), Config::new("/data".into())).unwrap();
    let table = broker
        .console_view()
        .topic_config_defaults(&console())
        .unwrap();
    let cleanup = table.iter().find(|c| c.name == "cleanup.policy").unwrap();
    assert_eq!(cleanup.default, "delete");
    assert!(cleanup.doc.contains("retention policy"), "{}", cleanup.doc);
    let retention = table.iter().find(|c| c.name == "retention.ms").unwrap();
    // A bare `Config` keeps records for ever; the flags of `doze-kafka` say 7 days.
    assert_eq!(retention.default, "-1");
    let mut config = Config::new("/data".into());
    config.acl_default_allow = false;
    let closed = Broker::open(fs(), config).unwrap();
    let nobody = Caller::anonymous(Some("127.0.0.1".parse().unwrap()));
    assert_eq!(closed.console_view().topic_config_defaults(&nobody), None);
}

#[tokio::test]
async fn the_partition_limits_are_the_flags_and_only_for_who_may_create() {
    let broker = Broker::open(fs(), Config::new("/data".into())).unwrap();
    let limits = broker.console_view().limits(&console()).unwrap();
    assert_eq!(
        (limits.max_partitions, limits.default_partitions),
        (1000, 1)
    );
    let mut config = Config::new("/data".into());
    config.acl_default_allow = false;
    let closed = Broker::open(fs(), config).unwrap();
    let nobody = Caller::anonymous(Some("127.0.0.1".parse().unwrap()));
    assert_eq!(closed.console_view().limits(&nobody), None);
}

#[tokio::test]
async fn access_names_the_flag_users_and_the_super_users_and_never_what_they_log_in_with() {
    use kafka_auth::{Credentials, Mechanism, Users};
    let creds = Credentials::new([("zed".to_owned(), "PLANTED-pw-0123".to_owned())]).unwrap();
    let mut config = Config::new("/data".into());
    config.users = std::sync::Arc::new(
        Users::derive(creds, &[Mechanism::Plain, Mechanism::ScramSha256]).unwrap(),
    );
    config.acl_default_allow = false;
    config.super_users.insert("User:admin".to_owned());
    let broker = Broker::open(fs(), config).unwrap();
    let seen = broker.console_view().access(&console()).unwrap();
    assert_eq!(seen.flag_users, ["zed"]);
    assert_eq!(seen.super_users, ["User:admin"]);
    assert!(!seen.acl_default_allow);
    assert_eq!(seen.stored_users, 0);
    let said = format!("{seen:?}");
    assert!(
        !said.contains("PLANTED"),
        "a password is nowhere in the facts: {said}"
    );
    let nobody = Caller::anonymous(Some("127.0.0.1".parse().unwrap()));
    assert_eq!(broker.console_view().access(&nobody), None);
    assert_eq!(broker.console_view().intervals(&nobody), None);
    assert_eq!(broker.console_view().data_size(&nobody), None);
}

#[tokio::test]
async fn the_intervals_are_the_flags_of_this_run() {
    let mut config = Config::new("/data".into());
    config.log_retention_check_every = std::time::Duration::from_secs(7);
    config.fsync_interval = std::time::Duration::from_millis(90);
    config.log_cleaner_enable = false;
    let broker = Broker::open(fs(), config).unwrap();
    let seen = broker.console_view().intervals(&console()).unwrap();
    assert_eq!(seen.retention_check.as_secs(), 7);
    assert_eq!(seen.fsync.as_millis(), 90);
    assert!(!seen.cleaner_on);
    assert_eq!(seen.transactions.as_millis(), 500);
    assert_eq!(seen.node_id, 1);
}

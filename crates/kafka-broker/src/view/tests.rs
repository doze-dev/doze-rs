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

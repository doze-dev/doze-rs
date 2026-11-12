//! The binary with ACL flags: refused when they are wrong, and enforced when they are right.

use super::*;
use kafka_protocol::messages::create_acls_request::AclCreation;
use kafka_protocol::messages::{
    CreateAclsRequest, CreateAclsResponse, MetadataRequest, MetadataResponse,
};

#[test]
fn acl_flags_that_cannot_work_stop_the_start_in_the_go_editions_words() {
    let data = scratch("acl-refused");
    for (flags, why) in [
        (
            &["--acl-default", "maybe"][..],
            "config: acl-default \"maybe\" must be \"allow\" or \"deny\"",
        ),
        (
            &["--acl-super-user", "admin"],
            "config: acl super user \"admin\" must be written Type:name, such as User:admin",
        ),
        (
            &["--acl-super-user", "User:ok,User:"],
            "acl super user \"User:\" must be written Type:name",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_doze-kafka"))
            .args(["--listen", "127.0.0.1:0", "--data-dir"])
            .arg(&data)
            .args(flags)
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{flags:?}");
        assert!(said.contains(why), "{flags:?}: {said}");
    }
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn a_deny_default_with_no_sasl_binds_the_anonymous_user_and_a_restart_keeps_the_acls() {
    let data = scratch("acl-deny");
    let flags = [
        "--acl-default",
        "deny",
        "--acl-super-user",
        "User:ANONYMOUS",
    ];
    let broker = start_with(&data, &flags);
    let mut c = broker.connect();
    // The anonymous user is the super user here, so it may grant: a topic for nobody in particular is made, then
    // the ACL APIs are used to show the ACL is stored.
    let mut acl = AclCreation::default();
    acl.resource_type = 2;
    acl.resource_name = StrBytes::from_static_str("t");
    acl.resource_pattern_type = 3;
    acl.principal = StrBytes::from_static_str("User:someone");
    acl.host = StrBytes::from_static_str("*");
    acl.operation = 8;
    acl.permission_type = 3;
    let mut create = CreateAclsRequest::default();
    create.creations.push(acl);
    let made: CreateAclsResponse = call(&mut c, ApiKey::CreateAcls, 3, &create);
    assert_eq!(made.results[0].error_code, 0);
    drop((c, broker));

    // Started again without the super user: the stored ACL is there, and the anonymous user can do nothing.
    let broker = start_with(&data, &["--acl-default", "deny"]);
    let mut c = broker.connect();
    let mut asks = MetadataRequest::default();
    let mut topic = kafka_protocol::messages::metadata_request::MetadataRequestTopic::default();
    topic.name = Some(kafka_protocol::messages::TopicName(
        StrBytes::from_static_str("t"),
    ));
    asks.topics = Some(vec![topic]);
    let seen: MetadataResponse = call(&mut c, ApiKey::Metadata, 9, &asks);
    assert_eq!(
        seen.topics[0].error_code, 29,
        "the anonymous user may not describe `t`"
    );
    let again: CreateAclsResponse = call(&mut c, ApiKey::CreateAcls, 3, &create);
    assert_eq!(again.results[0].error_code, 31, "nor use the ACL APIs");
    drop((c, broker));
    let _ = std::fs::remove_dir_all(&data);
}

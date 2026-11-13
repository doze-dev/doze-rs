//! More of the Go package's tests, ported: subtree ownership, stacks, the format stamp, declared domains, routes.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use super::tests::{Other, home, reg};
use super::*;
use crate::domains::{lookup_domain, route_domain, route_domains, valid_domain};
use crate::{Error, Name};

#[test]
fn a_registered_name_owns_its_subtree() {
    // Go: TestARegisteredNameOwnsItsSubtree. Names doze-aws mints at runtime cannot be claimed in advance.
    let r = reg(&home(), "doze-aws");
    let want = r.claim(&Name::qualified("aws", "harbour")).unwrap().ip;
    for host in [
        "aws.harbour.doze",
        "sqs.ap-south-1.aws.harbour.doze",
        "receipts.s3.us-east-1.aws.harbour.doze",
        "x70an6eshc.execute-api.ap-south-1.aws.harbour.doze",
        "AWS.HARBOUR.DOZE",
        "sqs.ap-south-1.aws.harbour.doze.",
    ] {
        assert_eq!(r.resolve(host), Some(want), "{host}");
    }
}

#[test]
fn an_unclaimed_name_still_fails() {
    // Go: TestAnUnclaimedNameStillFails. The walk must not make the resolver a catch-all.
    let r = reg(&home(), "doze-aws");
    r.claim(&Name::apex("aws")).unwrap();
    r.claim(&Name::qualified("aws", "harbour")).unwrap();
    for host in [
        "doze",
        "anything.doze",
        "kafka.doze",
        "aws.harbor.doze",
        "sqs.aws.harbor.doze",
        "aws.harbour.doze.evil.com",
        "harbour.doze",
    ] {
        assert_eq!(r.resolve(host), None, "{host} must stay NXDOMAIN");
    }
}

#[test]
fn the_most_specific_name_wins_and_the_front_door_follows_the_same_walk() {
    // Go: TestTheMostSpecificNameWins, TestTheFrontDoorFollowsTheSameWalk, TestURLForAnswersForASubdomain.
    let r = reg(&home(), "doze-aws");
    let apex = r.claim(&Name::apex("aws")).unwrap();
    let inst = r.claim(&Name::qualified("aws", "harbour")).unwrap();
    assert_ne!(apex.ip, inst.ip);
    assert_eq!(r.resolve("sqs.ap-south-1.aws.doze"), Some(apex.ip));
    assert_eq!(r.resolve("sqs.ap-south-1.aws.harbour.doze"), Some(inst.ip));
    inst.route("127.0.0.4:4566").unwrap();
    for host in [
        "aws.harbour.doze",
        "sqs.ap-south-1.aws.harbour.doze",
        "sqs.ap-south-1.aws.harbour.doze:80",
    ] {
        assert_eq!(
            r.front_target(host).as_deref(),
            Some("127.0.0.4:4566"),
            "{host}"
        );
    }
    assert_eq!(r.front_target("aws.harbor.doze"), None);
    let host = "sqs.ap-south-1.aws.harbour.doze";
    assert_eq!(
        r.url_for(host),
        Some(format!("http://{host}:4566")),
        "no front door: the port-ful form"
    );
    assert_eq!(r.url_for("aws.harbor.doze"), None);
}

#[test]
fn a_stack_name_belongs_to_one_project() {
    // Go: TestAStackNameBelongsToOneProject.
    let h = home();
    let first = reg(&h, "doze");
    let _held = first.claim_stack("shop", "/work/shop").unwrap();
    let daemon = Other::spawn();
    let mut other = reg(&h, "doze");
    other.pid = daemon.pid();
    match other.claim_stack("shop", "/elsewhere/shop") {
        Err(Error::StackTaken { dir, .. }) => assert_eq!(dir, "/work/shop"),
        x => panic!("{x:?}"),
    }
    other.claim_stack("shop", "/work/shop").unwrap();
    other.claim_stack("blog", "/elsewhere/blog").unwrap();
    assert!(
        !first.snapshot().contains_key("shop.doze"),
        "a stack claim is not a name"
    );
    assert_eq!(first.resolve("db.shop.doze"), None);
    let third_proc = Other::spawn();
    let mut third = reg(&h, "doze");
    third.pid = third_proc.pid();
    assert!(third.claim_stack("blog", "/third/blog").is_err());
}

#[test]
fn the_format_is_stamped_and_hidden_and_an_unstamped_file_is_read() {
    // Go: TestFormatIsStampedAndHidden, TestAnUnstampedFileIsRead.
    let h = home();
    let r = reg(&h, "doze");
    r.claim(&Name::qualified("db", "shop")).unwrap();
    let raw: BTreeMap<String, Entry> =
        serde_json::from_slice(&std::fs::read(r.path()).unwrap()).unwrap();
    let stamp = &raw[FORMAT_KEY];
    assert_eq!(
        (stamp.format, stamp.pid),
        (FORMAT_VERSION, i64::from(std::process::id()))
    );
    assert!(!r.snapshot().contains_key(FORMAT_KEY));
    assert_eq!(r.resolve("_format"), None);

    let old = home();
    let pid = std::process::id();
    let unstamped = format!(
        r#"{{"db.shop.doze":{{"ip":"127.0.0.20","pid":{pid},"owner":"doze","tier":"qualified"}}}}"#
    );
    std::fs::write(old.join(FILE_NAME), unstamped).unwrap();
    let r = reg(&old, "doze-aws");
    assert_eq!(r.resolve("db.shop.doze").unwrap().to_string(), "127.0.0.20");
    let raw: BTreeMap<String, Entry> =
        serde_json::from_slice(&std::fs::read(r.path()).unwrap()).unwrap();
    assert_eq!(raw[FORMAT_KEY].format, FORMAT_VERSION, "reading stamped it");
}

#[test]
fn a_newer_file_is_not_touched() {
    // Go: TestANewerFileIsNotTouched.
    let h = home();
    let newer = r#"{"_format":{"ip":"","pid":1,"owner":"doze","tier":"","format":99},"db.shop.doze":{"ip":"127.0.0.20","pid":1,"owner":"doze","tier":"qualified","shape":"something this build has never heard of"}}"#;
    std::fs::write(h.join(FILE_NAME), newer).unwrap();
    let r = reg(&h, "doze-aws");
    match r.claim(&Name::qualified("aws", "shop")) {
        Err(Error::NewerFormat { have, want, .. }) => {
            assert_eq!((have, want), (99, FORMAT_VERSION))
        }
        x => panic!("{x:?}"),
    }
    assert_eq!(std::fs::read_to_string(h.join(FILE_NAME)).unwrap(), newer);
    assert!(r.snapshot().is_empty());
}

#[test]
fn a_field_this_build_does_not_know_survives_its_write() {
    // Better than Go: Go dropped unknown fields on every rewrite (the cause of its `heal`).
    let h = home();
    let pid = std::process::id();
    let text = format!(
        r#"{{"db.shop.doze":{{"ip":"127.0.0.20","pid":{pid},"owner":"doze","tier":"qualified","shape":"round"}}}}"#
    );
    std::fs::write(h.join(FILE_NAME), text).unwrap();
    reg(&h, "doze-aws")
        .claim(&Name::qualified("aws", "shop"))
        .unwrap();
    let after = std::fs::read_to_string(h.join(FILE_NAME)).unwrap();
    assert!(after.contains(r#""shape": "round""#), "{after}");
}

#[test]
fn valid_domains_and_their_routes() {
    // Go: TestValidDomainAcceptsNamesUnderAPublicTLDAndRefusesTheRest, TestValidDomainRefusesWhatAShellOrAPathCouldMisread.
    for ok in [
        "dev.acme.com",
        "*.dev.acme.com",
        "app.acme.co.uk",
        "DEV.Acme.com.",
    ] {
        assert!(valid_domain(ok).is_ok(), "{ok}");
    }
    for bad in [
        "",
        "acme",
        "localhost",
        "app.localhost",
        "web.shop.doze",
        "127.0.0.1",
        "a.*.com",
        "*.com",
        "under_score.com",
        "-dash.com",
        "dev.acme.com;rm",
        "dev.acme.com'x",
        "dev.acme.com$(id)",
        "../etc",
        "dev/acme.com",
        "dev.acme.com\n",
        "dev acme.com",
        "dev.acme.com\\x",
        "*.acme.com/x",
        "dev..acme.com",
        ".acme.com",
    ] {
        assert!(valid_domain(bad).is_err(), "{bad:?} accepted");
    }
    assert_eq!(route_domain("*.dev.acme.com"), "dev.acme.com");
    let routes = route_domains(&[
        "*.dev.acme.com".into(),
        "dev.acme.com".into(),
        "api.acme.com".into(),
    ]);
    assert_eq!(routes, ["api.acme.com", "dev.acme.com"]);
}

#[test]
fn a_domain_is_the_apps_twin_and_a_wildcard_owns_what_is_under_it() {
    // Go: TestADomainIsTheAppsTwinAndAnswersForItsSubtree, TestAWildcardOwnsTheNameUnderItAndOneLevelBelow (the
    // resolving half; the certificate half is the CA's, N3).
    let r = reg(&home(), "doze");
    let web = r.claim(&Name::qualified("web", "shop")).unwrap();
    let dom = web.twin(&Name::domain("dev.acme.com")).unwrap();
    let wild = web.twin(&Name::domain("*.dev.acme.com")).unwrap();
    dom.route("127.0.0.1:3000").unwrap();
    wild.route("127.0.0.1:3000").unwrap();
    assert_eq!(r.resolve("dev.acme.com"), Some(web.ip));
    assert_eq!(r.resolve("tenant.dev.acme.com"), Some(web.ip));
    assert!(r.resolve("deep.tenant.dev.acme.com").is_some());
    for nope in ["acme.com", "other.com", "nope.doze"] {
        assert_eq!(r.resolve(nope), None, "{nope}");
    }
    assert_eq!(
        r.front_target("tenant.dev.acme.com:443").as_deref(),
        Some("127.0.0.1:3000")
    );
    assert!(r.is_domain("dev.acme.com") && r.is_domain("x.dev.acme.com:80"));
    assert!(!r.is_domain("web.shop.doze") && !r.is_domain("other.com"));
    assert_eq!(r.domains(), ["*.dev.acme.com", "dev.acme.com"]);
    let m = r.snapshot();
    for h in ["tenant.dev.acme.com", "a.b.dev.acme.com"] {
        assert_eq!(
            lookup_domain(&m, h).map(|(_, o)| o).as_deref(),
            Some("*.dev.acme.com"),
            "{h}"
        );
    }
    assert_eq!(
        lookup_domain(&m, "dev.acme.com").map(|(_, o)| o).as_deref(),
        Some("dev.acme.com")
    );
    assert!(lookup_domain(&m, "acme.com").is_none());
    dom.release().unwrap();
    wild.release().unwrap();
    assert_eq!(r.resolve("dev.acme.com"), None);
    assert_eq!(r.resolve("tenant.dev.acme.com"), None);
}

#[test]
fn a_url_is_port_less_only_while_a_doze_front_door_holds_80() {
    // Go: URLFor's rule. Asked of the registry, not the network: a dial only proves something answers.
    let r = reg(&home(), "doze-aws");
    let lease = r.claim(&Name::qualified("aws", "harbour")).unwrap();
    lease.route("127.0.0.4:4566").unwrap();
    let host = "aws.harbour.doze";
    assert_eq!(r.url_for(host), Some(format!("http://{host}:4566")));
    r.claim_holder(crate::front::INGRESS_KEY, "0.0.0.0:80")
        .unwrap();
    assert_eq!(
        r.ingress_holder().map(|e| e.owner).as_deref(),
        Some("doze-aws")
    );
    assert_eq!(r.url_for(host), Some(format!("http://{host}")));
    r.claim_holder(crate::front::INGRESS_KEY, "127.0.0.1:18080")
        .unwrap();
    assert_eq!(
        r.url_for(host),
        Some(format!("http://{host}:18080")),
        "a moved front door keeps its port"
    );
    r.release_key(crate::front::INGRESS_KEY).unwrap();
    assert!(r.ingress_holder().is_none());
}

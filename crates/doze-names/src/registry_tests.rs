//! The Go package's registry, allocator, subtree, stack, format and domain tests, ported one for one (each cites its
//! Go name). They run on the loopback network unless they say otherwise, so they mean the same on a Mac as on Linux.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use super::*;
use crate::pool::{LOOPBACK, VIRTUAL};
use crate::{Error, Name, Tier};
use std::process::{Child, Command, Stdio};

pub(crate) fn home() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let i = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("doze-names-{}-{i}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub(super) fn reg(home: &Path, owner: &str) -> Registry {
    let mut r = Registry::open(home, owner);
    r.net = LOOPBACK;
    r
}

/// A living process that is not this one, killed when dropped.
pub(super) struct Other(pub(super) Child);

impl Other {
    pub(super) fn spawn() -> Other {
        let child = if cfg!(windows) {
            Command::new("ping")
                .args(["-n", "60", "127.0.0.1"])
                .stdout(Stdio::null())
                .spawn()
        } else {
            Command::new("sleep").arg("60").spawn()
        };
        Other(child.unwrap())
    }
    pub(super) fn pid(&self) -> u32 {
        self.0.id()
    }
}

impl Drop for Other {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A pid that has exited and been reaped.
fn dead_pid() -> u32 {
    let mut c = Other::spawn();
    let pid = c.pid();
    let _ = c.0.kill();
    let _ = c.0.wait();
    pid
}

#[test]
fn apex_address_is_fixed() {
    // Go: TestApexAddressIsFixed. These go into /etc/hosts at setup; moving one strands every machine set up.
    let r = reg(&home(), "doze-aws");
    for (service, want) in [("aws", "127.0.0.2"), ("kafka", "127.0.0.3")] {
        assert_eq!(r.claim(&Name::apex(service)).unwrap().ip.to_string(), want);
    }
}

#[test]
fn apex_name_is_first_come_and_names_the_holder() {
    // Go: TestApexNameIsFirstComeAndNamesTheHolder.
    let h = home();
    reg(&h, "doze-aws").claim(&Name::apex("aws")).unwrap();
    let other = Other::spawn();
    let mut second = reg(&h, "doze");
    second.pid = other.pid();
    match second.claim(&Name::apex("aws")) {
        Err(Error::Held { pid, owner, .. }) => {
            assert_eq!((pid, owner.as_str()), (std::process::id(), "doze-aws"))
        }
        other => panic!("second claim = {other:?}, want Held"),
    }
    // The loser's qualified name is unaffected: losing the apex race costs nothing functional.
    second.claim(&Name::qualified("cloud", "shop")).unwrap();
}

#[test]
fn dead_holder_is_pruned() {
    // Go: TestDeadHolderIsPruned. A kill -9 runs no shutdown hook.
    let h = home();
    let mut ghost = reg(&h, "doze-aws");
    ghost.pid = dead_pid();
    ghost.claim(&Name::apex("aws")).unwrap();
    let lease = reg(&h, "doze-kafka").claim(&Name::apex("aws")).unwrap();
    assert_eq!(lease.ip.to_string(), "127.0.0.2");
}

#[test]
fn resolve_answers_every_peers_names() {
    // Go: TestResolveAnswersEveryPeersNames. The property the design rests on.
    let h = home();
    let aws = reg(&h, "doze-aws");
    aws.claim(&Name::apex("aws")).unwrap();
    let kafka = reg(&h, "doze-kafka");
    kafka.claim(&Name::apex("kafka")).unwrap();
    assert_eq!(kafka.resolve("aws.doze").unwrap().to_string(), "127.0.0.2");
    assert_eq!(aws.resolve("kafka.doze").unwrap().to_string(), "127.0.0.3");
    assert!(
        aws.resolve("AWS.Doze.").is_some(),
        "case and the trailing dot, as resolvers receive names"
    );
    assert!(aws.resolve("nope.doze").is_none());
    assert!(aws.resolve("example.com").is_none());
}

#[test]
fn release_frees_the_name() {
    // Go: TestReleaseFreesTheName.
    let h = home();
    let r = reg(&h, "doze-aws");
    r.claim(&Name::apex("aws")).unwrap().release().unwrap();
    assert!(r.resolve("aws.doze").is_none());
    let other = Other::spawn();
    let mut o = reg(&h, "doze");
    o.pid = other.pid();
    o.claim(&Name::apex("aws")).unwrap();
}

#[test]
fn qualified_addresses_are_stable_and_distinct() {
    // Go: TestQualifiedAddressesAreStableAndDistinct.
    let r = reg(&home(), "doze");
    let a = r.claim(&Name::qualified("cloud", "shop")).unwrap();
    let b = r.claim(&Name::qualified("cache", "shop")).unwrap();
    assert_ne!(a.ip, b.ip);
    for ip in [a.ip.to_string(), b.ip.to_string()] {
        assert!(
            !["192.0.2.53", "127.0.0.2", "127.0.0.3"].contains(&ip.as_str()),
            "{ip}"
        );
    }
    let c = reg(&home(), "doze")
        .claim(&Name::qualified("cloud", "shop"))
        .unwrap();
    assert_eq!(
        c.ip, a.ip,
        "the same name lands on the same address with nothing persisted"
    );
}

#[test]
fn unknown_apex_service_is_refused() {
    // Go: TestUnknownApexServiceIsRefused.
    assert!(matches!(
        reg(&home(), "doze").claim(&Name::apex("mystery")),
        Err(Error::NoApexAddress(_))
    ));
}

#[test]
fn the_pool_is_wide_and_a_name_keeps_its_address() {
    // Go: TestThePoolIsWideAndANameKeepsItsAddress.
    let r = reg(&home(), "doze");
    let mut seen = std::collections::BTreeSet::new();
    let mut beyond = false;
    for i in 0..300 {
        let lease = r
            .claim(&Name::qualified(&format!("svc{i}"), "shop"))
            .unwrap();
        let off = LOOPBACK.offset(lease.ip).unwrap();
        assert!(
            off >= 10 && off & 0xff != 0 && off & 0xff != 255,
            "{}",
            lease.ip
        );
        assert!(seen.insert(lease.ip), "{} handed out twice", lease.ip);
        beyond |= off > 255;
    }
    assert!(
        beyond,
        "300 services all fit in 127.0.0.x: the block is not being used"
    );
}

#[test]
fn no_setup_still_names_an_address_and_a_mac_uses_198_19() {
    // Go: TestNoSetupStillNamesAnAddress (the address part) and TestANameHasTheSameOffsetOnBothSystems.
    let mut r = reg(&home(), "doze");
    r.net = VIRTUAL;
    let a = r.claim(&Name::qualified("db", "shop")).unwrap();
    assert_eq!(&a.ip.octets()[..2], &[198, 19]);
    let b = reg(&home(), "doze")
        .claim(&Name::qualified("db", "shop"))
        .unwrap();
    assert_eq!(VIRTUAL.offset(a.ip), LOOPBACK.offset(b.ip));
}

#[test]
fn a_released_address_is_reused_and_claim_at_refuses_one_in_use() {
    // Go: TestAReleasedAddressIsReused and TestClaimAtRefusesAnAddressInUse.
    let r = reg(&home(), "doze");
    let db = r.claim(&Name::qualified("db", "shop")).unwrap();
    match r.claim_at(&Name::qualified("cache", "shop"), db.ip) {
        Err(Error::AddrTaken { host, .. }) => assert_eq!(host, "db.shop.doze"),
        other => panic!("{other:?}"),
    }
    r.claim_at(&Name::qualified("db", "shop"), db.ip).unwrap();
    r.claim_at(
        &Name::qualified("cache", "shop"),
        "127.0.0.200".parse().unwrap(),
    )
    .unwrap();
    db.release().unwrap();
    r.claim_at(&Name::qualified("new", "shop"), db.ip).unwrap();
}

#[test]
fn claim_at_shares_the_loopback_address() {
    // Go: TestClaimAtSharesTheLoopbackAddress. 127.0.0.1 is shared on purpose, on either network.
    for net in [LOOPBACK, VIRTUAL] {
        let mut r = reg(&home(), "doze");
        r.net = net;
        for svc in ["db", "cache", "api"] {
            r.claim_at(&Name::qualified(svc, "shop"), "127.0.0.1".parse().unwrap())
                .unwrap();
        }
    }
}

#[test]
fn a_program_may_share_an_address_between_its_own_names() {
    // Go: TestAProgramMayShareAnAddressBetweenItsOwnNames.
    let h = home();
    let aws = reg(&h, "doze-aws")
        .claim(&Name::qualified("aws", "shop"))
        .unwrap();
    let twin = Name {
        host: format!("sync-{}", aws.name.host),
        tier: Tier::Qualified,
    };
    aws.twin(&twin).unwrap();
    let other = Other::spawn();
    let mut o = reg(&h, "doze");
    o.pid = other.pid();
    assert!(matches!(
        o.claim_at(&Name::qualified("cache", "shop"), aws.ip),
        Err(Error::AddrTaken { .. })
    ));
}

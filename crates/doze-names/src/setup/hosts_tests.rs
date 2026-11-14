use super::*;

const OTHER_PEOPLES_HOSTS: &str = "##
# Host Database
##
127.0.0.1\tlocalhost
255.255.255.255\tbroadcasthost
::1             localhost

# Added by Docker Desktop
192.168.1.10 host.docker.internal
";

fn keeps_every_line(text: &str) {
    for line in OTHER_PEOPLES_HOSTS.trim().lines() {
        assert!(
            text.contains(line),
            "lost a line that was not ours: {line:?}"
        );
    }
}

/// Go: TestHostsBlockRoundTrip. Added, idempotent (DDEV once appended on every run and people ended up with enormous
/// files: this is the test that makes that impossible), and removed leaving the original behind.
#[test]
fn the_hosts_block_round_trips() {
    let want = apex_block();
    let added = replace_block(OTHER_PEOPLES_HOSTS, &want);
    assert!(added.contains(&want));
    keeps_every_line(&added);
    let twice = replace_block(&added, &want);
    assert_eq!(twice, added, "a second write changed the file");
    assert_eq!(twice.matches(BEGIN).count(), 1);
    let removed = replace_block(&added, "");
    assert!(
        !removed.contains(BEGIN) && !removed.contains("aws.doze"),
        "{removed}"
    );
    keeps_every_line(&removed);
    assert_eq!(
        removed, OTHER_PEOPLES_HOSTS,
        "removal gives back the file as it was"
    );
}

/// Go: TestHostsBlockSurvivesAMissingEndMarker. Someone hand-edited the file and deleted the end marker; truncating to
/// the end of the file would eat whatever came after, including entries another tool depends on.
#[test]
fn a_damaged_block_takes_nothing_else_with_it() {
    let damaged = format!(
        "{OTHER_PEOPLES_HOSTS}{BEGIN}\n127.0.0.2\taws.doze\n# Added by something else\n10.0.0.5 important.internal\n"
    );
    let fixed = replace_block(&damaged, &apex_block());
    assert!(fixed.contains("10.0.0.5 important.internal"));
}

/// Go: TestHostsBlockOnAnEmptyFile.
#[test]
fn the_block_on_an_empty_file() {
    let out = replace_block("", &apex_block());
    assert!(out.contains("aws.doze"));
    assert_eq!(
        replace_block(&out, &apex_block()),
        out,
        "idempotent from empty"
    );
}

/// Go: TestApexBlockIsByteStable. Check compares the file with this string, so an unstable render would report "not
/// installed" at random; and every reserved name is in it at its fixed address.
#[test]
fn the_apex_block_is_byte_stable_and_complete() {
    let first = apex_block();
    assert_eq!(apex_block(), first);
    for (service, offset) in apex_services() {
        let line = format!("{}\t{service}.doze\n", LOOPBACK.at(offset));
        assert!(first.contains(&line), "the block omits {line:?}");
    }
}

/// Go: TestHostsBlockInstalledReadsTheFile, on a file of the test's own: nothing here touches /etc/hosts.
#[test]
fn block_installed_reads_the_file() {
    let dir = std::env::temp_dir().join(format!("doze-names-hosts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("hosts");
    std::fs::write(&path, OTHER_PEOPLES_HOSTS).unwrap();
    assert!(
        !block_installed(&path),
        "installed before anything was written"
    );
    std::fs::write(&path, replace_block(OTHER_PEOPLES_HOSTS, &apex_block())).unwrap();
    assert!(block_installed(&path));
    let _ = std::fs::remove_dir_all(&dir);
}

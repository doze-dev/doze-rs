use super::*;
use crate::trust_store::{MAC_RELOAD_RESOLVERS, SYSTEM_KEYCHAIN, mac_resolver_port};

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("doze-names-trust-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// Go: TestTheMacOSScriptsQuoteAndTouchOnlyWhatTheyAreAskedTo. Routes are written and removed as asked, quoted, and a
/// route another stack keeps is not rewritten; the root is trusted for ssl and basic in the System keychain, untrusted
/// by file from the admin store and then by hash; a rotated-out root is untrusted by its kept file; a fingerprint that
/// is not one never reaches a script.
#[test]
fn the_macos_scripts_quote_and_touch_only_what_they_are_asked_to() {
    let s = scripts::routes(
        Os::Mac,
        &strings(&["dev.acme.com", "api.acme.com", "other.example"]),
        &strings(&["dev.acme.com", "api.acme.com"]),
        &strings(&["old.acme.com"]),
        &|_| false,
    );
    let port = mac_resolver_port();
    for want in [
        "mkdir -p /etc/resolver".to_string(),
        format!("printf 'nameserver 127.0.0.1\\nport {port}\\n' > '/etc/resolver/dev.acme.com'"),
        "> '/etc/resolver/api.acme.com'".into(),
        "rm -f '/etc/resolver/old.acme.com'".into(),
    ] {
        assert!(s.contains(&want), "routes script lacks {want:?}:\n{s}");
    }
    assert!(
        !s.contains("other.example"),
        "a route not asked for was written:\n{s}"
    );
    assert_reloads_after_every_route(&s);
    let removal = scripts::routes(Os::Mac, &[], &[], &strings(&["old.acme.com"]), &|_| false);
    assert_reloads_after_every_route(&removal);
    assert!(
        removal.starts_with("rm -f '/etc/resolver/old.acme.com'"),
        "{removal}"
    );
    let foreign = scripts::routes(
        Os::Mac,
        &[],
        &strings(&["x.example"]),
        &strings(&["y.example"]),
        &|_| true,
    );
    assert!(
        foreign.is_empty(),
        "someone else's route files are never touched: {foreign}"
    );
    macos_keychain_scripts();
}

/// macOS does not read a resolver file written or removed until mDNSResponder is told to (seen on macOS 27: over two
/// minutes, and a network-change notification did not help): the script ends with the HUP, once, after every write and
/// removal, and tolerates its failure.
fn assert_reloads_after_every_route(s: &str) {
    let lines: Vec<&str> = s.lines().collect();
    let hup = lines
        .iter()
        .position(|l| *l == MAC_RELOAD_RESOLVERS)
        .unwrap_or_else(|| panic!("no reload in\n{s}"));
    assert_eq!(hup, lines.len() - 1, "the reload comes last:\n{s}");
    assert_eq!(s.matches("killall -HUP mDNSResponder").count(), 1, "{s}");
    assert!(MAC_RELOAD_RESOLVERS.ends_with("|| true"));
    assert!(
        lines[..hup].iter().all(|l| l.contains("/etc/resolver")),
        "{s}"
    );
}

/// The keychain half of the test above. The certificate goes into the System keychain as root; its trust is set and
/// removed as the person, never in root's script, where macOS cannot show the dialog the change needs.
fn macos_keychain_scripts() {
    let dir = scratch("mac").join("ca");
    let ca = Ca::load_or_create(&dir).unwrap();
    let root = ca.root_path().display().to_string();
    let ts = scripts::trust(Os::Mac, &ca, false);
    assert_eq!(
        ts.root,
        format!("security add-certificates -k {SYSTEM_KEYCHAIN} '{root}'")
    );
    assert_eq!(
        ts.user,
        format!("security add-trusted-cert -d -r trustRoot -p ssl -p basic '{root}'")
    );
    assert!(
        !ts.root.contains("trusted-cert"),
        "trust settings never run as root: {ts:?}"
    );
    let again = scripts::trust(Os::Mac, &ca, true);
    assert_eq!(
        again.root, "",
        "a root the keychain holds already is not added again"
    );
    assert_eq!(again.user, ts.user);

    let us = scripts::untrust(Os::Mac, Some(&ca));
    assert_eq!(
        us.user,
        format!("security remove-trusted-cert -d '{root}' 2>/dev/null || true")
    );
    assert!(
        us.root
            .contains(&format!("delete-certificate -t -Z '{}'", ca.fingerprint()))
            && !us.root.contains("trusted-cert"),
        "{us:?}"
    );

    let old = ca.fingerprint();
    let rotated = Ca::rotate(&dir).unwrap();
    assert_eq!(rotated.previous(), std::slice::from_ref(&old));
    let again = Ca::probe(&dir).unwrap().unwrap();
    assert_eq!(
        again.previous().len(),
        1,
        "the rotated-out fingerprint is kept on disk"
    );
    let us = scripts::untrust_fingerprint(Os::Mac, Some(&again), &old);
    assert!(
        us.user.contains(&format!(
            "remove-trusted-cert -d '{}'",
            again.previous_path(&old).display()
        )) && !us.root.contains("trusted-cert"),
        "a rotated-out root is untrusted by its kept file, as the person: {us:?}"
    );
    assert_eq!(
        scripts::untrust_fingerprint(Os::Mac, Some(&again), "nope"),
        scripts::Lines::default()
    );
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
}

/// Go: TestTrustAndUntrustTouchOnlyTheRequestsRoutes. The prompt names what the script does, and the script does only
/// that: with two projects recorded under one home, the second's trust writes its own route only, and its removal
/// removes its own route only, never installing the first's. Print changes nothing.
#[test]
fn trust_and_untrust_touch_only_the_requests_routes() {
    let home = scratch("home");
    let (one, two) = (scratch("one"), scratch("two"));
    for d in [&one, &two] {
        std::fs::write(d.join("doze.hcl"), "app \"web\" {}\n").unwrap();
    }
    let (one_s, two_s) = (one.display().to_string(), two.display().to_string());
    let routes = Routes::with_check(&home, |_| false);
    // Project one only ever ran doze up: its domain is recorded, not routed.
    routes
        .record(&one_s, &strings(&["dev.aud.example"]))
        .unwrap();
    routes
        .record(&two_s, &strings(&["api.aud.example"]))
        .unwrap();

    let mut req = TrustRequest {
        domains: strings(&["api.aud.example"]),
        home: Some(home.clone()),
        ..TrustRequest::default()
    };
    let mut out = Vec::new();
    let mut o = Options {
        out: &mut out,
        how: How::Print,
    };
    trust(Os::Mac, &mut o, &mut req).unwrap();
    let script = String::from_utf8(std::mem::take(&mut out)).unwrap();
    assert!(
        script.contains("# route api.aud.example to doze"),
        "{script}"
    );
    assert!(
        script.contains("'/etc/resolver/api.aud.example'"),
        "{script}"
    );
    assert!(
        !script.contains("dev.aud.example"),
        "another project's route was written:\n{script}"
    );
    let at = |s: &str, needle: &str| {
        s.find(needle)
            .unwrap_or_else(|| panic!("no {needle:?} in\n{s}"))
    };
    assert!(
        at(&script, "> '/etc/resolver/api.aud.example'") < at(&script, MAC_RELOAD_RESOLVERS),
        "the reload comes after the write:\n{script}"
    );

    let mut o = Options {
        out: &mut out,
        how: How::Print,
    };
    untrust(Os::Mac, &mut o, &req).unwrap();
    let script = String::from_utf8(out).unwrap();
    assert!(
        script.contains("rm -f '/etc/resolver/api.aud.example'"),
        "{script}"
    );
    assert!(
        !script.contains("printf") && !script.contains("dev.aud.example"),
        "removal installs nothing:\n{script}"
    );
    assert!(
        at(&script, "rm -f '/etc/resolver/api.aud.example'") < at(&script, MAC_RELOAD_RESOLVERS),
        "the reload comes after the removal too:\n{script}"
    );
    assert_eq!(routes.claimants("dev.aud.example"), [one_s]);
    assert_eq!(routes.claimants("api.aud.example"), [two_s]);
    for d in [home, one, two] {
        let _ = std::fs::remove_dir_all(d);
    }
}

/// Linux restates the whole list (the zone and every kept route) now and in the zone unit's drop-in; Windows writes
/// and removes a rule per route, both namespaces of the domain, and only its own; the authority goes into the
/// distribution's bundle or the machine's Root store.
#[test]
fn linux_restates_the_list_and_windows_writes_a_rule_per_route() {
    let keep = strings(&["dev.acme.com", "api.acme.com"]);
    let s = scripts::routes(Os::Linux, &keep, &strings(&["api.acme.com"]), &[], &|_| {
        false
    });
    assert!(
        s.contains("resolvectl domain doze0 ~doze ~dev.acme.com ~api.acme.com 2>/dev/null || true"),
        "{s}"
    );
    assert!(
        s.contains("ExecStart=-/usr/bin/resolvectl domain doze0 ~doze ~dev.acme.com ~api.acme.com"),
        "{s}"
    );
    assert!(scripts::restates_routes(Os::Linux) && !scripts::restates_routes(Os::Windows));

    let w = scripts::routes(
        Os::Windows,
        &keep,
        &strings(&["api.acme.com"]),
        &strings(&["old.acme.com"]),
        &|_| false,
    );
    assert!(
        w.contains("Add-DnsClientNrptRule -Namespace 'api.acme.com','.api.acme.com'"),
        "{w}"
    );
    assert!(w.contains("-Comment 'doze route'"), "{w}");
    assert!(w.contains("$_.Namespace -contains '.old.acme.com'"), "{w}");
    assert!(!w.contains("dev.acme.com"), "only what was asked: {w}");
    assert!(
        !s.contains("mDNSResponder") && !w.contains("mDNSResponder"),
        "the reload is macOS's alone"
    );

    let dir = scratch("stores").join("ca");
    let ca = Ca::load_or_create(&dir).unwrap();
    let lt = scripts::trust(Os::Linux, &ca, false);
    assert!(lt.root.contains("cp -f '") && lt.user.is_empty(), "{lt:?}");
    let wt = scripts::trust(Os::Windows, &ca, false);
    assert!(wt.user.is_empty(), "{wt:?}");
    let wt = wt.root;
    assert!(
        wt.starts_with("certutil.exe -addstore -f Root '") && wt.contains("exit $LASTEXITCODE"),
        "{wt}"
    );
    assert!(
        scripts::untrust(Os::Windows, Some(&ca))
            .root
            .contains(&format!("-eq '{}'", ca.fingerprint()))
    );
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
}

/// The zone's https needs no declared domain: with no authority made yet, the plan names the root file it will be made
/// at (so a dry run shows the real script and makes nothing), trusted for TLS only; once it is made, the same file. On
/// macOS root's script only adds the certificate to the System keychain; the trust is the person's part, and the
/// prompt says macOS asks for it in its own dialog.
#[test]
fn https_for_the_zone_trusts_the_authority_before_and_after_it_is_made() {
    let home = scratch("https");
    let root = crate::ca::root_file(&ca_dir(&home)).display().to_string();
    let mac = https_plan(Os::Mac, &home, None).unwrap();
    assert_eq!(
        mac.script,
        format!("security add-certificates -k {SYSTEM_KEYCHAIN} '{root}'\n")
    );
    assert_eq!(
        mac.as_user,
        format!("security add-trusted-cert -d -r trustRoot -p ssl -p basic '{root}'\n")
    );
    assert!(
        mac.what.starts_with("trust the doze certificate authority")
            && mac.what.ends_with(MAC_TRUST_DIALOG),
        "{}",
        mac.what
    );
    assert!(!ca_dir(&home).exists(), "planning makes nothing");
    let linux = https_plan(Os::Linux, &home, None).unwrap();
    assert!(
        linux.script.contains(&format!("cp -f '{root}'")) && linux.as_user.is_empty(),
        "{linux:?}"
    );
    assert!(
        !linux.what.contains(MAC_TRUST_DIALOG),
        "only a plan with a person's part says macOS asks"
    );
    let ca = Ca::load_or_create(&ca_dir(&home)).unwrap();
    assert_eq!(ca.root_path().display().to_string(), root);
    if !ca_trusted(&ca).0 {
        let made = https_plan(Os::Mac, &home, Some(&ca)).unwrap();
        assert!(
            made.as_user.contains(&format!("'{root}'")),
            "{}",
            made.as_user
        );
    }
    let _ = std::fs::remove_dir_all(&home);
}

/// Setup is idempotent about the authority: one already trusted is left alone (no plan, no rotation, nothing removed);
/// one in the keychain but untrusted (a cancelled dialog) is only trusted, with no root script and so no password
/// prompt of doze's; rotated-out roots still trusted are untrusted as the person.
#[test]
fn an_authority_already_in_place_is_left_alone() {
    let home = scratch("idem");
    let ca = Ca::load_or_create(&ca_dir(&home)).unwrap();
    let trusted = CaFacts {
        trusted: true,
        in_keychain: true,
        old_trusted: Vec::new(),
    };
    for os in [Os::Mac, Os::Linux, Os::Windows] {
        assert_eq!(
            https_plan_with(os, &home, Some((&ca, &trusted))),
            None,
            "{os:?}: a trusted authority plans nothing"
        );
    }
    let cancelled = CaFacts {
        trusted: false,
        in_keychain: true,
        old_trusted: Vec::new(),
    };
    let p = https_plan_with(Os::Mac, &home, Some((&ca, &cancelled))).unwrap();
    assert_eq!(p.script, "", "nothing for root: no doze password prompt");
    assert!(
        p.as_user.starts_with("security add-trusted-cert -d "),
        "{p:?}"
    );
    let old = ca.fingerprint();
    let rotated = Ca::rotate(&ca_dir(&home)).unwrap();
    let with_old = CaFacts {
        trusted: true,
        in_keychain: true,
        old_trusted: vec![old.clone()],
    };
    let p = https_plan_with(Os::Mac, &home, Some((&rotated, &with_old))).unwrap();
    assert!(
        !p.script.contains("add-") && !p.as_user.contains("add-"),
        "the trusted current root is not touched: {p:?}"
    );
    assert!(
        p.script
            .contains(&format!("delete-certificate -t -Z '{old}'"))
            && p.as_user.contains("remove-trusted-cert -d "),
        "{p:?}"
    );
    let _ = std::fs::remove_dir_all(&home);
}

/// Removal on macOS: the certificates go as root, the trust settings as the person; uninstall's plan carries the
/// person's part and says macOS asks.
#[test]
fn removal_takes_the_trust_settings_out_as_the_person() {
    let home = scratch("rm");
    let ca = Ca::load_or_create(&ca_dir(&home)).unwrap();
    let req = TrustRequest {
        ca: Ca::probe(&ca_dir(&home)).unwrap(),
        ..TrustRequest::default()
    };
    let p = untrust_plan(Os::Mac, &req).unwrap();
    assert!(!p.script.contains("trusted-cert"), "{p:?}");
    assert!(
        p.script
            .contains(&format!("delete-certificate -t -Z '{}'", ca.fingerprint())),
        "{p:?}"
    );
    assert!(
        p.as_user.contains(&format!(
            "security remove-trusted-cert -d '{}'",
            ca.root_path().display()
        )),
        "{p:?}"
    );
    assert!(p.what.ends_with(MAC_TRUST_DIALOG), "{}", p.what);
    let (root, user) = cleanup_script(Os::Mac, &home);
    assert!(root.iter().all(|l| !l.contains("trusted-cert")), "{root:?}");
    assert!(user.contains("remove-trusted-cert -d "), "{user}");
    let (root, user) = cleanup_script(Os::Linux, &home);
    assert!(user.is_empty() && !root.is_empty(), "{root:?} {user}");
    let _ = std::fs::remove_dir_all(&home);
}

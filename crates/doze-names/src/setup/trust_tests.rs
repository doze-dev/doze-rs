use super::*;
use crate::trust_store::mac_resolver_port;

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

/// The keychain half of the test above.
fn macos_keychain_scripts() {
    let dir = scratch("mac").join("ca");
    let ca = Ca::load_or_create(&dir).unwrap();
    let ts = scripts::trust(Os::Mac, &ca);
    assert!(
        ts.contains("add-trusted-cert -d -r trustRoot -p ssl -p basic -k /Library/Keychains/System.keychain '"),
        "{ts}"
    );
    let us = scripts::untrust(Os::Mac, Some(&ca));
    assert!(
        us.contains(&format!(
            "remove-trusted-cert -d '{}'",
            ca.root_path().display()
        )),
        "{us}"
    );
    assert!(
        us.contains(&format!("delete-certificate -t -Z '{}'", ca.fingerprint())),
        "{us}"
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
        us.contains(&format!(
            "remove-trusted-cert -d '{}'",
            again.previous_path(&old).display()
        )),
        "a rotated-out root is untrusted by its kept file: {us}"
    );
    assert_eq!(
        scripts::untrust_fingerprint(Os::Mac, Some(&again), "nope"),
        ""
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

    let dir = scratch("stores").join("ca");
    let ca = Ca::load_or_create(&dir).unwrap();
    assert!(scripts::trust(Os::Linux, &ca).contains("cp -f '"));
    let wt = scripts::trust(Os::Windows, &ca);
    assert!(
        wt.starts_with("certutil.exe -addstore -f Root '") && wt.contains("exit $LASTEXITCODE"),
        "{wt}"
    );
    assert!(
        scripts::untrust(Os::Windows, Some(&ca)).contains(&format!("-eq '{}'", ca.fingerprint()))
    );
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
}

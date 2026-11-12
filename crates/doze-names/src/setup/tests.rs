use super::*;

fn mac(root: &Path) -> Setup {
    Setup {
        os: Os::Mac,
        paths: Paths::under(root),
        home: PathBuf::from("/Users/me/.doze"),
        daemon: Daemon {
            program: PathBuf::from("/Applications/Doze.app/Contents/MacOS/doze"),
            args: vec![crate::HELPER_ARG.into()],
        },
        user: Some((501, 20)),
    }
}

/// Go: TestCheckSeesADaemonFromAnOlderSetup. The daemon that runs is the copy setup made, so a job from an older setup
/// is reported until setup runs again; no job, or this build's, is not; nor is a job from before the version was
/// recorded at all.
#[test]
fn check_sees_a_daemon_from_an_older_setup() {
    let dir = std::env::temp_dir().join(format!("doze-names-job-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("job.plist");
    assert!(!mac::netd_outdated(&path), "no job is not an old job");
    let current = mac::launchd_plist(&mac(Path::new("/")), 501, 20);
    std::fs::write(&path, &current).unwrap();
    assert!(!mac::netd_outdated(&path), "this build's job is current");
    let older = current.replace(
        &format!("<string>{}</string></dict>", crate::netd::VERSION_DARWIN),
        "<string>3</string></dict>",
    );
    assert_ne!(older, current);
    std::fs::write(&path, older).unwrap();
    assert!(
        mac::netd_outdated(&path),
        "a job naming an earlier version is old"
    );
    let (from, to) = (
        current.find("  <key>EnvironmentVariables</key>").unwrap(),
        current.find("  <key>ProgramArguments</key>").unwrap(),
    );
    std::fs::write(&path, format!("{}{}", &current[..from], &current[to..])).unwrap();
    assert!(mac::netd_outdated(&path), "a job with no version is old");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The macOS job runs the copy with the helper argument, the person's registry and ids; the script stops the job (and
/// the one from before the daemon) before starting the new copy, so re-running setup restarts it, and routes the zone
/// with /etc/resolver/doze on the resolver's port.
#[test]
fn the_macos_script_boots_out_before_bootstrap() {
    let s = mac(Path::new("/r"));
    let plist = mac::launchd_plist(&s, 501, 20);
    let p = |path: &Path| path.display().to_string();
    let args = format!(
        "<string>{}</string>\n    <string>{}</string>\n    <string>{}</string>\n    <string>501</string>\n    \
         <string>20</string>",
        p(&s.paths.helper),
        crate::HELPER_ARG,
        p(&s.registry())
    );
    assert!(plist.contains(&args), "{plist}");
    let script = mac::plan_for(&s, &plist).script;
    let at = |needle: &str| {
        script
            .find(needle)
            .unwrap_or_else(|| panic!("no {needle:?} in\n{script}"))
    };
    let job = shell_quote(&p(&s.paths.launchd_job));
    assert!(
        at(&format!("launchctl bootout system {job}"))
            < at(&format!("launchctl bootstrap system {job}"))
    );
    assert!(
        at(&format!(
            "launchctl bootout system {}",
            shell_quote(&p(&s.paths.old_launchd_job))
        )) < at("cp -f ")
    );
    assert!(
        at(&format!(
            "cp -f '/Applications/Doze.app/Contents/MacOS/doze' {}",
            shell_quote(&p(&s.paths.helper))
        )) > 0
    );
    let port = crate::trust_store::mac_resolver_port();
    assert!(script.ends_with(&format!(
        "printf 'nameserver 127.0.0.1\\nport {port}\\n' > {}\n",
        shell_quote(&p(&s.paths.resolver_dir.join("doze")))
    )));
    let un = mac::uninstall_plan(&s, &["echo cleanup".into()]).script;
    assert!(
        un.contains(&format!(
            "rm -f {}",
            shell_quote(&p(&s.paths.resolver_dir.join("doze")))
        )) && un.ends_with("echo cleanup\n"),
        "{un}"
    );
}

/// Check needs no privilege and reports each part; a machine with nothing set up (every path in an empty folder) is
/// not OK on any system, and says why per step.
#[test]
fn check_on_a_bare_machine_says_what_is_missing() {
    let dir = std::env::temp_dir().join(format!("doze-names-bare-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for os in [Os::Mac, Os::Linux] {
        let mut s = mac(&dir);
        s.os = os;
        s.home = dir.join("home");
        let st = s.check();
        assert!(!st.ok(), "{os:?}: {st}");
        assert!(st.steps.iter().all(|step| !step.detail.is_empty()));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

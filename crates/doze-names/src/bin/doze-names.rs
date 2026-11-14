//! The zone's own command line: machine setup and trust, the network daemon, and a name held by hand. For the root
//! end-to-end runs (Lima VM, Windows CI, the owner's Mac) and for anyone setting a machine up without a doze program;
//! the doze core embeds the same library calls.
//!
//! ```text
//! doze-names check                          what is in place (no privilege)
//! doze-names install   [--print|--prompt]   set the machine up (one prompt)
//! doze-names uninstall [--print|--prompt]   take it all away (one prompt)
//! doze-names trust     [--print|--prompt] [--ca] <domain>...   route domains here, trust the doze CA
//! doze-names untrust   [--print|--prompt] [--ca] [--all] <domain>...
//! doze-names claim <host>...                hold names (and stand in line for the resolver) until killed
//! doze-names __doze-netd <registry> [<uid> <gid>]   the network daemon, as setup installs it
//! ```

use doze_names::ca::{Ca, ca_dir};
use doze_names::setup::trust::{TrustRequest, trust, trust_status, untrust};
use doze_names::setup::{Daemon, How, Options, Os, Setup};
use doze_names::{Error, HELPER_ARG, Name, Registry, in_zone};
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((cmd, rest)) = args.split_first() else {
        return usage();
    };
    let result = match cmd.as_str() {
        HELPER_ARG => daemon(rest),
        "check" => check(),
        "install" | "uninstall" => setup(cmd, rest),
        "trust" | "untrust" => trust_cmd(cmd, rest),
        "claim" => claim(rest),
        _ => return usage(),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("doze-names: {e}");
            ExitCode::FAILURE
        }
    }
}

fn usage() -> ExitCode {
    eprintln!(
        "usage: doze-names check | install | uninstall | trust <domain>... | untrust <domain>... | claim <host>... \
         [--print|--prompt] [--ca] [--all]"
    );
    ExitCode::from(2)
}

fn how<'a>(flags: &[String]) -> How<'a> {
    if flags.iter().any(|f| f == "--print") {
        How::Print
    } else if flags.iter().any(|f| f == "--prompt") {
        How::Prompt
    } else {
        How::Terminal
    }
}

fn setup_here() -> Result<Setup, Error> {
    Ok(Setup::native(Daemon::this_program()?))
}

fn check() -> Result<ExitCode, Error> {
    let st = setup_here()?.check();
    print!("{}", st.platform);
    println!();
    print!("{st}");
    Ok(if st.ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn setup(cmd: &str, flags: &[String]) -> Result<ExitCode, Error> {
    let s = setup_here()?;
    let mut out = std::io::stdout();
    let mut o = Options {
        out: &mut out,
        how: how(flags),
    };
    if cmd == "install" {
        s.install(&mut o)?;
    } else {
        s.uninstall(&mut o)?;
    }
    Ok(ExitCode::SUCCESS)
}

fn trust_cmd(cmd: &str, rest: &[String]) -> Result<ExitCode, Error> {
    let home = doze_names::setup::setup_home();
    let (flags, domains): (Vec<String>, Vec<String>) =
        rest.iter().cloned().partition(|a| a.starts_with("--"));
    let ca = if flags.iter().any(|f| f == "--ca") {
        Some(Ca::load_or_create(&ca_dir(&home))?)
    } else {
        None
    };
    let mut req = TrustRequest {
        domains,
        ca,
        home: Some(home),
        untrust_all: flags.iter().any(|f| f == "--all"),
        ..TrustRequest::default()
    };
    let mut out = std::io::stdout();
    let mut o = Options {
        out: &mut out,
        how: how(&flags),
    };
    if cmd == "trust" {
        trust(Os::native(), &mut o, &mut req)?;
    } else {
        untrust(Os::native(), &mut o, &req)?;
        for st in trust_status(&req) {
            println!(
                "{} {} · {}",
                if st.done { "✓" } else { "✗" },
                st.name,
                st.detail
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Holds each host (a zone name, or a domain outside it) and takes its turn at the resolver, until killed: what any
/// doze program does, for tests and by hand. Prints `<host> <address>` per name, then `ready`.
fn claim(hosts: &[String]) -> Result<ExitCode, Error> {
    let reg = Registry::open(&doze_names::home(), "doze-names claim");
    let mut leases = Vec::new();
    for host in hosts {
        let name = if in_zone(host) {
            let labels = host.trim_end_matches(".doze").split('.').count();
            let tier = if labels == 1 {
                doze_names::Tier::Apex
            } else {
                doze_names::Tier::Qualified
            };
            Name {
                host: host.to_ascii_lowercase(),
                tier,
            }
        } else {
            Name::domain(host)
        };
        let lease = reg.claim(&name)?;
        println!("{} {}", lease.name.host, lease.ip);
        leases.push(lease);
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let mut cfg = doze_names::resolver::ResolverConfig::from_registry(&reg);
        cfg.log = std::sync::Arc::new(|line| eprintln!("resolver: {line}"));
        let _resolver = doze_names::resolver::serve(cfg);
        println!("ready");
        std::future::pending::<()>().await;
    });
    drop(leases);
    Ok(ExitCode::SUCCESS)
}

/// The network daemon, as the job setup wrote starts it: on macOS with the uid and gid to drop to, elsewhere alone.
fn daemon(args: &[String]) -> Result<ExitCode, Error> {
    match args {
        [registry, uid, gid] => {
            let (Ok(uid), Ok(gid)) = (uid.parse(), gid.parse()) else {
                return Ok(usage());
            };
            doze_names::netd::run_translating(&PathBuf::from(registry), uid, gid)?;
        }
        [registry] => doze_names::netd::run_zone(&PathBuf::from(registry))?,
        _ => return Ok(usage()),
    }
    Ok(ExitCode::SUCCESS)
}

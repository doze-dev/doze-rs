//! Serving: take the data directory, listen, say so, run until stopped, and tidy up.

use std::path::PathBuf;
use std::time::Instant;

use doze_runtime::{DataDirLock, Tasks};

use crate::config::Config;
use crate::instance::{self, Instance, endpoint_of};
use crate::services::Svc;
use crate::style::{Stream, Style};

/// Serves until Ctrl-C or SIGTERM; a start that fails is told plainly (it says what to do).
pub(crate) fn main(cfg: &Config, started: Instant) -> i32 {
    let colour = std::io::IsTerminal::is_terminal(&std::io::stderr())
        && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty());
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(colour)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .thread_name("doze-aws")
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("doze-aws: {e}");
            return 1;
        }
    };
    match runtime.block_on(run(cfg, started)) {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("\n{message}\n");
            1
        }
    }
}

/// Whom outbound https calls trust: the OS store, `--ca-file`, or (said loudly) nothing is verified.
fn outbound_tls(cfg: &Config) -> Result<(), String> {
    let ca = cfg.text("ca-file");
    let ca = (!ca.is_empty()).then(|| PathBuf::from(&ca));
    let trust = aws_webhook::Trust::new(ca.as_deref(), cfg.switch("insecure-outbound-tls"))?;
    if trust.insecure {
        tracing::warn!(
            "--insecure-outbound-tls: the certificates of outbound https calls (SNS https subscriptions, API destinations, API Gateway HTTP integrations, JWT issuers) are NOT verified. Use this only with local servers; --ca-file <PEM> trusts a self-signed certificate without turning verification off"
        );
    }
    if let Some(file) = &ca {
        tracing::info!(
            "outbound https trusts the OS trust store and {} certificate(s) of {}",
            trust.extra.len(),
            file.display()
        );
    }
    aws_webhook::configure(trust);
    Ok(())
}

async fn run(cfg: &Config, started: Instant) -> Result<(), String> {
    let data = cfg.data_dir();
    let _lock = match DataDirLock::take(&data, "doze-aws") {
        Ok(lock) => lock,
        Err(doze_runtime::LockError::Held { .. }) => {
            return Err(crate::startfail::data_dir_held(&data));
        }
        Err(e) => return Err(format!("doze-aws cannot use its data directory: {e}")),
    };
    outbound_tls(cfg)?;
    let on = crate::services::parse_list(&cfg.list("services"))?;
    let listen: std::net::SocketAddr = cfg
        .text("listen")
        .parse()
        .map_err(|e| format!("--listen: {e}"))?;
    let mut me = Instance {
        pid: std::process::id(),
        region: cfg.text("region"),
        account: cfg.text("account"),
        auth: cfg.text("auth"),
        services: on.iter().map(|s| s.name.to_owned()).collect(),
        version: crate::VERSION.to_owned(),
        started: unix_secs(),
        data_dir: data.display().to_string(),
        ..Instance::default()
    };
    let console = crate::console::plan(cfg).await?;
    let mut gw = aws_gateway::Config::new(listen);
    gw.account_id = cfg.text("account");
    gw.default_region = cfg.text("region");
    gw.auth = crate::wire::auth_of(cfg);
    gw.known_keys = crate::wire::configured_keys(cfg);
    // A verifying gateway serves an unsigned S3 request as the anonymous principal only when IAM is there to judge it.
    gw.anonymous = cfg.text("iam") != "off";
    gw.health = Some(me.health());
    // The Lab's rules, when there is a console to make them from and `--lab` allows them.
    gw.faults = console.as_ref().and_then(crate::console::Planned::faults);
    let tasks = Tasks::new();
    // With the console on, its Live record is told about every call (wire::live_on_the_bus).
    let live = console.as_ref().map(crate::console::Planned::recorder);
    // The bus between services; the console's Resources space calls on it too.
    let peers = aws_peers::Peers::new();
    let (gateway, resumable, engine, apis) =
        crate::wire::gateway(cfg, (gw, &peers), &on, &tasks, live);
    let running = match gateway.bind().await {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            return Err(crate::startfail::port_taken(listen));
        }
        Err(e) => return Err(format!("doze-aws cannot listen on {listen}: {e}")),
    };
    me.endpoint = endpoint_of(running.local_addr());
    resumable.resume(&me.endpoint).await;
    me.advertised = advertised(cfg, "advertise")?;
    // Refused here, with the gateway's, not found out when the console starts.
    advertised(cfg, "advertise-console")?;
    // The line a supervisor (and the conformance harness) waits for.
    tracing::info!("doze-aws listening on {}", running.local_addr());
    write_instance(cfg, &data, &mut me);
    let console = console
        .map(|c| c.start(cfg, &me, (&on, peers, engine), apis))
        .transpose()?;
    let url = console.as_ref().map(aws_console::Running::url);
    announce(cfg, &me, &on, started, url.as_deref());
    mention_size(&data, cfg.count("data-warn-gb"), &tasks);
    wait_for_stop().await;
    tracing::info!("stopping");
    crate::console::stop(console).await;
    running.shutdown().await;
    tasks.shutdown().await;
    instance::remove(&data);
    Ok(())
}

/// The instance file `doze-aws env` finds this process by (with the key, under `--auth verify`).
fn write_instance(cfg: &Config, data: &std::path::Path, me: &mut Instance) {
    if cfg.text("auth") == "verify" {
        (me.access_key, me.secret_key) =
            (Some(cfg.text("access-key")), Some(cfg.text("secret-key")));
    }
    if let Err(e) = instance::write(data, me) {
        tracing::warn!(
            "{}: {e} (`doze-aws env` will not find this instance by its data directory)",
            instance::FILE
        );
    }
}

/// The URL `--advertise` (or `--advertise-console`) names (without a trailing slash), empty when none is given.
fn advertised(cfg: &Config, key: &str) -> Result<String, String> {
    let given = cfg.text(key);
    let url = given.trim().trim_end_matches('/');
    if !url.is_empty() && !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(format!(
            "--{key} {given:?} is not an http:// or https:// address"
        ));
    }
    Ok(url.to_owned())
}

/// The ready block, then the template `--template` names, applied beside the server.
fn announce(
    cfg: &Config,
    me: &Instance,
    on: &[&'static Svc],
    started: Instant,
    console: Option<&str>,
) {
    let ready = crate::ready::Ready {
        endpoint: me.public_endpoint(),
        console,
        services: on,
        region: &me.region,
        account: &me.account,
        took: started.elapsed(),
        config: cfg,
        shell: crate::env::Shell::pick(None),
    };
    print!("{}", ready.render(Style::of(Stream::Out)));
    let template = cfg.text("template");
    if template.is_empty() {
        return;
    }
    let client = me.client();
    let cwd = std::env::current_dir().unwrap_or_default();
    let plan = crate::apply::Plan {
        file: PathBuf::from(template),
        stack: crate::apply::stack_name(&cwd),
        params: Vec::new(),
    };
    // A blocking client on its own thread: the server's one thread keeps serving the calls it makes. A failed apply is reported
    // and the server stays up, so the template can be fixed and applied again with `doze-aws apply`.
    let spawned = std::thread::Builder::new()
        .name("doze-aws-apply".into())
        .spawn(move || match client {
            Ok(c) => {
                let _ =
                    crate::apply::apply(&crate::cfn::Cfn::new(c), &plan, Style::of(Stream::Out));
            }
            Err(e) => eprintln!("doze-aws: --template: {e}"),
        });
    if let Err(e) = spawned {
        tracing::warn!("--template was not applied: {e}");
    }
}

/// Once, after the ready block, a little at a time on the serving thread: when the data directory has grown past `--data-warn-gb`, say so and say what
/// shows where it went. A person whose disk filled should not have to know where to look.
fn mention_size(data: &std::path::Path, warn_gb: u64, tasks: &Tasks) {
    if warn_gb == 0 {
        return;
    }
    let dir = data.to_path_buf();
    // WHY: on the serving thread, a directory at a time with a yield between, not on a thread of its own: glibc keeps a
    // finished thread's malloc arena, which cost 210 KB of idle memory for the rest of the process (CI weight, 2026-11-15).
    tasks.spawn(async move {
        let kept = crate::data::dir_bytes_yielding(&dir).await;
        if warn_gb > 0 && kept >= warn_gb << 30 {
            let shown =
                crate::config::display_path(&dir, &std::env::current_dir().unwrap_or_default());
            println!(
                "    {}  doze-aws keeps {} in {shown}: `doze-aws data` shows where it goes\n",
                Style::of(Stream::Out).yellow("note     "),
                crate::data_show::size(kept)
            );
        }
    });
}

fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Resolves on Ctrl-C or, on Unix, SIGTERM.
async fn wait_for_stop() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        if let Ok(mut term) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
            return;
        }
    }
    // WHY: with no way to wait for a signal the process runs until it is killed.
    if tokio::signal::ctrl_c().await.is_err() {
        std::future::pending::<()>().await;
    }
}

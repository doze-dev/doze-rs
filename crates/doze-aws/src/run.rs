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
    let mut gw = aws_gateway::Config::new(listen);
    gw.account_id = cfg.text("account");
    gw.default_region = cfg.text("region");
    gw.auth = crate::wire::auth_of(cfg);
    gw.health = Some(me.health());
    let tasks = Tasks::new();
    let (gateway, resumable) = crate::wire::gateway(cfg, gw, &on, &tasks);
    let running = match gateway.bind().await {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            return Err(crate::startfail::port_taken(listen));
        }
        Err(e) => return Err(format!("doze-aws cannot listen on {listen}: {e}")),
    };
    me.endpoint = endpoint_of(running.local_addr());
    resumable.resume(&me.endpoint).await;
    // The line a supervisor (and the conformance harness) waits for.
    tracing::info!("doze-aws listening on {}", running.local_addr());
    if cfg.text("auth") == "verify" {
        (me.access_key, me.secret_key) =
            (Some(cfg.text("access-key")), Some(cfg.text("secret-key")));
    }
    if let Err(e) = instance::write(&data, &me) {
        tracing::warn!(
            "{}: {e} (`doze-aws env` will not find this instance by its data directory)",
            instance::FILE
        );
    }
    announce(cfg, &me, &on, started);
    wait_for_stop().await;
    tracing::info!("stopping");
    running.shutdown().await;
    tasks.shutdown().await;
    instance::remove(&data);
    Ok(())
}

/// The ready block, then the template `--template` names, applied beside the server.
fn announce(cfg: &Config, me: &Instance, on: &[&'static Svc], started: Instant) {
    let ready = crate::ready::Ready {
        endpoint: &me.endpoint,
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

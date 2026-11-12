//! Wiring: the services, the gateway, and the stop.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use aws_auth::{Credential, Secret, StaticCredentials};
use aws_gateway::{Auth, Config, Gateway};
use doze_runtime::{Ctx, DataDirLock, SystemClock, Tasks};

use crate::cli::{Args, AuthMode};

pub(crate) async fn run(args: Args) -> Result<()> {
    let _lock = DataDirLock::take(&args.data_dir, "doze-aws")?;
    let tasks = Tasks::new();
    let ctx = |name: &str| Ctx {
        dir: args.data_dir.join(name),
        idle: Duration::from_secs(args.idle_secs),
        tasks: tasks.clone(),
        clock: Arc::new(SystemClock),
    };
    let mut config = Config::new(args.listen);
    config.account_id.clone_from(&args.account);
    config.auth = match args.auth {
        AuthMode::Trust => Auth::Trust,
        AuthMode::Verify => {
            let mut creds = StaticCredentials::new();
            creds.insert(
                args.access_key.clone(),
                Credential {
                    secret: Secret::new(args.secret_key.clone()),
                    session_token: None,
                    expires_at: None,
                },
            );
            Auth::Verify(Arc::new(creds))
        }
    };
    let running = Gateway::new(config)
        .with(Arc::new(aws_sqs::Sqs::new(Arc::new(ctx("sqs")))))
        .bind()
        .await
        .with_context(|| format!("listening on {}", args.listen))?;
    // The line a supervisor (and the conformance harness) waits for.
    tracing::info!("doze-aws listening on {}", running.local_addr());
    wait_for_stop().await;
    tracing::info!("stopping");
    running.shutdown().await;
    tasks.shutdown().await;
    Ok(())
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

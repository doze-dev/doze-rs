//! Wiring: the services, the gateway, and the stop.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use aws_auth::{Credential, Secret, StaticCredentials};
use aws_gateway::{Auth, Config, Gateway};
use aws_peers::Peers;
use aws_wire::service::Service;
use doze_runtime::{Ctx, DataDirLock, SystemClock, Tasks};

use crate::cli::{Args, AuthMode};

pub(crate) async fn run(args: Args) -> Result<()> {
    let _lock = DataDirLock::take(&args.data_dir, "doze-aws")?;
    let tasks = Tasks::new();
    // The bus the services call each other over; each is registered on it as well as on the gateway.
    let peers = Peers::new();
    let ext: Arc<dyn std::any::Any + Send + Sync> = peers.clone();
    let ctx = |name: &str| Ctx {
        dir: args.data_dir.join(name),
        idle: Duration::from_secs(args.idle_secs),
        tasks: tasks.clone(),
        clock: Arc::new(SystemClock),
        ext: Some(Arc::clone(&ext)),
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
    peers.set_mode(args.iam.into());
    let (iam, sts) = aws_iam::services(Arc::new(ctx("iam")));
    let iam = Arc::new(iam);
    peers.set_authorizer(iam.clone());
    // Under `--auth verify`, a key the command line does not name may be one IAM issued (an access key or a role session).
    config.credential_source = Some(iam.clone());
    let cloudwatch = Arc::new(aws_cloudwatch::Cloudwatch::new(Arc::new(ctx("cloudwatch"))));
    // Lambda wakes its event source pollers from the sources themselves, so it is handed them (the bus does not carry a wake-up).
    let sqs = Arc::new(aws_sqs::Sqs::new(Arc::new(ctx("sqs"))));
    let dynamodb = Arc::new(aws_dynamodb::Dynamodb::new(Arc::new(ctx("dynamodb"))));
    let kinesis = Arc::new(aws_kinesis::Kinesis::new(Arc::new(ctx("kinesis"))));
    let lambda = Arc::new(aws_lambda::Lambda::with_config(Arc::new(ctx("lambda")), lambda_config(&args)));
    lambda.connect(aws_lambda::Sources {
        sqs: Arc::clone(&sqs),
        kinesis: Arc::clone(&kinesis),
        dynamodb: Arc::clone(&dynamodb),
    });
    let services: Vec<Arc<dyn Service>> = vec![
        iam,
        Arc::new(sts),
        sqs,
        Arc::new(aws_ssm::Ssm::new(Arc::new(ctx("ssm")))),
        Arc::new(aws_secrets::Secrets::new(Arc::new(ctx("secretsmanager")))),
        Arc::new(aws_kms::Kms::new(Arc::new(ctx("kms")))),
        Arc::new(aws_sns::Sns::new(Arc::new(ctx("sns")))),
        dynamodb,
        Arc::new(aws_s3::S3::new(Arc::new(ctx("s3")))),
        kinesis,
        Arc::new(aws_logs::Logs::new(Arc::new(ctx("logs")))),
        cloudwatch.clone(),
        lambda.clone(),
    ];
    let mut gateway = Gateway::new(config);
    for service in services {
        peers.register(Arc::clone(&service));
        gateway = gateway.with(service);
    }
    let running = gateway
        .bind()
        .await
        .with_context(|| format!("listening on {}", args.listen))?;
    // Alarms that were pending when the process last stopped are judged again; nothing else would wake their region.
    cloudwatch.resume().await;
    // Event source mappings that were enabled when the process last stopped poll again; functions find the other services here.
    lambda.set_endpoint(&format!("http://{}", running.local_addr()));
    lambda.resume().await;
    // The line a supervisor (and the conformance harness) waits for.
    tracing::info!("doze-aws listening on {}", running.local_addr());
    wait_for_stop().await;
    tracing::info!("stopping");
    running.shutdown().await;
    tasks.shutdown().await;
    Ok(())
}

/// How Lambda runs functions: the idle time and quiet flag from the command line, and the interpreters from `DOZE_LAMBDA_PYTHON`,
/// `_NODE`, `_RUBY`, `_JAVA` and `_DOTNET` (the one on `PATH` otherwise).
fn lambda_config(args: &Args) -> aws_lambda::Config {
    let env = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    aws_lambda::Config {
        runtime: aws_lambda::RuntimeConfig {
            idle: Duration::from_secs(args.lambda_idle_secs),
            interpreters: aws_lambda::Interpreters {
                python: env("DOZE_LAMBDA_PYTHON"),
                node: env("DOZE_LAMBDA_NODE"),
                ruby: env("DOZE_LAMBDA_RUBY"),
                java: env("DOZE_LAMBDA_JAVA"),
                dotnet: env("DOZE_LAMBDA_DOTNET"),
            },
        },
        quiet: args.lambda_quiet,
    }
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

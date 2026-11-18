//! Wiring: every service, made and connected, on the gateway and the peer bus. A service left out of `--services` is still made
//! (it is lazy: nothing opens until a call) and on the bus, but the gateway puts it behind [`Off`], which answers every call from
//! outside with how to turn it on.

use std::sync::Arc;
use std::time::Duration;

use aws_auth::{Credential, Secret, StaticCredentials};
use aws_gateway::{Auth, Config as GatewayConfig, Gateway};
use aws_peers::Peers;
use aws_wire::service::Service;
use doze_runtime::{Ctx, SystemClock, Tasks};

use crate::config::Config;
use crate::services::{Off, Svc};

/// The services that need waking after a restart (what was pending when the process stopped carries on).
pub(crate) struct Resumable {
    cloudwatch: Option<Arc<aws_cloudwatch::Cloudwatch>>,
    eventbridge: Option<Arc<aws_eventbridge::Eventbridge>>,
    scheduler: Option<Arc<aws_scheduler::Scheduler>>,
    secrets: Option<Arc<aws_secrets::Secrets>>,
    dynamodb: Option<Arc<aws_dynamodb::Dynamodb>>,
    lambda: Option<Arc<aws_lambda::Lambda>>,
    states: Option<Arc<aws_sfn::Sfn>>,
}

impl Resumable {
    /// Wakes what was pending, in the services that are served. Lambda is told where the gateway is first.
    pub(crate) async fn resume(&self, endpoint: &str) {
        if let Some(s) = &self.cloudwatch {
            s.resume().await;
        }
        if let Some(s) = &self.eventbridge {
            s.resume().await;
        }
        if let Some(s) = &self.scheduler {
            s.resume().await;
        }
        // Scheduled secret rotations: what came due while stopped starts now, and the next window is waited for.
        if let Some(s) = &self.secrets {
            s.resume().await;
        }
        if let Some(s) = &self.dynamodb {
            s.resume().await;
        }
        if let Some(s) = &self.lambda {
            s.set_endpoint(endpoint);
            s.resume().await;
        }
        if let Some(s) = &self.states {
            s.resume().await;
        }
    }
}

/// Which services are served.
struct On<'a>(&'a [&'static Svc]);

impl On<'_> {
    fn has(&self, name: &str) -> bool {
        self.0.iter().any(|s| s.name == name)
    }

    /// `service` as it is served: itself, or [`Off`].
    fn wrap(&self, name: &str, service: Arc<dyn Service>) -> Arc<dyn Service> {
        match crate::services::find(name) {
            Some(svc) if !self.has(name) => Arc::new(Off::new(service, svc, self.0)),
            _ => service,
        }
    }

    fn keep<T>(&self, name: &str, service: &Arc<T>) -> Option<Arc<T>> {
        self.has(name).then(|| Arc::clone(service))
    }
}

/// The gateway with every service on it, the services to resume once it listens, and the data engine over all of them.
pub(crate) fn gateway(
    cfg: &Config,
    gw: GatewayConfig,
    on: &[&'static Svc],
    tasks: &Tasks,
) -> (Gateway, Resumable, Arc<crate::data::Data>) {
    let on = On(on);
    let peers = Peers::new();
    let ext: Arc<dyn std::any::Any + Send + Sync> = peers.clone();
    let idle = Duration::from_secs(cfg.count("idle-secs"));
    let data = cfg.data_dir();
    let ctx = |name: &str| {
        Arc::new(Ctx {
            dir: data.join(name),
            idle,
            tasks: tasks.clone(),
            clock: Arc::new(SystemClock),
            ext: Some(Arc::clone(&ext)),
        })
    };
    let mut gw = gw;
    let hook = data_hook(cfg, &mut gw);
    peers.set_mode(iam_mode(cfg));
    let (iam, sts) = aws_iam::services(ctx("iam"));
    let iam = Arc::new(iam);
    // IAM decides for every service whether or not IAM's own API is served.
    peers.set_authorizer(iam.clone());
    // A key the command line does not name may be one IAM issued (an access key or a role session): under `--auth verify` for every
    // call, and under trust for a function URL with AWS_IAM.
    gw.credential_source = Some(iam.clone());
    let (cloudwatch, logs) = observability(&ctx, cfg);
    let eventbridge = Arc::new(aws_eventbridge::Eventbridge::new(ctx("eventbridge")));
    let secrets = Arc::new(aws_secrets::Secrets::new(ctx("secretsmanager")));
    let scheduler = Arc::new(aws_scheduler::Scheduler::new(ctx("scheduler")));
    let states = Arc::new(aws_sfn::Sfn::with_config(ctx("states"), states_config()));
    let (sqs, dynamodb, kinesis, lambda) = compute(&ctx, cfg);
    let cloudformation = aws_cloudformation::Cloudformation::with_config(
        ctx("cloudformation"),
        cloudformation_config(cfg),
    );
    let services: Vec<(&str, Arc<dyn Service>)> = vec![
        ("iam", iam),
        ("sts", Arc::new(sts)),
        ("sqs", sqs),
        ("ssm", Arc::new(aws_ssm::Ssm::new(ctx("ssm")))),
        ("secretsmanager", secrets.clone()),
        ("kms", Arc::new(aws_kms::Kms::new(ctx("kms")))),
        ("sns", Arc::new(aws_sns::Sns::new(ctx("sns")))),
        ("dynamodb", dynamodb.clone()),
        ("s3", Arc::new(aws_s3::S3::new(ctx("s3")))),
        ("kinesis", kinesis),
        ("logs", logs),
        ("cloudwatch", cloudwatch.clone()),
        ("eventbridge", eventbridge.clone()),
        ("scheduler", scheduler.clone()),
        ("lambda", lambda.clone()),
        ("stepfunctions", states.clone()),
        ("cloudformation", Arc::new(cloudformation)),
    ];
    // The data engine sees every service, served or not: what one keeps on disk is the person's whatever `--services` says today.
    let mut kept = catalogued(&services);
    let mut gateway = Gateway::new(gw);
    for (name, service) in services {
        // The bus carries the service itself: one that is served leans on the others (Lambda writes to CloudWatch Logs) whether or
        // not they are served.
        peers.register(Arc::clone(&service));
        gateway = gateway.with(on.wrap(name, service));
    }
    let (gateway, apis) = paired(gateway, &on, &peers, &dynamodb, ctx("apigateway"));
    kept.extend(catalogued(&[("apigateway", apis)]));
    let data = Arc::new(crate::data::Data::new(data.clone(), kept));
    hook.set(Arc::clone(&data));
    let resumable = Resumable {
        cloudwatch: on.keep("cloudwatch", &cloudwatch),
        eventbridge: on.keep("eventbridge", &eventbridge),
        scheduler: on.keep("scheduler", &scheduler),
        secrets: on.keep("secretsmanager", &secrets),
        dynamodb: on.keep("dynamodb", &dynamodb),
        lambda: on.keep("lambda", &lambda),
        states: on.keep("stepfunctions", &states),
    };
    (gateway, resumable, data)
}

/// The services that share a signing name with another, which the bus cannot key by it.
fn paired(
    gateway: Gateway,
    on: &On<'_>,
    peers: &Arc<Peers>,
    dynamodb: &Arc<aws_dynamodb::Dynamodb>,
    apigateway: Arc<Ctx>,
) -> (Gateway, Arc<dyn Service>) {
    // DynamoDB Streams signs as `dynamodb` too: the gateway routes it by its target prefix, and it is not on the peer bus.
    let streams = aws_dynamodb_streams::Streams::new(dynamodb.clone(), Some(peers.clone()));
    let gateway = gateway.with(on.wrap("dynamodb", Arc::new(streams)));
    // API Gateway's two APIs sign alike (`apigateway`); the gateway tells them apart by path, the bus by name.
    let (rest_apis, http_apis) = aws_apigateway::services(apigateway);
    // The `@connections` management API signs as `execute-api`, a name of its own.
    let management: Arc<dyn Service> = Arc::new(rest_apis.management());
    peers.register(Arc::clone(&management));
    let gateway = gateway.with(on.wrap("apigateway", management));
    let (rest_apis, http_apis): (Arc<dyn Service>, Arc<dyn Service>) =
        (Arc::new(rest_apis), Arc::new(http_apis));
    peers.register_as("apigateway", Arc::clone(&rest_apis));
    peers.register_as("apigatewayv2", Arc::clone(&http_apis));
    // Both APIs keep their files in one directory; whichever answers for its data answers for both.
    let data = if http_apis.storage().is_some() {
        Arc::clone(&http_apis)
    } else {
        Arc::clone(&rest_apis)
    };
    let gateway = gateway
        .with(on.wrap("apigateway", rest_apis))
        .with(on.wrap("apigateway", http_apis));
    (gateway, data)
}

fn iam_mode(cfg: &Config) -> aws_peers::Mode {
    match cfg.text("iam").as_str() {
        "off" => aws_peers::Mode::Off,
        "enforce" => aws_peers::Mode::Enforce,
        _ => aws_peers::Mode::Soft,
    }
}

/// Who may call: anyone (trust), or the one key from the configuration (verify; IAM's own keys are added by the credential source).
pub(crate) fn auth_of(cfg: &Config) -> Auth {
    if cfg.text("auth") != "verify" {
        return Auth::Trust;
    }
    Auth::Verify(configured_keys(cfg))
}

/// The key the configuration names. Under trust it is still known: a function URL with `AWS_IAM` signed with it is checked.
pub(crate) fn configured_keys(cfg: &Config) -> Arc<StaticCredentials> {
    let mut creds = StaticCredentials::new();
    creds.insert(
        cfg.text("access-key"),
        Credential {
            secret: Secret::new(cfg.text("secret-key")),
            session_token: None,
            expires_at: None,
        },
    );
    Arc::new(creds)
}

fn cloudformation_config(cfg: &Config) -> aws_cloudformation::Config {
    use aws_cloudformation::Unsupported;
    let mut overrides: Vec<(String, Unsupported)> = cfg
        .list("cfn-skip")
        .into_iter()
        .map(|t| (t, Unsupported::Skip))
        .collect();
    overrides.extend(
        cfg.list("cfn-fail")
            .into_iter()
            .map(|t| (t, Unsupported::Fail)),
    );
    aws_cloudformation::Config {
        unsupported: if cfg.text("cfn-unsupported") == "fail" {
            Unsupported::Fail
        } else {
            Unsupported::Skip
        },
        overrides,
        logical_names: cfg.switch("cfn-logical-names"),
    }
}

/// CloudWatch and Logs with their local storage bounds.
fn observability(
    ctx: &impl Fn(&str) -> Arc<Ctx>,
    cfg: &Config,
) -> (Arc<aws_cloudwatch::Cloudwatch>, Arc<dyn Service>) {
    let cloudwatch = Arc::new(aws_cloudwatch::Cloudwatch::with_settings(
        ctx("cloudwatch"),
        aws_cloudwatch::Settings {
            retention: Duration::from_secs(cfg.count("cloudwatch-retention-hours") * 3600),
        },
    ));
    let days = cfg.count("logs-retention-days");
    let settings = aws_logs::Settings {
        retention: (days > 0).then(|| Duration::from_secs(days * 86_400)),
        max_events: cfg.count("logs-max-events"),
    };
    let logs = Arc::new(aws_logs::Logs::with_settings(ctx("logs"), settings));
    (cloudwatch, logs)
}

/// SQS, DynamoDB, Kinesis and Lambda, made together: Lambda wakes its event source pollers from the sources themselves, so it is
/// handed them (the peer bus does not carry a wake-up).
fn compute(
    ctx: &impl Fn(&str) -> Arc<Ctx>,
    cfg: &Config,
) -> (
    Arc<aws_sqs::Sqs>,
    Arc<aws_dynamodb::Dynamodb>,
    Arc<aws_kinesis::Kinesis>,
    Arc<aws_lambda::Lambda>,
) {
    let sqs = Arc::new(aws_sqs::Sqs::with_settings(
        ctx("sqs"),
        cfg.switch("sqs-skip-60s-waits"),
    ));
    let dynamodb = Arc::new(aws_dynamodb::Dynamodb::with_windows(
        ctx("dynamodb"),
        dynamodb_windows(cfg),
    ));
    let kinesis = Arc::new(aws_kinesis::Kinesis::new(ctx("kinesis")));
    let lambda = Arc::new(aws_lambda::Lambda::with_config(
        ctx("lambda"),
        lambda_config(cfg),
    ));
    lambda.connect(aws_lambda::Sources {
        sqs: Arc::clone(&sqs),
        kinesis: Arc::clone(&kinesis),
        dynamodb: Arc::clone(&dynamodb),
    });
    (sqs, dynamodb, kinesis, lambda)
}

/// DynamoDB's transitional windows: each setting, else its older environment variable (`DOZE_DDB_CREATE_MS` ...), else 0.
fn dynamodb_windows(cfg: &Config) -> aws_dynamodb::Windows {
    let pick = |name: &str, old: &str| {
        let set = cfg.count(name);
        let ms = if set > 0 {
            set
        } else {
            std::env::var(old)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0)
        };
        i64::try_from(ms).unwrap_or(i64::MAX)
    };
    aws_dynamodb::Windows {
        create_ms: pick("dynamodb-create-ms", "DOZE_DDB_CREATE_MS"),
        update_ms: pick("dynamodb-update-ms", "DOZE_DDB_UPDATE_MS"),
        delete_ms: pick("dynamodb-delete-ms", "DOZE_DDB_DELETE_MS"),
    }
}

/// How Lambda runs functions: the idle time and quiet switch, and the interpreters from `DOZE_LAMBDA_PYTHON`, `_NODE`, `_RUBY`,
/// `_JAVA` and `_DOTNET` (the one on `PATH` otherwise).
fn lambda_config(cfg: &Config) -> aws_lambda::Config {
    let env = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    aws_lambda::Config {
        runtime: aws_lambda::RuntimeConfig {
            idle: Duration::from_secs(cfg.count("lambda-idle-secs")),
            interpreters: aws_lambda::Interpreters {
                python: env("DOZE_LAMBDA_PYTHON"),
                node: env("DOZE_LAMBDA_NODE"),
                ruby: env("DOZE_LAMBDA_RUBY"),
                java: env("DOZE_LAMBDA_JAVA"),
                dotnet: env("DOZE_LAMBDA_DOTNET"),
            },
        },
        quiet: cfg.switch("lambda-quiet"),
        // Not AWS behaviour: every durable execution wait and step retry delay multiplied by it (as `DOZE_SFN_WAIT_SCALE` is for
        // Step Functions).
        durable_wait_scale: env("DOZE_LAMBDA_DURABLE_WAIT_SCALE")
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|s| s.is_finite())
            .map(|s| s.max(0.0)),
    }
}

/// Step Functions' local-testing settings, from the environment: `DOZE_SFN_WAIT_SCALE` (every Wait and retry interval multiplied by
/// it) and `SFN_MOCK_CONFIG` (Step Functions Local's mock configuration file; `DOZE_SFN_MOCK_CONFIG` also works).
fn states_config() -> aws_sfn::Config {
    let env = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    let mut config = aws_sfn::Config::default();
    if let Some(scale) = env("DOZE_SFN_WAIT_SCALE").and_then(|s| s.parse::<f64>().ok()) {
        config.wait_scale = scale.max(0.0);
    }
    if let Some(path) = env("DOZE_SFN_MOCK_CONFIG").or_else(|| env("SFN_MOCK_CONFIG")) {
        match aws_sfn::Config::read_mocks(std::path::Path::new(&path)) {
            Ok(mocks) => config.mocks = Some(mocks),
            Err(e) => tracing::warn!("step functions: the mock configuration is not used: {e}"),
        }
    }
    config
}

/// The `/_doze/data` hook, on the gateway's settings; under `--auth verify` it answers only a request carrying the secret key.
fn data_hook(cfg: &Config, gw: &mut GatewayConfig) -> Arc<crate::data_web::DataHook> {
    let key = (cfg.text("auth") == "verify").then(|| cfg.text("secret-key"));
    // The services' own local addresses (`/_doze/iam/...`, `/_doze/sqs/...`) ask for the same key.
    gw.local_key.clone_from(&key);
    let hook = Arc::new(crate::data_web::DataHook::new(key));
    gw.hooks.0.push(hook.clone());
    hook
}

/// Each service with its catalogue entry.
fn catalogued(services: &[(&str, Arc<dyn Service>)]) -> Vec<(&'static Svc, Arc<dyn Service>)> {
    services
        .iter()
        .filter_map(|(name, s)| Some((crate::services::find(name)?, Arc::clone(s))))
        .collect()
}

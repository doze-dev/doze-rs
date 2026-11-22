//! The doze module face of doze-aws (docs/port/modules/PLAN.md step 15, core plan M3): the same binary, started by doze
//! as its aws module, speaks module protocol 2 and plans itself as the engine.
//!
//! It is the module when it runs under the name a module release packages it as (`aws-plugin`, how doze starts it,
//! with no arguments or only `--describe`), or as `doze-aws module`. Anything else is doze-aws, as before; the plan
//! starts the binary with its own flags, so an `aws-plugin` with arguments is the server.
//!
//! What it does, against the Go module it replaces (`doze-aws/module`): the same block, argument for argument
//! (`spec.rs`), the same variable (`AWS_ENDPOINT_URL`), one instance for the whole local AWS. Better than Go, each
//! tested:
//! - what the stack would refuse is refused before boot at its line (`check.rs`): references to blocks nobody
//!   declared, malformed table keys and JSON, a function's missing code directory, the wrong number of targets;
//! - converge keeps the declared resources as one CloudFormation stack on the instance (`converge.rs`): a changed
//!   argument is updated in place, where Go only created what was missing, and a failure rolls back whole, naming
//!   what failed; a removed block leaves its resource and data where they are, as Go did;
//! - the resources are the instance's objects, so `doze plan` shows them;
//! - the shell client (`aws`, pointed at the instance) and `doze init`'s detection are declared.

mod check;
mod check_more;
mod converge;
mod godata;
mod spec;
mod template;
mod template_more;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use doze_module::proto::msg::error::{Diagnostic, Error, ErrorKind};
use doze_module::proto::msg::hello::{ClientCommand, Detect};
use doze_module::proto::msg::instance::{Address, Connection, Instance, Listener, Toolchain};
use doze_module::proto::msg::run::{Pin, Plan, Process, Provisioned, Ready, Resolved, StopHow};
use doze_module::proto::msg::value::Value;
use doze_module::{About, Cx, Module, Result, help, serve};

pub(crate) use spec::AwsSpec;

/// The name a module release gives the binary (`bin/<engine>-plugin`), which doze starts with no arguments.
const PLUGIN_SUFFIX: &str = "-plugin";
/// The Region of a block that sets none: where the module's own clients sign and the server answers.
pub(crate) const DEFAULT_REGION: &str = "us-east-1";

/// The published example: the Go module's, less its function and state machine (they need code beside doze.hcl).
const EXAMPLE: &str = r#"aws "local" {
  port = 4566

  bucket "uploads" { versioning = true }
  queue  "emails"  {
    dlq          = "auto"
    max_receives = 5
  }
  queue  "orders"  {
    fifo          = true
    content_dedup = true
  }

  topic "signups" {
    subscribe {
      queue = "emails"
      raw   = true
    }
  }

  table "sessions" {
    key = "session_id:S"
    ttl = "expires_at"
  }

  rule "order_placed" {
    pattern = "{\"source\":[\"shop.checkout\"]}"
    targets = ["queue:emails"]
  }

  key "app_key" {}
  secret "db_password" { value = "s3cr3t" }
  parameter "/app/feature/banner" { value = "hello" }
}
"#;

/// Whether this process was started as the module (see the module docs).
pub(crate) fn invoked() -> bool {
    let args: Vec<OsString> = std::env::args_os().collect();
    is_module(&args)
}

fn is_module(args: &[OsString]) -> bool {
    let rest = args.get(1..).unwrap_or_default();
    if rest.first().is_some_and(|a| a == "module") {
        return true;
    }
    let plugin = args
        .first()
        .and_then(|a| Path::new(a).file_stem())
        .is_some_and(|s| s.to_string_lossy().ends_with(PLUGIN_SUFFIX));
    plugin && rest.iter().all(|a| a == "--describe")
}

/// Serves doze on the channel it was started with, until doze goes.
pub(crate) fn run() -> std::process::ExitCode {
    serve(Aws)
        .release(crate::VERSION)
        .converge()
        .objects()
        .attributes()
        .run()
}

struct Aws;

/// Whether the block declares no resource at all.
pub(crate) fn declared_nothing(s: &AwsSpec) -> bool {
    s.bucket.is_empty()
        && s.queue.is_empty()
        && s.topic.is_empty()
        && s.table.is_empty()
        && s.function.is_empty()
        && s.rule.is_empty()
        && s.key.is_empty()
        && s.secret.is_empty()
        && s.parameter.is_empty()
        && s.state_machine.is_empty()
}

/// The main backend's `host:port`: doze-aws listens on TCP only.
pub(crate) fn backend(instance: &Instance) -> Result<String> {
    match instance
        .listeners
        .get(doze_module::proto::network::MAIN)
        .map(|g| &g.backend)
    {
        Some(Address::Tcp(a)) => Ok(a.clone()),
        Some(Address::Unix(_)) => Err(Error::new(
            ErrorKind::Unsupported,
            "doze-aws cannot listen on a socket; the main listener needs a TCP backend",
        )),
        None => Err(Error::new(
            ErrorKind::Unavailable,
            "no backend granted for \"main\"",
        )),
    }
}

/// doze-aws's command line for an instance: where it listens and keeps its data, the Region it answers as.
pub(crate) fn server_args(instance: &Instance, region: &str) -> Result<Vec<String>> {
    Ok(vec![
        "--listen".into(),
        backend(instance)?,
        "--data-dir".into(),
        instance.data_dir.clone(),
        "--region".into(),
        region.into(),
    ])
}

impl Module for Aws {
    type Spec = AwsSpec;

    fn about(&self) -> About {
        let targets = [
            "aarch64-apple-darwin",
            "aarch64-unknown-linux-gnu",
            "x86_64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
            "aarch64-pc-windows-msvc",
        ];
        About {
            engine: "aws".into(),
            category: "cloud".into(),
            title: "AWS".into(),
            tagline: "The whole local AWS as one service.".into(),
            // Versionless, as Go's: the engine is this binary, and a block names no version.
            versions: vec![],
            exact_dots: 0,
            default_port: 4566,
            example: EXAMPLE.into(),
            targets: targets.map(String::from).to_vec(),
            description: Some(
                "Every doze-aws service (S3, SQS, SNS, DynamoDB, Lambda, EventBridge, Step Functions, Kinesis, API Gateway, \
                 CloudFormation, IAM, KMS, SSM, Secrets Manager, STS) behind one endpoint, as one doze instance. Declare \
                 buckets, queues, topics, tables, functions, rules, keys, secrets, parameters and state machines as nested \
                 blocks; doze keeps them as a CloudFormation stack on the instance. A stock SDK or the aws CLI points \
                 AWS_ENDPOINT_URL at the one endpoint. One process, no Docker, no JVM."
                    .into(),
            ),
            homepage: Some("https://github.com/doze-dev/doze-rs".into()),
            source: Some("doze/aws".into()),
            example_label: Some("local".into()),
            default_version: None,
            client: Some(client()),
            detect: Some(detect()),
            ..About::default()
        }
    }

    fn check(&self, spec: &AwsSpec, instance: &Instance) -> Vec<Diagnostic> {
        check::check(spec, &instance.dir)
    }

    fn network(&self, _cx: &Cx<AwsSpec>) -> Vec<Listener> {
        vec![Listener {
            name: doze_module::proto::network::MAIN.into(),
            port: 0,
            http: true,
            routes: vec![],
            tcp: true,
        }]
    }

    fn connection(&self, cx: &Cx<AwsSpec>) -> Result<Connection> {
        // A Region other than the default is what a client of this instance must be told; the default is what an SDK is
        // already left to find, and an app that sets its own is not overridden.
        let env = match cx.spec.region() {
            DEFAULT_REGION => BTreeMap::new(),
            region => ["AWS_REGION", "AWS_DEFAULT_REGION"]
                .map(|k| (k.to_owned(), region.to_owned()))
                .into(),
        };
        let addr = cx
            .instance
            .listeners
            .get(doze_module::proto::network::MAIN)
            .map_or_else(|| "127.0.0.1:4566".to_string(), |g| g.addr.clone());
        Ok(Connection {
            env_var: "AWS_ENDPOINT_URL".into(),
            url: format!("http://{addr}"),
            env,
        })
    }

    async fn resolve(&self, _cx: &Cx<AwsSpec>, _pins: &[Pin], _target: &str) -> Result<Resolved> {
        // Nothing to fetch: the engine is this binary.
        let exe = std::env::current_exe()
            .map_err(|e| Error::new(ErrorKind::Internal, format!("finding this binary: {e}")))?;
        let bin_dir = exe
            .parent()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(Resolved {
            toolchain: Toolchain {
                engine: "aws".into(),
                full: crate::VERSION.into(),
                bin_dir,
                tools: BTreeMap::new(),
            },
            pins: vec![],
        })
    }

    async fn provision(&self, cx: &Cx<AwsSpec>, _tc: &Toolchain) -> Result<Provisioned> {
        let made = help::empty_data_dir(&cx.instance.data_dir)?;
        // A folder the Go doze-aws module left: its files are set aside before this server first starts (godata.rs).
        match godata::set_aside(std::path::Path::new(&cx.instance.data_dir)) {
            Ok(true) => cx.log(
                2,
                format!(
                    "the Go doze-aws data is set aside in {}/, to be carried over at converge",
                    godata::KEPT
                ),
            ),
            Ok(false) => {}
            Err(why) => {
                return Err(Error::new(
                    ErrorKind::Internal,
                    format!("setting the Go doze-aws data aside: {why}"),
                ));
            }
        }
        Ok(made)
    }

    async fn plan(&self, cx: &Cx<AwsSpec>, _tc: &Toolchain) -> Result<Plan> {
        let args = server_args(&cx.instance, cx.spec.region())?;
        let addr = backend(&cx.instance)?;
        let program = std::env::current_exe()
            .map_err(|e| Error::new(ErrorKind::Internal, format!("finding this binary: {e}")))?;
        Ok(Plan {
            processes: vec![Process {
                name: cx.instance.name.clone(),
                program: program.to_string_lossy().into_owned(),
                args,
                env: BTreeMap::new(),
                dir: None,
                after: vec![],
                // The gateway answers its health path once every service is in place.
                ready: Some(Ready::Http(format!("http://{addr}/_doze/health"))),
                hooks: vec![],
                stop: Some(StopHow::Terminate),
            }],
        })
    }
}

impl doze_module::caps::Attributes for Aws {
    /// What other blocks read: `module.<name>.region`.
    fn attributes(&self, cx: &Cx<AwsSpec>) -> Result<BTreeMap<String, Value>> {
        Ok(BTreeMap::from([(
            "region".to_owned(),
            Value::String(cx.spec.region().to_owned()),
        )]))
    }
}

/// The aws CLI pointed at the instance, in its prompt mode, with the keys doze-aws takes.
fn client() -> ClientCommand {
    let env = [
        ("AWS_ENDPOINT_URL", "http://{host}:{port}"),
        ("AWS_ACCESS_KEY_ID", "test"),
        ("AWS_SECRET_ACCESS_KEY", "test"),
        // Filled in with the instance's Region (`region` in its block) when doze connects.
        ("AWS_DEFAULT_REGION", "{region}"),
    ];
    ClientCommand {
        program: "aws".into(),
        args: vec!["--cli-auto-prompt".into()],
        env: env.map(|(k, v)| (k.to_owned(), v.to_owned())).into(),
        fallbacks: vec![],
    }
}

/// How `doze init` recognises a project that uses AWS: the SDKs, LocalStack's images, the endpoint variables.
fn detect() -> Detect {
    let list = |v: &[&str]| v.iter().map(ToString::to_string).collect::<Vec<_>>();
    Detect {
        packages: BTreeMap::from([
            (
                "go".into(),
                list(&["github.com/aws/aws-sdk-go-v2", "github.com/aws/aws-sdk-go"]),
            ),
            (
                "npm".into(),
                list(&[
                    "aws-sdk",
                    "@aws-sdk/client-s3",
                    "@aws-sdk/client-sqs",
                    "@aws-sdk/client-dynamodb",
                ]),
            ),
            ("pypi".into(), list(&["boto3", "botocore"])),
            ("cargo".into(), list(&["aws-config", "aws-sdk-s3"])),
        ]),
        images: list(&[
            "localstack/localstack",
            "amazon/dynamodb-local",
            "softwaremill/elasticmq",
        ]),
        env: BTreeMap::from([
            ("AWS_ENDPOINT_URL".into(), "url".into()),
            ("AWS_ENDPOINT".into(), "url".into()),
            ("LOCALSTACK_HOST".into(), "address".into()),
        ]),
    }
}

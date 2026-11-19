//! The command engine's config: an app's body decoded (Go's `process.DecodeConfig`), a service's
//! and a task's built (`CommandConfig`), the port bound (`BindPort`) and the stack's defaults
//! applied (`ApplyDefaults`).

use crate::body::Body;
use crate::error::{At, Error, Sources, diag_error, pos_err};
use crate::gohcl::{Spec, Ty, decode_body, opt, req};
use crate::gotime;
use crate::model::{Command, CommandDefaults, Restart};
use crate::runspec;
use doze_hcl::decode::Schema;
use doze_hcl::diag::{Diagnostic, Diagnostics};
use doze_hcl::eval::Context;
use std::path::Path;
use std::time::Duration;

const DEFAULT_BACKOFF: Duration = Duration::from_secs(1);
const DEFAULT_MAX_RETRIES: i64 = 5;

pub static RESTART: Spec = Spec {
    attrs: &[
        opt("policy", Ty::Str),
        opt("backoff", Ty::Str),
        opt("max_retries", Ty::Int),
    ],
    blocks: &[],
    remain: false,
};

static APP: Spec = Spec {
    attrs: &[
        req("run", Ty::Str),
        opt("dir", Ty::Str),
        opt("env", Ty::StrMap),
        opt("env_file", Ty::Str),
        opt("ready", Ty::Any),
        opt("ready_timeout", Ty::Str),
        opt("setup", Ty::Any),
        opt("on_ready", Ty::Any),
        opt("stop", Ty::Any),
        opt("sleep", Ty::Str),
        opt("console", Ty::Str),
        opt("watch", Ty::StrList),
        opt("direct", Ty::Bool),
    ],
    blocks: &[("restart", &RESTART)],
    remain: false,
};

/// Arguments an app no longer takes, and what replaced them.
const REMOVED: [(&str, &str, bool, bool); 8] = [
    ("command", "`run`", false, false),
    ("cwd", "`dir`", false, false),
    (
        "forward",
        "nothing: an app with a `port` is named automatically",
        false,
        false,
    ),
    (
        "ingress",
        "nothing: an app with a `port` is named automatically",
        false,
        false,
    ),
    ("hooks", "`setup`, `on_ready` and `stop`", true, false),
    (
        "health",
        "`ready`, e.g. ready = http(\"/healthz\")",
        true,
        false,
    ),
    ("front", "it was never acted on; delete it", false, true),
    ("logs", "it was never acted on; delete it", false, true),
];

fn check_removed(body: &Body<'_>) -> Option<Diagnostic> {
    let schema = REMOVED
        .iter()
        .fold(Schema::default(), |s, (n, _, block, _)| {
            if *block {
                s.block(n, &[])
            } else {
                s.attr(n, false)
            }
        });
    let (c, _, _) = body.partial_content(&schema);
    for (name, now, _, gone) in REMOVED {
        if let Some(a) = c.attr(name) {
            let summary = if gone {
                format!("`{name}` is no longer an argument: {now}")
            } else {
                format!("`{name}` is no longer an argument; use {now}")
            };
            return Some(Diagnostic::error(summary, "", a.name_range).in_file(a.file));
        }
        if let Some(b) = c.blocks.iter().find(|b| b.ty == name) {
            return Some(
                Diagnostic::error(
                    format!("the `{name}` block is gone; use {now}"),
                    "",
                    b.def.range,
                )
                .in_file(&b.def.file),
            );
        }
    }
    None
}

/// What decoding an app's body can fail with: diagnostics, or a message for the block's header.
#[derive(Debug)]
pub enum DecodeError {
    Diags(Diagnostics),
    Msg(String),
}

/// Go's `process.DecodeConfig` for an app body (`ctx` already sees the probe functions).
pub fn decode_app(
    body: &Body<'_>,
    ctx: &Context<'_>,
    base_dir: &str,
) -> Result<Command, DecodeError> {
    if let Some(d) = check_removed(body) {
        return Err(DecodeError::Diags(d.into()));
    }
    let (raw, ds) = decode_body(body, ctx, &APP);
    if ds.has_errors() {
        return Err(DecodeError::Diags(ds));
    }
    let run = raw.str("run");
    if run.trim().is_empty() {
        return Err(DecodeError::Msg("an app needs a non-empty `run`".into()));
    }
    let env_file = raw.str("env_file");
    let msg = DecodeError::Msg;
    let mut c = Command {
        run,
        env: raw.map("env"),
        dir: resolve_dir(base_dir, &raw.str("dir")),
        console: raw.str("console"),
        watch: raw.list("watch"),
        direct: raw.bool("direct"),
        env_file: if env_file.is_empty() {
            String::new()
        } else {
            resolve_dir(base_dir, &env_file)
        },
        setup: runspec::steps("setup", &raw.any("setup")).map_err(msg)?,
        on_ready: runspec::steps("on_ready", &raw.any("on_ready")).map_err(msg)?,
        stop: runspec::steps("stop", &raw.any("stop")).map_err(msg)?,
        ready: runspec::parse_ready(&raw.any("ready")).map_err(msg)?,
        ready_timeout: runspec::parse_ready_timeout(&raw.str("ready_timeout")).map_err(msg)?,
        sleep: runspec::parse_sleep(&raw.str("sleep")).map_err(msg)?,
        ..Command::default()
    };
    runspec::check_console(&c.console).map_err(msg)?;
    let r = raw.blocks.get("restart");
    c.restart =
        decode_restart(r.map(|r| (r.str("policy"), r.str("backoff"), r.int("max_retries"))))
            .map_err(msg)?;
    Ok(c)
}

/// Go's `decodeRestart` (the app's own words for a bad backoff).
fn decode_restart(r: Option<(String, String, i64)>) -> Result<Restart, String> {
    let mut out = Restart {
        policy: "on_failure".into(),
        backoff: DEFAULT_BACKOFF,
        max_retries: DEFAULT_MAX_RETRIES,
    };
    let Some((policy, backoff, max)) = r else {
        return Ok(out);
    };
    if !policy.is_empty() {
        if !matches!(policy.as_str(), "no" | "on_failure" | "always") {
            return Err(format!(
                "invalid restart policy {} (want no, on_failure, or always)",
                doze_hcl::value::quote::go_quote(&policy)
            ));
        }
        out.policy = policy;
    }
    if !backoff.is_empty() {
        let q = doze_hcl::value::quote::go_quote(&backoff);
        let d =
            gotime::parse_go(&backoff).map_err(|e| format!("invalid restart backoff {q}: {e}"))?;
        if d < 0 {
            return Err("restart backoff must not be negative".into());
        }
        out.backoff = Duration::from_nanos(d.unsigned_abs());
    }
    if max > 0 {
        out.max_retries = max;
    }
    if out.policy == "no" {
        out.max_retries = 0;
    }
    Ok(out)
}

/// Go's `resolveDir`: absolute, against the declaring file's folder.
pub fn resolve_dir(base_dir: &str, p: &str) -> String {
    let p = if p.is_empty() { "." } else { p };
    if Path::new(p).is_absolute() {
        return crate::files::path_string(&crate::paths::clean(Path::new(p)));
    }
    let base = if base_dir.is_empty() { "." } else { base_dir };
    crate::files::path_string(&crate::paths::clean(&Path::new(base).join(p)))
}

/// Go's `BindPort`: the block's port handed to its command, and what it rules out.
pub fn bind_port(c: &mut Command, port: u16) -> Result<(), String> {
    c.declared_port = port;
    if c.sleep.set && !c.sleep.never && !c.proxied() {
        let what = if c.service { "service" } else { "app" };
        return Err(format!(
            "sleep = \"{}\" needs doze to hold the {what}'s port, so that a connection can wake it: declare a `port`, and do not set `direct`",
            gotime::format(c.sleep.after)
        ));
    }
    if c.proxied() {
        for name in ["PORT", "HOST"] {
            if c.env.contains_key(name) {
                return Err(format!(
                    "env sets {name}, but doze chooses it for an app with a `port`: remove it from env (the app is started with ${name}), or set `direct = true` to have the app bind {port} itself"
                ));
            }
        }
    }
    if port == 0
        && let Some(r) = &c.ready
        && r.needs_port()
    {
        return Err(format!("ready = {}(…) needs a `port` to probe", r.kind));
    }
    Ok(())
}

/// Go's `ApplyDefaults`: the stack's defaults where the block's body did not say.
pub fn apply_defaults(c: &mut Command, d: &CommandDefaults, body: &Body<'_>) {
    let schema = Schema::default()
        .attr("ready_timeout", false)
        .attr("env", false)
        .block("restart", &[]);
    let (got, _, _) = body.partial_content(&schema);
    let set = |arg: &str| got.attr(arg).is_some() || got.blocks.iter().any(|b| b.ty == arg);
    if !c.task && !set("ready_timeout") && d.ready_timeout > Duration::ZERO {
        c.ready_timeout = d.ready_timeout;
    }
    if !c.task && !set("restart") && !d.restart_policy.is_empty() {
        c.restart = Restart {
            policy: d.restart_policy.clone(),
            backoff: if d.restart_backoff > Duration::ZERO {
                d.restart_backoff
            } else {
                DEFAULT_BACKOFF
            },
            max_retries: if d.restart_max_retries > 0 {
                d.restart_max_retries
            } else {
                DEFAULT_MAX_RETRIES
            },
        };
    }
    for (k, v) in &d.env {
        c.env.entry(k.clone()).or_insert_with(|| v.clone());
    }
}

/// A service's or a task's command (Go's `CommandConfig`), its port bound.
pub fn command_config(
    mut c: Command,
    restart: &Restart,
    name: &str,
    port: u16,
) -> Result<Command, String> {
    c.restart = Restart {
        policy: if restart.policy.is_empty() {
            "on_failure".into()
        } else {
            restart.policy.clone()
        },
        backoff: if restart.backoff > Duration::ZERO {
            restart.backoff
        } else {
            DEFAULT_BACKOFF
        },
        max_retries: if restart.max_retries > 0 {
            restart.max_retries
        } else {
            DEFAULT_MAX_RETRIES
        },
    };
    if c.restart.policy == "no" || c.task {
        c.restart.max_retries = 0;
    }
    bind_port(&mut c, port).map_err(|e| format!("{name}: {e}"))?;
    Ok(c)
}

/// An app decode failure as Go reports it: diagnostics as they are, a message at the block.
pub fn decode_error(src: &dyn Sources, e: DecodeError, def: &At, title: &str) -> Error {
    match e {
        DecodeError::Diags(ds) => diag_error(src, ds),
        DecodeError::Msg(m) => pos_err(src, def, format!("{title}: {m}"), ""),
    }
}

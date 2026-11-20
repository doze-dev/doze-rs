//! The `defaults` and `tls` blocks (Go's `decodeDefaults` and `decodeTLS`).

use crate::body::Block;
use crate::command::RESTART;
use crate::error::{At, Result, Sources, diag_error, pos_err};
use crate::gohcl::{Spec, Ty, decode_body, opt};
use crate::gotime;
use crate::host::{Host, names_disabled};
use crate::model::{Config, Tls};
use crate::runspec;
use doze_hcl::eval::Context;
use doze_hcl::value::quote::go_quote;
use hcl_edit::Span;
use std::time::Duration;

static DEFAULTS: Spec = Spec {
    attrs: &[
        opt("sleep", Ty::Str),
        opt("names", Ty::Bool),
        opt("log_runs", Ty::Int),
        opt("log_size", Ty::Str),
        opt("ready_timeout", Ty::Str),
        opt("env", Ty::StrMap),
    ],
    blocks: &[("restart", &RESTART)],
    remain: false,
};

static TLS: Spec = Spec {
    attrs: &[
        opt("cert", Ty::Str),
        opt("key", Ty::Str),
        opt("required", Ty::Bool),
    ],
    blocks: &[],
    remain: false,
};

/// Arguments `defaults` used to take, and what to write now. (Go ranges over a map; a block with
/// both is refused for one of them.)
fn old_default(name: &str, text: &str) -> Option<(String, String)> {
    match name {
        "idle_timeout" => {
            let t = text.trim_matches('"');
            let text = if t == "0s" || t == "0" {
                "\"never\""
            } else {
                text
            };
            Some((
                "`idle_timeout` is now `sleep`, the word a block uses for its own".into(),
                format!("write: sleep = {text}"),
            ))
        }
        "domains" => Some((
            "in defaults, `domains` is now `names`: it turns on the <name>.<stack>.doze names"
                .into(),
            format!("write: names = {text} (`domains` on an app is for your own hostnames)"),
        )),
        _ => None,
    }
}

pub(crate) fn decode_defaults(
    cfg: &mut Config,
    src: &dyn Sources,
    host: &dyn Host,
    block: &Block<'_>,
    ctx: &Context<'_>,
) -> Result<()> {
    for (syntax, source) in block.body.syntax_parts() {
        for a in syntax.attributes() {
            let text = a
                .value
                .span()
                .and_then(|s| source.text().get(s))
                .unwrap_or("\"…\"");
            if let Some((msg, fix)) = old_default(a.key.as_str(), text) {
                let r = a.key.span().map(|s| source.range(s)).unwrap_or_default();
                return Err(pos_err(src, &At::new(&block.def.file, r), msg, fix));
            }
        }
    }
    let (d, ds) = decode_body(&block.body, ctx, &DEFAULTS);
    if ds.has_errors() {
        return Err(diag_error(src, ds));
    }
    let at = &block.def;
    let sleep = d.str("sleep");
    if !sleep.is_empty() {
        let sl = runspec::parse_sleep(&sleep)
            .map_err(|e| pos_err(src, at, "invalid sleep in defaults", e))?;
        cfg.defaults.idle_timeout = if sl.never { Duration::ZERO } else { sl.after };
    }
    cfg.defaults.names = d.bool("names") && !names_disabled(host);
    let rt = d.str("ready_timeout");
    if !rt.is_empty() {
        cfg.defaults.commands.ready_timeout = runspec::parse_ready_timeout(&rt)
            .map_err(|e| pos_err(src, at, "invalid ready_timeout in defaults", e))?;
    }
    if let Some(r) = d.blocks.get("restart") {
        let rd = parse_restart(&r.str("policy"), &r.str("backoff"), r.int("max_retries"))
            .map_err(|e| pos_err(src, at, "invalid restart in defaults", e))?;
        let c = &mut cfg.defaults.commands;
        (c.restart_policy, c.restart_backoff, c.restart_max_retries) =
            (rd.policy, rd.backoff, rd.max_retries);
    }
    cfg.defaults.commands.env = d.map("env");
    let runs = d.int("log_runs");
    if runs < 0 {
        return Err(pos_err(src, at, "invalid log_runs", "must not be negative"));
    }
    cfg.defaults.log_runs = runs;
    let size = d.str("log_size");
    if !size.is_empty() {
        cfg.defaults.log_bytes =
            parse_size(&size).map_err(|e| pos_err(src, at, "invalid log_size", e))?;
    }
    Ok(())
}

/// Go's config-side `parseRestart` (defaults and services): the policy checked, the backoff read.
pub(crate) fn parse_restart(
    policy: &str,
    backoff: &str,
    max_retries: i64,
) -> std::result::Result<crate::model::Restart, String> {
    let mut out = crate::model::Restart {
        policy: policy.to_owned(),
        backoff: Duration::ZERO,
        max_retries,
    };
    if !matches!(policy, "" | "no" | "on_failure" | "always") {
        return Err(format!(
            "invalid restart policy {} (want no, on_failure, or always)",
            go_quote(policy)
        ));
    }
    if !backoff.is_empty() {
        out.backoff = gotime::parse_non_negative(backoff)
            .ok_or_else(|| format!("invalid restart backoff {}", go_quote(backoff)))?;
    }
    Ok(out)
}

/// `log_size = "10MB"`.
fn parse_size(s: &str) -> std::result::Result<i64, String> {
    let upper = s.trim().to_uppercase();
    for (suffix, bytes) in [
        ("GB", 1i64 << 30),
        ("MB", 1 << 20),
        ("KB", 1 << 10),
        ("B", 1),
    ] {
        if let Some(num) = upper.strip_suffix(suffix) {
            match num.trim().parse::<i64>() {
                Ok(n) if n > 0 => return n.checked_mul(bytes).ok_or_else(|| bad_size(s)),
                _ => break,
            }
        }
    }
    Err(bad_size(s))
}

fn bad_size(s: &str) -> String {
    format!("{} is not a size (try \"10MB\", \"512KB\")", go_quote(s))
}

pub(crate) fn decode_tls(
    cfg: &mut Config,
    src: &dyn Sources,
    block: &Block<'_>,
    ctx: &Context<'_>,
) -> Result<()> {
    let (t, ds) = decode_body(&block.body, ctx, &TLS);
    if ds.has_errors() {
        return Err(diag_error(src, ds));
    }
    cfg.tls = Tls {
        enabled: true,
        cert: t.str("cert"),
        key: t.str("key"),
        required: t.bool("required"),
    };
    if cfg.tls.cert.is_empty() != cfg.tls.key.is_empty() {
        return Err(pos_err(
            src,
            &block.def,
            "incomplete tls block",
            "set both cert and key, or neither (to auto-generate a self-signed cert)",
        ));
    }
    Ok(())
}

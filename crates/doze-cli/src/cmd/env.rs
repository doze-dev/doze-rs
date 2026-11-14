//! `doze env [service…] [--dotenv]` and `doze url <name>` (Go cmd/doze/env.go, verbs.go `urlCmd`), and the variables
//! `doze run --env` sets. Go computed them from doze.hcl with each module's plugin; here they are each module's own
//! answer on the wire (`InstanceView.env`, `url`, `https`: D1's module catalog), so the CLI holds nothing
//! module-specific. The stack's daemon is asked, and started first when it is not running (as `doze run` starts it:
//! the modules answer, nothing wakes; connecting is what wakes a service), which is said on stderr.
//!
//! Go's rules kept: names checked against doze.hcl first (exit 4); a variable two services answer to keeps the first
//! bare and suffixes the next with its name (`DATABASE_URL_REPORTS`), and any other variable two answer keeps the
//! first answer only (Go added AWS_REGION and the like once); a stack that serves a domain from the doze
//! authority adds the variables that point clients at it. Go's AWS stand-in credentials are the aws module's to answer.

use crate::cli::args::{Env, Service};
use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::output::{Done, Mode, Outcome};
use crate::paint::Paint;
use crate::stack::{self, Stack};
use crate::view::View;
use doze_config::Config;
use doze_control::Snapshot;
use serde::{Serialize, Serializer};

/// One variable, as Go's `EnvPair`.
#[derive(Debug, Clone, Serialize)]
pub struct Pair {
    pub service: String,
    pub engine: String,
    pub var: String,
    pub value: String,
}

/// What `doze env` prints.
#[derive(Debug)]
pub struct Vars {
    pub pairs: Vec<Pair>,
    dotenv: bool,
}

impl Serialize for Vars {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.pairs.serialize(s)
    }
}

impl Vars {
    pub(crate) fn human(&self, _p: Paint) -> String {
        let mut out = String::new();
        for p in &self.pairs {
            if self.dotenv {
                out.push_str(&format!("{}={}\n", p.var, p.value));
                continue;
            }
            let note = if p.service.is_empty() {
                String::new()
            } else {
                format!("  # {} {}", p.engine, p.service)
            };
            out.push_str(&format!("export {}={}{note}\n", p.var, quote(&p.value)));
        }
        out
    }
}

/// A POSIX shell's single-quoted word (Go `shellQuote`).
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// `doze env`.
pub(crate) fn run(ctx: &mut Ctx<'_>, mode: &Mode, a: &Env) -> Outcome {
    let names = stack::unique(&a.services);
    let (cfg, snap) = asked(ctx, mode, &names, "doze env")?;
    let pairs = pairs(ctx, &cfg, &snap, &names);
    if pairs.is_empty() && !mode.json && !a.dotenv {
        let _ = writeln!(
            ctx.err,
            "{}",
            mode.err().dim("no services with connection URLs declared")
        );
    }
    Ok(Done::ok(View::Env(Vars {
        pairs,
        dotenv: a.dotenv,
    })))
}

/// `doze url <name>`: the one line a client needs: the app's own https domain, its connection string, or its address.
pub(crate) fn url(ctx: &mut Ctx<'_>, mode: &Mode, a: &Service) -> Outcome {
    let name = a.name.clone();
    let (_, snap) = asked(ctx, mode, std::slice::from_ref(&name), "doze url")?;
    let v = stack::declared(&snap)
        .find(|v| v.name == name)
        .ok_or_else(|| stack::not_declared(&snap, &name))?;
    let line = [&v.https, &v.url, &v.endpoint]
        .into_iter()
        .find(|s| !s.is_empty())
        .cloned()
        .ok_or_else(|| {
            Diagnostic::new(
                ids::NOTHING_TO_DO,
                format!("{name} has no address: it declares no port"),
            )
        })?;
    Ok(Done::ok(View::Url(Line { name, url: line })))
}

/// `doze url`'s result (Go's `{name, url}`).
#[derive(Debug, Serialize)]
pub struct Line {
    pub name: String,
    pub url: String,
}

/// doze.hcl's shape (names checked, exit 4/5), then the running stack's snapshot, its daemon started when it is down.
pub(crate) fn asked(
    ctx: &mut Ctx<'_>,
    mode: &Mode,
    names: &[String],
    what: &str,
) -> Result<(Config, Snapshot), Diagnostic> {
    let stack = Stack::find(ctx, mode)?;
    stack::check_offline(&stack, names)?;
    let cfg = stack.loader.shape(&stack.project)?;
    let conn = match stack.open(mode.timeout)? {
        Some(c) => c,
        None => {
            let _lock = stack::lock(ctx, &stack)?;
            let c = crate::cmd::up::start(ctx, mode, &stack)?;
            let line = format!(
                "started the doze daemon for {what}; services wake on their first connection"
            );
            let _ = writeln!(ctx.err, "{}", mode.err().dim(&line));
            c
        }
    };
    Ok((cfg, stack::snapshot(&conn)?))
}

/// The variables of `names` (none: every declared one), in the stack's order, then the authority's.
pub(crate) fn pairs(
    ctx: &mut Ctx<'_>,
    cfg: &Config,
    snap: &Snapshot,
    names: &[String],
) -> Vec<Pair> {
    let mut out: Vec<Pair> = Vec::new();
    for v in stack::declared(snap).filter(|v| names.is_empty() || names.contains(&v.name)) {
        // The module's own variable (its connection string) first, then the rest in name order.
        let main = v.env.get_key_value(&v.env_var);
        let rest = v.env.iter().filter(|(k, _)| **k != v.env_var);
        for (var, value) in main.into_iter().chain(rest) {
            let taken = out.iter().any(|p| &p.var == var);
            if value.is_empty() || (taken && *var != v.env_var) {
                continue; // a shared variable (AWS_REGION) keeps the first answer, as Go added it once
            }
            let var = if taken {
                format!("{var}_{}", suffix(&v.name))
            } else {
                var.clone()
            };
            out.push(Pair {
                service: v.name.clone(),
                engine: v.engine.clone(),
                var,
                value: value.clone(),
            });
        }
    }
    out.extend(authority(ctx, cfg));
    out
}

/// An instance name as a variable's suffix (Go `envSuffix`): `app-dev` → `APP_DEV`.
fn suffix(name: &str) -> String {
    name.chars()
        .map(|c| match c.to_ascii_uppercase() {
            u @ ('A'..='Z' | '0'..='9') => u,
            _ => '_',
        })
        .collect()
}

/// The variables that point clients at the doze authority (Go `caPairs`), for a stack that serves a domain from it,
/// once the daemon has made it.
fn authority(ctx: &mut Ctx<'_>, cfg: &Config) -> Vec<Pair> {
    let uses = cfg
        .instances
        .iter()
        .any(|i| i.enabled && !i.domains.is_empty() && i.cert.is_empty());
    let home = std::path::Path::new(&cfg.home);
    let Some(ca) = uses
        .then(|| {
            doze_names::ca::Ca::probe(&doze_names::ca::ca_dir(home))
                .ok()
                .flatten()
        })
        .flatten()
    else {
        return Vec::new();
    };
    let pair = |var: &str, value: &std::path::Path, service: &str| Pair {
        service: service.into(),
        engine: String::new(),
        var: var.into(),
        value: value.display().to_string(),
    };
    let root = ca.root_path();
    let mut out = vec![
        pair("DOZE_CA_FILE", &root, "the doze certificate authority"),
        pair("NODE_EXTRA_CA_CERTS", &root, "Node adds it"),
    ];
    // The two that replace a client's store get the machine's roots and this one, never the root alone.
    match ca.ensure_bundle() {
        Ok(bundle) => out.extend([
            pair("SSL_CERT_FILE", &bundle, "OpenSSL, Go on Linux, curl"),
            pair("REQUESTS_CA_BUNDLE", &bundle, "Python requests"),
        ]),
        Err(e) => {
            let _ = writeln!(
                ctx.err,
                "SSL_CERT_FILE and REQUESTS_CA_BUNDLE left unset: {e}"
            );
        }
    }
    out
}

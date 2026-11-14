//! `service` and `task` blocks: declared by name first, published to every other block, and
//! decoded once every module and app is (Go's `runnable.go`).

use crate::attrs::join_host_port;
use crate::body::{Block, Body, attrs_schema};
use crate::command::{apply_defaults, command_config, resolve_dir};
use crate::defaults::parse_restart;
use crate::deps::{parse_after, portless_url, resolve, self_reference};
use crate::error::{At, Error, Result, diag_error, pos_err};
use crate::eval::decode_int;
use crate::foreach::stamps;
use crate::gohcl::{Decoded, Spec, Ty, decode_body, opt, req};
use crate::load::{Declared, Loader, Runnable};
use crate::model::{
    Command, Condition, Config, Instance, Kind, Ref, ServiceDecl, Spec as RunSpec, TaskDecl, title,
};
use crate::refs::referenced;
use crate::runcmd::{boot_edges, command_instance, parse_download, task};
use crate::runspec;
use doze_hcl::Value;
use doze_hcl::value::quote::go_quote;
use std::collections::BTreeMap;

static SERVICE: Spec = Spec {
    attrs: &[
        req("run", Ty::Str),
        opt("dir", Ty::Str),
        opt("port", Ty::Int),
        opt("env", Ty::StrMap),
        opt("env_file", Ty::Str),
        opt("download", Ty::Any),
        opt("setup", Ty::Any),
        opt("on_ready", Ty::Any),
        opt("stop", Ty::Any),
        opt("ready", Ty::Any),
        opt("ready_timeout", Ty::Str),
        opt("sleep", Ty::Str),
        opt("console", Ty::Str),
    ],
    blocks: &[("restart", &crate::command::RESTART)],
    remain: false,
};

static TASK: Spec = Spec {
    attrs: &[
        req("run", Ty::Str),
        opt("dir", Ty::Str),
        opt("env", Ty::StrMap),
        opt("env_file", Ty::Str),
        opt("inputs", Ty::StrList),
        opt("manual", Ty::Bool),
    ],
    blocks: &[],
    remain: false,
};

/// A body without `after`, what `after` names, what the body references, and `after`'s waits.
type Split<'a> = (Body<'a>, Vec<Ref>, Vec<Ref>, BTreeMap<String, Condition>);

impl<'a> Loader<'a> {
    /// Go's `declareRunnable`: a service's or task's name (and a service's port), per stamp.
    pub(crate) fn declare_runnable(&mut self, cfg: &mut Config, block: &Block<'a>) -> Result<()> {
        let files = self.files;
        let label = block.label(0);
        let kind = if block.ty == "task" {
            Kind::Task
        } else {
            Kind::Service
        };
        let meta = attrs_schema(&["count", "for_each", "enabled", "depends_on"]);
        let (m, body, ds) = block.body.partial_content(&meta);
        if ds.has_errors() {
            return Err(diag_error(files, ds));
        }
        if let Some(a) = m.attr("depends_on") {
            return Err(pos_err(
                files,
                &At::new(a.file, a.name_range),
                "`depends_on` is now `after`",
                "write: after = [module.db, started(app.api)]: a bare reference waits until it is ready; started(…) until it runs",
            ));
        }
        let what = title(kind, "", label);
        if m.attr("count").is_some() && m.attr("for_each").is_some() {
            return Err(pos_err(
                files,
                &block.def,
                format!("{what}: set either count or for_each, not both"),
                "",
            ));
        }
        let enabled = self.enabled(m.attr("enabled"), &what)?;
        let stamps = self.in_ctx(None, false, |ctx| {
            stamps(files, m.attr("count"), m.attr("for_each"), ctx)
        })?;
        for st in stamps {
            let name = st.name(label);
            self.claim_name(&title(kind, "", &name), &name, &block.def)?;
            let run = Runnable {
                name: name.clone(),
                kind,
                def: block.def.clone(),
                body: body.clone(),
                stamp: st.vars.clone(),
                enabled,
                waits: BTreeMap::new(),
            };
            if kind == Kind::Task {
                cfg.tasks.push(TaskDecl {
                    name,
                    ..TaskDecl::default()
                });
            } else {
                let port = self.service_port(&body, &run, &name)?;
                cfg.services.push(ServiceDecl {
                    name,
                    port,
                    ..ServiceDecl::default()
                });
            }
            self.runnables.push(run);
        }
        Ok(())
    }

    fn service_port(&self, body: &Body<'a>, run: &Runnable<'a>, name: &str) -> Result<u16> {
        let (head, _, _) = body.partial_content(&attrs_schema(&["port"]));
        let Some(a) = head.attr("port") else {
            return Ok(0);
        };
        let p = self
            .in_ctx(run.stamp.as_ref(), false, |ctx| decode_int(a, ctx))
            .map_err(|ds| diag_error(self.files, ds))?;
        u16::try_from(p).ok().filter(|p| *p >= 1).ok_or_else(|| {
            pos_err(
                self.files,
                &At::new(a.file, a.range),
                format!(
                    "{}: port {p} is out of range",
                    title(Kind::Service, "", name)
                ),
                "a port must be between 1 and 65535",
            )
        })
    }

    /// Go's `publishRunnables`: `service.<name>.*` and `task.<name>.*` for every other block.
    pub(crate) fn publish_runnables(&mut self, cfg: &Config) {
        let s = Value::string;
        let mut services = BTreeMap::new();
        for sv in &cfg.services {
            let host = if cfg.defaults.names {
                cfg.domain_for(&sv.name)
            } else {
                "127.0.0.1".into()
            };
            let address = if sv.port == 0 {
                String::new()
            } else {
                join_host_port(&host, &sv.port.to_string())
            };
            services.insert(
                sv.name.clone(),
                Value::Object(BTreeMap::from([
                    ("name".to_owned(), s(&sv.name)),
                    ("address".to_owned(), s(&address)),
                    ("host".to_owned(), s(&host)),
                    ("port".to_owned(), Value::int(i64::from(sv.port))),
                    ("data".to_owned(), s(&cfg.cluster_dir(&sv.name))),
                ])),
            );
        }
        let tasks = cfg
            .tasks
            .iter()
            .map(|t| {
                (
                    t.name.clone(),
                    Value::Object(BTreeMap::from([("name".to_owned(), s(&t.name))])),
                )
            })
            .collect();
        self.vars.insert("service".into(), Value::Object(services));
        self.vars.insert("task".into(), Value::Object(tasks));
    }

    pub(crate) fn decode_runnables(&mut self, cfg: &mut Config, known: &Declared) -> Result<()> {
        for kind in [Kind::Service, Kind::Task] {
            for i in 0..self.runnables.len() {
                if self.runnables.get(i).is_some_and(|r| r.kind == kind) {
                    self.decode_runnable(cfg, i, known)?;
                }
            }
        }
        Ok(())
    }

    /// Go's `splitAfter`: the body without `after`, `after`'s references, and the body's own.
    fn split_after(&self, cfg: &Config, r: &Runnable<'a>, known: &Declared) -> Result<Split<'a>> {
        let files = self.files;
        let (meta, rest, ds) = r.body.partial_content(&attrs_schema(&["after"]));
        if ds.has_errors() {
            return Err(diag_error(files, ds));
        }
        let (after, waits) = match meta.attr("after") {
            Some(a) => parse_after(files, a, known)?,
            None => (Vec::new(), BTreeMap::new()),
        };
        let mut refs = Vec::new();
        let file = rest.file_name().to_owned();
        for t in referenced(&rest) {
            let Some(rf) = resolve(files, known, &file, &t)? else {
                continue;
            };
            if rf.name == r.name {
                return Err(self_reference(
                    files,
                    &format!("{} {}", rf.kind.as_str(), go_quote(&r.name)),
                    &file,
                    &t,
                ));
            }
            portless_url(files, cfg, &file, &rf, &t)?;
            refs.push(rf);
        }
        Ok((rest, after, refs, waits))
    }

    fn decode_runnable(&mut self, cfg: &mut Config, i: usize, known: &Declared) -> Result<()> {
        let Some(r) = self.runnables.get(i) else {
            return Ok(());
        };
        let (rest, after, refs, waits) = self.split_after(cfg, r, known)?;
        let what = title(r.kind, "", &r.name);
        if r.kind == Kind::Service {
            let (gone, _, _) = rest.partial_content(&attrs_schema(&["front", "logs"]));
            for arg in ["front", "logs"] {
                if let Some(a) = gone.attr(arg) {
                    return Err(pos_err(
                        self.files,
                        &At::new(a.file, a.name_range),
                        format!(
                            "{what}: `{arg}` is no longer an argument: it was never acted on; delete it"
                        ),
                        "",
                    ));
                }
            }
        }
        let spec = if r.kind == Kind::Service {
            &SERVICE
        } else {
            &TASK
        };
        let (raw, ds) = self.in_ctx(r.stamp.as_ref(), true, |ctx| decode_body(&rest, ctx, spec));
        if ds.has_errors() {
            return Err(diag_error(self.files, ds));
        }
        let fail = |e: String| pos_err(self.files, &r.def, format!("{what}: {e}"), "");
        if raw.str("run").trim().is_empty() {
            return Err(fail("`run` must not be empty".into()));
        }
        let base = self.base_dir(&r.def);
        let (name, enabled, body) = (r.name.clone(), r.enabled, r.body.clone());
        let mut inst = if r.kind == Kind::Service {
            self.service(cfg, &raw, &base, &name, (&after, &refs))
                .map_err(fail)?
        } else {
            task(cfg, &raw, &base, &name, (&after, &refs))
                .map_err(|e| Error::msg(format!("{what}: {e}")))?
        };
        if let RunSpec::Command(c) = &mut inst.spec {
            apply_defaults(c, &cfg.defaults.commands, &body);
        }
        inst.deps = boot_edges(&name, &refs, &after, &waits);
        (inst.enabled, inst.after, inst.refs, inst.index) =
            (enabled, after, refs, cfg.instances.len());
        if let Some(r) = self.runnables.get_mut(i) {
            r.waits = waits;
        }
        cfg.instances.push(inst);
        Ok(())
    }

    fn service(
        &self,
        cfg: &mut Config,
        raw: &Decoded<'a>,
        base: &str,
        name: &str,
        (after, refs): (&[Ref], &[Ref]),
    ) -> std::result::Result<Instance, String> {
        let env_file = raw.str("env_file");
        let port = cfg.service(name).map_or(0, |s| s.port);
        let c = Command {
            run: raw.str("run"),
            env: raw.map("env"),
            dir: resolve_dir(base, &raw.str("dir")),
            dir_set: !raw.str("dir").is_empty(),
            console: raw.str("console"),
            env_file: if env_file.is_empty() {
                String::new()
            } else {
                resolve_dir(base, &env_file)
            },
            download: parse_download(&raw.any("download"))?,
            setup: runspec::steps("setup", &raw.any("setup"))?,
            on_ready: runspec::steps("on_ready", &raw.any("on_ready"))?,
            stop: runspec::steps("stop", &raw.any("stop"))?,
            ready: runspec::parse_ready(&raw.any("ready"))?,
            service: true,
            ..Command::default()
        };
        if let Some(p) = c.ready.as_ref().filter(|p| p.needs_port() && port == 0) {
            return Err(format!("ready = {}(…) needs a `port` to probe", p.kind));
        }
        let mut c = Command {
            ready_timeout: runspec::parse_ready_timeout(&raw.str("ready_timeout"))?,
            sleep: runspec::parse_sleep(&raw.str("sleep"))?,
            ..c
        };
        runspec::check_console(&c.console)?;
        let r = raw.blocks.get("restart");
        let restart = parse_restart(
            &r.map(|r| r.str("policy")).unwrap_or_default(),
            &r.map(|r| r.str("backoff")).unwrap_or_default(),
            r.map_or(0, |r| r.int("max_retries")),
        )?;
        c.restart = restart.clone();
        let decl = ServiceDecl {
            name: name.to_owned(),
            run: c.run.clone(),
            dir: c.dir.clone(),
            port,
            dir_set: c.dir_set,
            env: c.env.clone(),
            env_file: c.env_file.clone(),
            download: c.download.clone(),
            setup: c.setup.clone(),
            on_ready: c.on_ready.clone(),
            stop: c.stop.clone(),
            ready: c.ready.clone(),
            ready_timeout: c.ready_timeout,
            restart: restart.clone(),
            sleep: c.sleep,
            console: c.console.clone(),
            after: after.to_vec(),
            refs: refs.to_vec(),
        };
        if let Some(s) = cfg.services.iter_mut().find(|s| s.name == name) {
            *s = decl;
        }
        let c = command_config(c, &restart, name, port)?;
        Ok(command_instance(Kind::Service, name, port, c))
    }
}

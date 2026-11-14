//! Module and app bodies, decoded in dependency order so that each sees what it references (Go's
//! `loader.evaluate` and `instanceDeps`).

use crate::deps::{depends_on_gone, parse_after, portless_url, resolve, self_reference};
use crate::error::{Result, diag_error, pos_err};
use crate::load::{Declared, Loader, Pending, Probes};
use crate::model::{Condition, Config, Dependency, Kind, Spec};
use crate::refs::referenced;
use doze_hcl::Value;
use std::collections::BTreeMap;

impl<'a> Loader<'a> {
    pub(crate) fn evaluate(
        &mut self,
        cfg: &mut Config,
        pending: &mut [Pending<'a>],
        known: &Declared,
    ) -> Result<()> {
        for p in pending.iter() {
            self.instance_deps(cfg, p, known)?;
        }
        let order = self.topo_order(cfg, pending)?;
        self.check_disabled(cfg, pending)?;
        self.vars
            .insert("module".into(), Value::Object(BTreeMap::new()));
        self.vars
            .insert("app".into(), Value::Object(BTreeMap::new()));
        let stand_ins = if self.shape {
            crate::shape::referenced_module_attrs(self.files)
        } else {
            BTreeMap::new()
        };
        for i in order {
            let Some(p) = pending.get(i) else { continue };
            let attrs = if p.module.is_none() {
                let inst = cfg.instances.get(p.idx).cloned().unwrap_or_default();
                crate::shape::stand_in(&inst, stand_ins.get(&inst.name))
            } else {
                self.decode_body(cfg, p)?;
                crate::attrs::attributes_for(cfg, self.opts.modules, p.idx)
            };
            let Some(inst) = cfg.instances.get(p.idx) else {
                continue;
            };
            let kind = inst.kind().as_str();
            if let Some(Value::Object(m)) = self.vars.get_mut(kind) {
                m.insert(inst.name.clone(), attrs);
            }
        }
        Ok(())
    }

    /// Go's `instanceDeps`: what the body references and what `after` names, as dependencies.
    fn instance_deps(&self, cfg: &mut Config, p: &Pending<'a>, known: &Declared) -> Result<()> {
        let files = self.files;
        let Some(inst) = cfg.instances.get(p.idx) else {
            return Ok(());
        };
        let (me, title) = (inst.name.clone(), inst.title());
        let mut order: Vec<String> = Vec::new();
        let mut cond: BTreeMap<String, Condition> = BTreeMap::new();
        let mut refs = Vec::new();
        let mut edge = |name: &str, order: &mut Vec<String>| {
            if name != me && !cond.contains_key(name) {
                order.push(name.to_owned());
                cond.insert(name.to_owned(), Condition::Healthy);
            }
        };
        let file = p.body.file_name().to_owned();
        for t in referenced(&p.body) {
            let Some(r) = resolve(files, known, &file, &t)? else {
                continue;
            };
            if r.name == me {
                return Err(self_reference(files, &title, &file, &t));
            }
            portless_url(files, cfg, &file, &r, &t)?;
            edge(&r.name, &mut order);
            refs.push(r);
        }
        let mut after = Vec::new();
        let mut waits = BTreeMap::new();
        if let Some(a) = &p.after {
            (after, waits) = parse_after(files, a, known)?;
            for r in &after {
                edge(&r.name, &mut order);
            }
        }
        cond.extend(waits);
        if let Some(a) = &p.depends_on {
            return Err(depends_on_gone(files, &title, &p.explicit, a, known));
        }
        let Some(inst) = cfg.instances.get_mut(p.idx) else {
            return Ok(());
        };
        inst.refs = refs;
        inst.after = after;
        inst.deps = order
            .into_iter()
            .map(|n| Dependency {
                condition: cond.get(&n).copied().unwrap_or_default(),
                name: n,
            })
            .collect();
        Ok(())
    }

    /// Go's `topoOrder`: a depth-first order of the pending blocks (indexes into `pending`).
    fn topo_order(&self, cfg: &Config, pending: &[Pending<'a>]) -> Result<Vec<usize>> {
        let by_name: BTreeMap<&str, usize> = pending
            .iter()
            .enumerate()
            .filter_map(|(i, p)| cfg.instances.get(p.idx).map(|inst| (inst.name.as_str(), i)))
            .collect();
        let mut state = vec![0u8; pending.len()];
        let mut order = Vec::new();
        for i in 0..pending.len() {
            self.visit(
                cfg,
                pending,
                &by_name,
                i,
                &mut Vec::new(),
                &mut state,
                &mut order,
            )?;
        }
        Ok(order)
    }

    #[allow(clippy::too_many_arguments)]
    fn visit(
        &self,
        cfg: &Config,
        pending: &[Pending<'a>],
        by_name: &BTreeMap<&str, usize>,
        i: usize,
        stack: &mut Vec<String>,
        state: &mut [u8],
        order: &mut Vec<usize>,
    ) -> Result<()> {
        let (Some(p), Some(s)) = (pending.get(i), state.get(i).copied()) else {
            return Ok(());
        };
        let Some(inst) = cfg.instances.get(p.idx) else {
            return Ok(());
        };
        match s {
            2 => return Ok(()),
            1 => {
                let mut cycle = stack.clone();
                cycle.push(inst.name.clone());
                return Err(pos_err(
                    self.files,
                    &p.def,
                    "dependency cycle between instances",
                    cycle.join(" → "),
                ));
            }
            _ => {}
        }
        if let Some(s) = state.get_mut(i) {
            *s = 1;
        }
        for d in &inst.deps {
            if let Some(&j) = by_name.get(d.name.as_str()) {
                stack.push(inst.name.clone());
                let r = self.visit(cfg, pending, by_name, j, stack, state, order);
                stack.pop();
                r?;
            }
        }
        if let Some(s) = state.get_mut(i) {
            *s = 2;
        }
        order.push(i);
        Ok(())
    }

    fn check_disabled(&self, cfg: &Config, pending: &[Pending<'a>]) -> Result<()> {
        for p in pending {
            let Some(inst) = cfg.instances.get(p.idx).filter(|i| i.enabled) else {
                continue;
            };
            for d in &inst.deps {
                let off = pending.iter().any(|q| {
                    cfg.instances
                        .get(q.idx)
                        .is_some_and(|o| o.name == d.name && !o.enabled)
                });
                if off {
                    let q = doze_hcl::value::quote::go_quote;
                    return Err(pos_err(
                        self.files,
                        &p.def,
                        format!(
                            "{} depends on {}, which is disabled",
                            inst.title(),
                            q(&d.name)
                        ),
                        format!(
                            "enable {}, or disable {} as well",
                            q(&d.name),
                            q(&inst.name)
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    /// Go's `decodeBody`: an app's command, or a module's body as written.
    fn decode_body(&self, cfg: &mut Config, p: &Pending<'a>) -> Result<()> {
        let files = self.files;
        let Some(inst) = cfg.instances.get(p.idx) else {
            return Ok(());
        };
        let (title, port, kind) = (inst.title(), inst.port, inst.kind());
        let spec = if kind == Kind::App {
            let mut c = self
                .in_ctx(p.stamp.as_ref(), Probes::Inner, |ctx| {
                    crate::command::decode_app(&p.remain, ctx, &p.base_dir)
                })
                .map_err(|e| crate::command::decode_error(files, e, &p.def, &title))?;
            crate::command::bind_port(&mut c, port)
                .map_err(|e| pos_err(files, &p.def, format!("{title}: {e}"), ""))?;
            crate::command::apply_defaults(&mut c, &cfg.defaults.commands, &p.body);
            Spec::Command(Box::new(c))
        } else {
            let (b, ds) = self.in_ctx(p.stamp.as_ref(), Probes::No, |ctx| {
                crate::module_body::decode(&p.remain, ctx)
            });
            if ds.has_errors() {
                return Err(diag_error(files, ds));
            }
            Spec::Module(b)
        };
        let Some(inst) = cfg.instances.get_mut(p.idx) else {
            return Ok(());
        };
        inst.spec = spec;
        self.check_sleep(cfg, p)
    }

    /// A module doze does not sit in front of cannot sleep.
    fn check_sleep(&self, cfg: &Config, p: &Pending<'a>) -> Result<()> {
        let Some(inst) = cfg.instances.get(p.idx) else {
            return Ok(());
        };
        if inst.kind() != Kind::Module
            || !inst.sleep.set
            || inst.sleep.never
            || !p.module.as_ref().is_some_and(|m| m.supervised)
        {
            return Ok(());
        }
        let at = p.sleep_at.clone().unwrap_or_else(|| p.def.clone());
        Err(pos_err(
            self.files,
            &at,
            format!(
                "{} can't sleep: doze doesn't sit in front of it, so nothing would wake it",
                inst.title()
            ),
            "delete sleep, or set sleep = \"never\"",
        ))
    }
}

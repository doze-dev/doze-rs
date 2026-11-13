//! The look before Up and before a service wakes (Go `Preflight`, `PreflightInstance`, D19): nothing in the way, it
//! just goes; something red, the sheet asks. And the ways out of the sheet (Go `PreflightFix`): stop the process
//! holding a port (its pid checked again first: it must still hold a port this start needs, and not be doze's), start
//! without one service, start anyway, edit doze.hcl, copy the setup command, remove a stack whose folder is gone.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use doze_control::Verb;
use doze_desk::act::{
    FixKind, Looked, StackAction, decl_port, preflight, preflight_instance, starting_set,
};
use doze_desk::overview::Shape;
use doze_desk::{Hub, StackKey, convert};
use doze_display as words;
use gpui::{ClipboardItem, Context};

use super::{Acts, ActsEvent, Check};
use crate::facts::Mode;

/// What goes ahead once the sheet is answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Then {
    Stack(StackAction),
    Verb(String, Verb),
}

/// A stack's shape now, off the UI thread: `doze --json status` in its folder (a fixture's own `status.json`).
pub(crate) fn shape_now(mode: &Mode, dir: &Path) -> Shape {
    let json = match mode {
        Mode::Fixture => std::fs::read_to_string(dir.join("status.json")).ok(),
        Mode::Live(Some(doze)) => crate::facts::run(doze, dir, &["status"]),
        Mode::Live(None) => None,
    };
    Shape::read(dir, json.as_deref())
}

/// The domains among `domains` the machine has not set up, from `doze --json doctor` (a fixture's machine is set up).
fn untrusted(mode: &Mode, dir: &Path, domains: &[String]) -> Vec<String> {
    let Mode::Live(Some(doze)) = mode else {
        return Vec::new();
    };
    if domains.is_empty() {
        return Vec::new();
    }
    let Some(doctor) = crate::facts::run(doze, dir, &["doctor"])
        .as_deref()
        .and_then(doze_desk::library::parse_doctor)
    else {
        return Vec::new();
    };
    let ok = |name: &str| doctor.checks.iter().any(|c| c.name == name && c.ok);
    let ca = ok("certificate authority");
    domains
        .iter()
        .filter(|d| !ca || !ok(&format!("route {d}")))
        .cloned()
        .collect()
}

/// Every running service with a pid: (stack, service, pid), for telling doze's own holders apart.
fn services(hub: &Hub) -> Vec<(String, String, i64)> {
    hub.stacks()
        .into_iter()
        .filter(|s| s.running)
        .flat_map(|s| {
            s.instances
                .iter()
                .filter(|v| v.pid > 0)
                .map(|v| (s.name.clone(), v.name.clone(), v.pid))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn look(hub: &Hub, ports: &[u16], untrusted: Vec<String>) -> Looked {
    let owned = services(hub);
    let refs: Vec<(&str, &str, i64)> = owned
        .iter()
        .map(|(a, b, p)| (a.as_str(), b.as_str(), *p))
        .collect();
    Looked::look(ports.iter().copied(), &refs, untrusted)
}

/// Stops `pid` if it still holds one of `ports` and is not doze's; says why not otherwise.
fn stop_holder(hub: &Hub, pid: i64, ports: &[u16], what: &str) -> Result<(), String> {
    let seen = look(hub, ports, Vec::new());
    let held = ports
        .iter()
        .filter_map(|p| seen.held.get(p))
        .find(|h| h.pid == pid);
    match held {
        None => {
            return Err(format!(
                "pid {pid} no longer holds a port {what} needs; nothing was stopped"
            ));
        }
        Some(h) if !h.stack.is_empty() => {
            return Err(format!(
                "pid {pid} is {} in {}; doze does not stop another stack's service from here",
                h.service, h.stack
            ));
        }
        Some(h) if h.doze => return Err(format!("pid {pid} is doze's own; nothing was stopped")),
        Some(_) => {}
    }
    let p = u32::try_from(pid).map_err(|_| format!("pid {pid} is not a process"))?;
    doze_os::process::terminate(p).map_err(|e| format!("could not stop pid {pid}: {e}"))?;
    let start = Instant::now();
    while doze_os::process::alive(p) && start.elapsed() < Duration::from_secs(3) {
        std::thread::sleep(Duration::from_millis(100));
    }
    for port in ports {
        doze_desk::ports::forget(*port);
    }
    if doze_os::process::alive(p) {
        return Err(format!("pid {pid} did not stop; nothing was started"));
    }
    Ok(())
}

impl Acts {
    /// Up looks first: with nothing in the way it just goes; with something red, the sheet asks.
    pub(super) fn check_up(&mut self, key: &StackKey, action: StackAction, cx: &mut Context<Self>) {
        let Some(st) = self.hub.stack(key) else {
            return;
        };
        let (hub, mode, k) = (self.hub.clone(), self.mode.clone(), key.clone());
        cx.spawn(async move |this, cx| {
            let a = action.clone();
            let found = cx
                .background_executor()
                .spawn(async move {
                    let dir = PathBuf::from(&st.dir);
                    let shape = shape_now(&mode, &dir);
                    let chosen = match &a {
                        StackAction::Up(n) => n.clone(),
                        _ => Vec::new(),
                    };
                    let set = starting_set(&shape.decls, &st.last_set, a.name(), &chosen);
                    let ports: Vec<u16> = set.decls.iter().filter_map(|d| decl_port(d)).collect();
                    let domains: Vec<String> =
                        set.decls.iter().flat_map(|d| d.domains.clone()).collect();
                    let names = set.names.clone();
                    let looked = look(&hub, &ports, untrusted(&mode, &dir, &domains));
                    let name = if st.name.is_empty() {
                        shape.stack.clone()
                    } else {
                        st.name.clone()
                    };
                    let p = preflight(
                        &name,
                        &dir,
                        &shape,
                        &st.last_set,
                        (a.name(), &chosen),
                        &looked,
                    );
                    (p, ports, names)
                })
                .await;
            let _ = this.update(cx, |acts, cx| {
                let (found, ports, names) = found;
                if found.blocked {
                    cx.emit(ActsEvent::Check(Check {
                        key: k,
                        found,
                        then: Then::Stack(action),
                        ports,
                        names,
                    }));
                } else {
                    acts.run_stack(&k, action, cx);
                }
            });
        })
        .detach();
    }

    /// Waking (or turning on) one service looks at its port first.
    pub(super) fn check_wake(
        &mut self,
        key: &StackKey,
        name: &str,
        verb: Verb,
        cx: &mut Context<Self>,
    ) {
        let Some(st) = self.hub.stack(key) else {
            return;
        };
        let (hub, mode, k, n) = (
            self.hub.clone(),
            self.mode.clone(),
            key.clone(),
            name.to_owned(),
        );
        cx.spawn(async move |this, cx| {
            let n2 = n.clone();
            let found = cx
                .background_executor()
                .spawn(async move {
                    let v = st.instance(&n2).map(convert::instance);
                    let awake = v.as_ref().is_some_and(words::awake_now);
                    let task = v.as_ref().is_some_and(|v| v.kind == words::Kind::Task);
                    let dir = PathBuf::from(&st.dir);
                    if awake || task {
                        return None;
                    }
                    let shape = shape_now(&mode, &dir);
                    let ports: Vec<u16> = shape
                        .decls
                        .iter()
                        .filter(|d| d.name == n2)
                        .filter_map(decl_port)
                        .collect();
                    let looked = look(&hub, &ports, Vec::new());
                    Some((
                        preflight_instance(&n2, awake, task, &dir, &shape, &looked),
                        ports,
                    ))
                })
                .await;
            let _ = this.update(cx, |acts, cx| match found {
                Some((found, ports)) if found.blocked => {
                    let then = Then::Verb(n.clone(), verb);
                    cx.emit(ActsEvent::Check(Check {
                        key: k,
                        found,
                        then,
                        ports,
                        names: vec![n],
                    }));
                }
                _ => acts.run_verbs(&k, verb, vec![n], cx),
            });
        })
        .detach();
    }

    /// Goes ahead with what the sheet held back.
    pub(crate) fn go_ahead(&mut self, c: &Check, cx: &mut Context<Self>) {
        match &c.then {
            Then::Stack(a) => self.run_stack(&c.key, a.clone(), cx),
            Then::Verb(n, v) => self.run_verbs(&c.key, *v, vec![n.clone()], cx),
        }
    }

    /// A way out of the sheet.
    pub(crate) fn fix(&mut self, c: &Check, kind: &FixKind, cx: &mut Context<Self>) {
        match kind {
            FixKind::Edit(path) => cx.open_url(&format!("file://{path}")),
            FixKind::Repair => {
                cx.write_to_clipboard(ClipboardItem::new_string("doze trust".into()));
                self.say("copied doze trust: run it in a terminal (one password) to set this Mac's names up", false, cx);
            }
            FixKind::Remove => self.stack(&c.key, StackAction::Remove, cx),
            FixKind::Without(n) => {
                let names: Vec<String> = c.names.iter().filter(|x| *x != n).cloned().collect();
                self.run_stack(&c.key, StackAction::Up(names), cx);
            }
            FixKind::Stop(pid) => self.stop_then(c.clone(), *pid, cx),
        }
    }

    fn stop_then(&mut self, c: Check, pid: i64, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        let what = match &c.then {
            Then::Verb(n, _) => n.clone(),
            Then::Stack(_) => c.found.stack.clone(),
        };
        cx.spawn(async move |this, cx| {
            let ports = c.ports.clone();
            let done = cx
                .background_executor()
                .spawn(async move { stop_holder(&hub, pid, &ports, &what) })
                .await;
            let _ = this.update(cx, |a, cx| match done {
                Ok(()) => {
                    a.say(format!("stopped pid {pid}"), false, cx);
                    a.go_ahead(&c, cx);
                }
                Err(e) => a.say(e, true, cx),
            });
        })
        .detach();
    }
}

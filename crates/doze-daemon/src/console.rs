//! The `console` topic (Go's `daemon/console.go`): one instance's console, its spec and data actions first, then its
//! tables every 2 s and, when the subscriber names a stream panel, that panel's rows as they come. It never wakes the
//! instance: a sleeping one has a spec and no rows. Everything shown comes from the module's manifest and answers.

use crate::Daemon;
use doze_control::{
    ConsoleColumn, ConsoleExtension, ConsoleFrame, ConsoleNode, ConsolePanel, ConsoleRow,
    ConsoleSpec, ConsoleTab, Frame,
};
use doze_module_proto::msg::console as proto;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

/// How often a console's tables are read while someone looks (Go's `consoleGap`).
const GAP: Duration = Duration::from_secs(2);

type Done = Result<(), String>;

fn node(n: &proto::Node) -> ConsoleNode {
    ConsoleNode {
        kind: n.kind.clone(),
        id: n.id.clone(),
        title: n.title.clone(),
        weight: i64::from(n.weight),
        panel: n.panel.clone(),
        from: n.from.clone(),
        text: n.text.clone(),
        actions: n.actions.clone(),
        children: n.children.iter().map(node).collect(),
        keys: n.keys.clone(),
        by: n.by.clone(),
    }
}

/// A column as the wire shows it.
fn column(c: &proto::Column) -> ConsoleColumn {
    ConsoleColumn {
        key: c.key.clone(),
        title: c.title.clone(),
        align: c.align.clone(),
        role: c.role.clone(),
    }
}

/// Go's `specView`.
pub(crate) fn spec_view(s: Option<&proto::ConsoleSpec>) -> ConsoleSpec {
    let Some(s) = s else {
        return ConsoleSpec::default();
    };
    ConsoleSpec {
        front: s.front.clone().unwrap_or_default(),
        web_url: s.web_url.clone().unwrap_or_default(),
        panels: s
            .panels
            .iter()
            .map(|p| ConsolePanel {
                id: p.id.clone(),
                title: p.title.clone(),
                kind: p.kind.clone(),
                source: p.source.clone(),
                columns: p.columns.iter().map(column).collect(),
                select: p.select.clone().unwrap_or_default(),
            })
            .collect(),
        tabs: s
            .tabs
            .iter()
            .map(|t| ConsoleTab {
                id: t.id.clone(),
                title: t.title.clone(),
                body: node(&t.body),
            })
            .collect(),
        extend: s
            .extend
            .iter()
            .map(|e| ConsoleExtension {
                after: e.after.clone(),
                node: node(&e.node),
            })
            .collect(),
    }
}

/// Go's `tableRows`: the resources of a panel's kind as rows keyed by column; one the config does not declare (of a
/// kind it declares some of) is marked as made at run time.
pub(crate) fn table_rows(
    p: &proto::Panel,
    res: &[proto::Resource],
    declared: Option<&BTreeMap<String, BTreeSet<String>>>,
) -> Vec<ConsoleRow> {
    res.iter()
        .filter(|r| r.kind == p.source)
        .map(|r| {
            let mut cells = BTreeMap::from([
                ("name".to_string(), r.name.clone()),
                ("status".to_string(), r.status.clone()),
            ]);
            cells.extend(r.info.clone());
            if let Some(names) = declared.and_then(|d| d.get(&r.kind))
                && !names.contains(&r.name)
            {
                cells.insert(doze_control::CELL_RUNTIME.into(), "1".into());
            }
            ConsoleRow {
                cells: Some(cells),
                tone: String::new(),
            }
        })
        .collect()
}

struct Out<'a> {
    w: &'a crate::session::Writer,
    id: u64,
    name: String,
}

impl Out<'_> {
    fn emit(&self, mut f: ConsoleFrame) -> Done {
        f.instance.clone_from(&self.name);
        let frame = Frame {
            id: self.id,
            topic: "console".into(),
            console: Some(f),
            ..Frame::default()
        };
        crate::session::send(self.w, &frame).map_err(|e| e.to_string())
    }
}

fn state_of(d: &Daemon, name: &str) -> String {
    d.engine()
        .get(name)
        .map(|i| i.state.as_str().to_string())
        .unwrap_or_default()
}

fn running(d: &Daemon, name: &str) -> bool {
    d.engine().get(name).is_some_and(|i| i.state.running())
}

/// Go's `subConsole`.
pub(crate) async fn console(
    d: &Daemon,
    w: &crate::session::Writer,
    req: &doze_control::Request,
) -> Done {
    let [name] = req.names.as_slice() else {
        let out = Out {
            w,
            id: req.id,
            name: String::new(),
        };
        return out.emit(ConsoleFrame {
            error: "the console topic follows exactly one instance".into(),
            ..ConsoleFrame::default()
        });
    };
    let out = Out {
        w,
        id: req.id,
        name: name.clone(),
    };
    if d.engine().get(name).is_none() {
        return out.emit(ConsoleFrame {
            error: format!("instance {name:?} is not declared"),
            ..ConsoleFrame::default()
        });
    }
    let mi = d.module(name);
    let manifest = match &mi {
        Some(mi) => Some(mi.manifest().await?),
        None => None,
    };
    let admin = manifest
        .as_ref()
        .is_some_and(|m| m.caps & doze_module_proto::msg::hello::caps::ADMIN != 0);
    let spec = manifest.as_ref().and_then(|m| m.console.clone());
    out.emit(ConsoleFrame {
        spec: Some(spec_view(spec.as_ref())),
        actions: manifest
            .as_ref()
            .map(crate::admin::action_views)
            .unwrap_or_default(),
        state: state_of(d, name),
        ..ConsoleFrame::default()
    })?;
    let (Some(mi), Some(spec)) = (mi.filter(|_| admin), spec.filter(|s| !s.panels.is_empty()))
    else {
        return std::future::pending().await;
    };
    let _stream = stream(d, &out, &mi, req);
    loop {
        tables(d, &out, &mi, &spec).await?;
        tokio::time::sleep(GAP).await;
    }
}

/// The stream panel the subscriber named, while it runs; dropping the handle stops it.
fn stream(
    d: &Daemon,
    out: &Out<'_>,
    mi: &std::sync::Arc<doze_engine::modules::ModuleInstance>,
    req: &doze_control::Request,
) -> Option<AbortOnDrop> {
    if req.panel.is_empty() || !running(d, &out.name) {
        return None;
    }
    let (d, w, mi) = (d.clone(), out.w.clone(), mi.clone());
    let (id, name, panel, selector) = (
        out.id,
        out.name.clone(),
        req.panel.clone(),
        req.selector.clone(),
    );
    Some(AbortOnDrop(tokio::spawn(async move {
        let out = Out { w: &w, id, name };
        let ctx = d.engine().ctx_for(&out.name);
        let mut rows = match mi.tail(&ctx, &panel, &selector).await {
            Ok(s) => s,
            Err(error) => {
                let _ = out.emit(ConsoleFrame {
                    panel,
                    error,
                    ..ConsoleFrame::default()
                });
                return;
            }
        };
        while let Some(r) = rows.next::<proto::Row>().await {
            let f = match r {
                Ok(r) => ConsoleFrame {
                    panel: panel.clone(),
                    row: Some(ConsoleRow {
                        cells: Some(r.cells),
                        tone: r.tone,
                    }),
                    ..ConsoleFrame::default()
                },
                Err(e) => ConsoleFrame {
                    panel: panel.clone(),
                    error: e.describe(),
                    ..ConsoleFrame::default()
                },
            };
            if out.emit(f).is_err() {
                return;
            }
        }
    })))
}

/// One reading of every table and counters panel (or the state alone while it sleeps).
async fn tables(
    d: &Daemon,
    out: &Out<'_>,
    mi: &doze_engine::modules::ModuleInstance,
    spec: &proto::ConsoleSpec,
) -> Done {
    let state = state_of(d, &out.name);
    if !running(d, &out.name) {
        return out.emit(ConsoleFrame {
            state,
            ..ConsoleFrame::default()
        });
    }
    let ctx = d.engine().ctx_for(&out.name);
    let res = match mi.resources(&ctx).await {
        Ok(r) => r,
        Err(error) => {
            return out.emit(ConsoleFrame {
                state,
                error,
                ..ConsoleFrame::default()
            });
        }
    };
    let declared = mi.declared(&ctx).await;
    for p in spec
        .panels
        .iter()
        .filter(|p| p.kind == "table" || p.kind == "counters")
    {
        out.emit(ConsoleFrame {
            state: state.clone(),
            panel: p.id.clone(),
            rows: table_rows(p, &res, declared.as_ref()),
            ..ConsoleFrame::default()
        })?;
    }
    Ok(())
}

/// A task stopped when its handle goes.
struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

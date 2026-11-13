//! `doze-app shot` for M2's overlays: ⌘K with a query, a scope or a row done (`--query`, `--scope`, `--enter`), and
//! Needs you's surfaces (`--over bell|alerts|away`). An alert is a red item that appeared while the window was open,
//! and the away strip what happened while it was closed, so a shot stages them from the fixture's own items and
//! journal as the window would have met them.

use std::time::Duration;

use gpui::{Context, Window};

use crate::cli::{Screen, Shot};
use crate::overlays::Overlay;
use crate::shell::Root;

/// Sets ⌘K or Needs you up as the shot asks.
pub(crate) fn stage(root: &mut Root, window: &mut Window, cx: &mut Context<Root>, s: &Shot) {
    let stacks = root.hub_stacks();
    if s.screen == Screen::Palette
        && let Some(Overlay::Palette(p)) = root.overlay_now()
    {
        p.update(cx, |p, cx| {
            if let Some(sc) = s.scope.as_deref().and_then(|sc| scope_of(&stacks, sc)) {
                p.set_scope(sc, cx);
            }
            if let Some(q) = &s.query {
                p.type_in(q, window, cx);
            }
            if s.enter {
                p.choose(window, cx);
            }
        });
    }
    if s.machine_unset {
        let part = |section: &str, name: &str, fix: &str| doze_desk::attention::MachinePart {
            section: section.into(),
            name: name.into(),
            ok: false,
            text: String::new(),
            fix: fix.into(),
        };
        root.attn.parts = Some(vec![
            part("names", "network", "doze dns-setup"),
            part("domains", "route dev.shop.example", "doze trust"),
        ]);
        root.rebuild_attention(cx);
    }
    match s.over.as_deref() {
        Some("bell") => root.on_bell(&crate::actions::OpenBell, window, cx),
        Some("alerts") => {
            root.attn.alerts = root
                .attn
                .list
                .items
                .iter()
                .filter(|i| i.red && !i.fixed)
                .take(3)
                .cloned()
                .collect();
            cx.notify();
        }
        Some("away") => {
            let now = root.hub_now();
            let since = now
                .checked_sub(Duration::from_secs(3 * 3600 + 30 * 60))
                .unwrap_or(now);
            root.attn.away = doze_desk::attention::away::away_of(
                &stacks,
                since,
                now,
                doze_desk::page::time::Zone::UTC,
            );
            cx.notify();
        }
        _ => {}
    }
}

/// `--scope shop` or `--scope shop/db`: a stack by its name, and a service in it.
fn scope_of(stacks: &[doze_desk::StackView], sc: &str) -> Option<doze_desk::palette::Scope> {
    let (stack, name) = match sc.split_once('/') {
        Some((a, b)) => (a, Some(b.to_owned())),
        None => (sc, None),
    };
    let st = stacks.iter().find(|s| s.name == stack)?;
    Some(doze_desk::palette::Scope {
        key: st.key.dir().to_string_lossy().into_owned(),
        name,
    })
}

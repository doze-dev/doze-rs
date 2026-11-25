//! `doze-app shot --act …` for a console's working blocks: through the same calls a click makes, so a shot shows the
//! real tree, value viewer and form against whatever daemon feeds the window (a fixture's, or a real stack's with
//! `--home`). Acts chain with `|` (`shot_act::run`).
//!
//! - `open:TREE:ID,ID,=LEAF` opens folders in turn, then a leaf (`=`), in tree block `TREE`;
//! - `filter:TREE:WORDS` types into the tree's filter;
//! - `pick:TABLE:N` picks row `N` of table block `TABLE` (as sent);
//! - `wake:VALUE` presses "Open and wake it" on value block `VALUE`;
//! - `rest:VALUE` presses "Load the rest";
//! - `fold:VALUE:LINE` folds or opens the JSON container on line `LINE`;
//! - `keys:down,right,enter` presses keys in the tab (the block clicked last takes them);
//! - `sleep:NAME` and `up:NAME` put the instance to sleep and wake it, as `doze sleep` and `doze wake` do;
//! - `form:ACTION` opens the action's form; `fill:FIELD=TEXT;FIELD=TEXT` types into it; `submit` presses it.

use doze_desk::page::viewer::Viewer;
use gpui::{Context, Entity, Window};

use crate::console::ConsoleTabView;
use crate::shell::Root;

fn console(root: &Root, cx: &Context<Root>) -> Option<Entity<ConsoleTabView>> {
    root.inside_instance()?.read(cx).console_tab()
}

/// Does a block act; `None` when `act` is not one. Says how long to wait before the next.
pub(crate) fn start(
    root: &mut Root,
    act: &str,
    window: &mut Window,
    cx: &mut Context<Root>,
) -> Option<u64> {
    let (verb, rest) = act.split_once(':').unwrap_or((act, ""));
    match verb {
        "fill" => {
            let f = root.form()?;
            for pair in rest.split(';') {
                if let Some((k, v)) = pair.split_once('=') {
                    f.update(cx, |f, cx| {
                        f.type_in(k, &v.replace("\\n", "\n"), window, cx)
                    });
                }
            }
            return Some(200);
        }
        "keys" => return press_keys(root, rest, window, cx),
        "sleep" => {
            daemon_verb(root, doze_control::Verb::Sleep, rest);
            return Some(3000);
        }
        "up" => {
            daemon_verb(root, doze_control::Verb::Wake, rest);
            return Some(4000);
        }
        "submit" => {
            root.form()?.update(cx, |f, cx| f.submit(cx));
            return Some(1500);
        }
        "open" | "filter" | "pick" | "wake" | "rest" | "fold" | "form" => {}
        _ => return None,
    }
    let c = console(root, cx)?;
    let (block, arg) = rest.split_once(':').unwrap_or((rest, ""));
    let (block, arg) = (block.to_owned(), arg.to_owned());
    c.update(cx, |c, cx| match verb {
        "open" => open(c, &block, &arg, cx),
        "filter" => filter(c, &block, &arg, window, cx),
        "pick" => {
            if let Ok(at) = arg.parse::<usize>() {
                c.pick(&block, at, cx);
            }
        }
        "wake" => c.value_ask(&block, Viewer::open_waking, cx),
        "rest" => c.value_ask(&block, Viewer::rest, cx),
        "fold" => {
            if let (Ok(line), Some(s)) = (arg.parse::<usize>(), c.work.values.get_mut(&block))
                && !s.folded.remove(&line)
            {
                s.folded.insert(line);
            }
            cx.notify();
        }
        "form" => {
            let a = c.offered().into_iter().find(|a| a.id == block);
            if let Some(a) = a {
                c.act(a, cx);
            }
        }
        _ => {}
    });
    Some(if verb == "wake" { 6000 } else { 1200 })
}

/// Opens each folder of `ids` in turn (not one open already), then the leaf marked `=`.
fn open(c: &mut ConsoleTabView, tree: &str, ids: &str, cx: &mut Context<ConsoleTabView>) {
    for id in ids.split(',').filter(|s| !s.is_empty()) {
        match id.strip_prefix('=') {
            Some(leaf) => c.open_leaf(tree, leaf, cx),
            None => {
                let open = c
                    .work
                    .trees
                    .get(tree)
                    .is_some_and(|t| t.state.open.contains(id));
                if !open {
                    c.toggle_folder(tree, id, cx);
                }
            }
        }
    }
}

/// Types `words` into the tree's filter, as the keys would.
fn filter(
    c: &mut ConsoleTabView,
    tree: &str,
    words: &str,
    window: &mut Window,
    cx: &mut Context<ConsoleTabView>,
) {
    if let Some(i) = c.work.trees.get(tree).and_then(|t| t.filter.clone()) {
        i.update(cx, |i, cx| i.set_value(words.to_owned(), window, cx));
    }
    let f = c
        .work
        .trees
        .get_mut(tree)
        .and_then(|t| t.state.set_filter(words));
    if let Some(f) = f {
        c.fetch_children(tree, f, cx);
    }
}

/// Keys pressed in the console tab, the block clicked last taking them: `keys:down,down,right,enter`.
fn press_keys(
    root: &mut Root,
    keys: &str,
    window: &mut Window,
    cx: &mut Context<Root>,
) -> Option<u64> {
    let c = console(root, cx)?;
    let f = c.read(cx).focus.clone();
    window.focus(&f, cx);
    for k in keys.split(',') {
        let a: Box<dyn gpui::Action> = match k {
            "down" => Box::new(crate::actions::RowDown),
            "up" => Box::new(crate::actions::RowUp),
            "right" => Box::new(crate::actions::FolderOpen),
            "left" => Box::new(crate::actions::FolderClose),
            "enter" => Box::new(crate::actions::OpenLeaf),
            _ => continue,
        };
        window.dispatch_action(a, cx);
    }
    Some(1500)
}

/// The instance put to sleep or woken by the daemon itself, as `doze sleep` and `doze wake` would (no question asked).
fn daemon_verb(root: &Root, v: doze_control::Verb, name: &str) {
    if let Some(k) = root.key().cloned()
        && let Err(e) = root.hub().verb(&k, v, name)
    {
        eprintln!("doze-app shot: {} {name}: {e}", v.as_str());
    }
}

//! ⌘K, over the page (M2 step 2.3): `doze_desk::palette`'s model, held to Go's goldens, drawn as the Palette boards
//! draw it. Nothing typed: what needs you, commands for where you are, what you did lately, every stack. A name, a
//! name then words, a verb then a name; "Did you mean"; the closest name when nothing matched. Tab narrows to a stack
//! or a service (its crumbs in the box), ⌫ on an empty box or ⇧Tab steps out, esc clears, then steps out, then closes.
//! A copy says "Copied ✓" in place and stays open. ⌘⏎ does the highlighted row's first action.

use doze_desk::Hub;
use doze_desk::palette::{
    self as model, Ask, DoKind, Doing, RecentCommand, RecentService, Row, Scope,
};
use gpui::{Context, Entity, EventEmitter, ScrollHandle, Window, prelude::*};
use gpui_component::input::{InputEvent, InputState};

/// The action table's rows offered in ⌘K, as palette rows: the label, what it does, its key; ⏎ runs the action
/// (`do:<label>`). One table drives the keys, the menus, the help sheet and these. A row the model already offers in
/// its own words ("Restart shop", "Edit shop's doze.hcl", "Keyboard shortcuts") has `palette: false`, so ⌘K lists
/// each command once.
fn table_rows() -> Vec<Row> {
    crate::actions::palette()
        .filter(|s| s.built)
        .map(|s| Row {
            glyph: "›",
            tone: "dim",
            label: s.label.to_owned(),
            note: s.what.to_owned(),
            key: crate::actions::chip(s).unwrap_or_default(),
            doing: Doing {
                kind: DoKind::App,
                text: format!("do:{}", s.label),
                ..Doing::default()
            },
            ..Row::default()
        })
        .collect()
}

/// What the palette asks its owner to do: the row's doing, after it closes.
pub(crate) enum PaletteEvent {
    Do(Doing),
}

/// The palette.
pub(crate) struct Palette {
    pub(crate) input: Entity<InputState>,
    hub: Hub,
    /// The stack on screen (its key) and the service on screen, whose commands come first.
    current: String,
    inst: String,
    pub(super) scope: Option<Scope>,
    recent: Vec<RecentService>,
    cmds: Vec<RecentCommand>,
    pub(super) model: model::Palette,
    /// The highlighted row, counting every section's rows.
    pub(super) on: usize,
    /// The row just copied, by label: it says "Copied ✓" in place.
    pub(super) copied: Option<String>,
    pub(super) scroll: ScrollHandle,
    /// The action table's ⌘K rows (what Go's list does not have), each doing its action.
    extra: Vec<Row>,
    /// The box's placeholder as last set.
    pub(super) hint: String,
    /// What the window knows beyond the stacks (Go `Service.Palette`): the stopped stacks whose doze.hcl does not
    /// load, by key, and Needs you's items.
    told: Told,
}

/// What the window tells the palette beyond the stacks.
#[derive(Default)]
pub(crate) struct Told {
    /// Stopped stacks whose doze.hcl does not load, by key: their Up rows open their page.
    pub(crate) broken: std::collections::BTreeSet<String>,
    /// Needs you's list: its red stack-level items join "Needs you" on the empty palette.
    pub(crate) needs: Vec<doze_desk::attention::Item>,
}

impl EventEmitter<PaletteEvent> for Palette {}

impl Palette {
    /// A palette over every stack, `current` and `inst` naming where the person is.
    pub(crate) fn new(
        hub: Hub,
        current: String,
        inst: String,
        told: Told,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("A service, a stack, or what to do…")
        });
        cx.subscribe(&input, |this: &mut Palette, _, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                this.copied = None;
                this.refresh(cx);
            }
        })
        .detach();
        input.update(cx, |i, cx| i.focus(window, cx));
        let mut p = Palette {
            input,
            hub,
            current,
            inst,
            scope: None,
            recent: crate::prefs::recall::recent(),
            cmds: crate::prefs::recall::commands(),
            model: model::Palette::default(),
            on: 0,
            copied: None,
            scroll: ScrollHandle::new(),
            hint: String::new(),
            extra: table_rows(),
            told,
        };
        p.compute(cx);
        p
    }

    fn query(&self, cx: &gpui::App) -> String {
        self.input.read(cx).value().to_string()
    }

    fn compute(&mut self, cx: &gpui::App) {
        let stacks = self.hub.stacks();
        let q = self.query(cx);
        let ask = Ask {
            current: &self.current,
            inst: &self.inst,
            scope: self.scope.as_ref(),
            recent: &self.recent,
            cmds: &self.cmds,
            query: &q,
            now: self.hub.now(),
            extra: &self.extra,
        };
        self.model = model::palette(&stacks, &ask);
        model::broken_up(&mut self.model, &self.told.broken);
        if q.trim().is_empty() && self.scope.is_none() {
            model::stack_needs(&mut self.model, &self.told.needs);
        }
        self.on = 0;
        self.scroll.scroll_to_item(0);
    }

    /// Reads the stacks and the box again.
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        self.compute(cx);
        cx.notify();
    }

    /// The highlighted row.
    pub(super) fn row(&self) -> Option<&Row> {
        self.model.rows().nth(self.on)
    }

    /// Moves the highlight, keeping it in view.
    pub(crate) fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        let n = self.model.rows().count();
        if n > 0 {
            self.on = self.on.saturating_add_signed(by).min(n - 1);
            self.scroll.scroll_to_item(self.child_of(self.on));
            cx.notify();
        }
    }

    /// The body's child index of row `i`: each section's heading is a child before its rows.
    fn child_of(&self, i: usize) -> usize {
        let mut seen = 0;
        for (s, sec) in self.model.sections.iter().enumerate() {
            if i < seen + sec.rows.len() {
                return i + s + 1;
            }
            seen += sec.rows.len();
        }
        i
    }

    /// ⏎: the highlighted row; with nothing found, the closest name (into the box, or its page).
    pub(crate) fn choose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(r) = self.row().cloned() {
            self.run(&r, r.doing.clone(), cx);
            return;
        }
        let Some(n) = self.model.nothing.clone() else {
            return;
        };
        if !n.fill.is_empty() {
            self.input
                .update(cx, |i, cx| i.set_value(n.fill.clone(), window, cx));
            self.refresh(cx);
        } else if let Some(d) = n.doing {
            cx.emit(PaletteEvent::Do(d));
        }
    }

    /// Does `doing` for row `r`: a copy in place, anything else by the owner. A command is remembered for Recent.
    fn run(&mut self, r: &Row, doing: Doing, cx: &mut Context<Self>) {
        if r.enter.is_none() {
            let mut kept = RecentCommand::of(r);
            kept.doing = doing.clone();
            crate::prefs::recall::remember(kept.clone());
            self.cmds = doze_desk::palette::remember(&self.cmds, kept);
        }
        if doing.kind == DoKind::Copy {
            crate::outward::copy(cx, &doing.text);
            self.copied = Some(r.label.clone());
            cx.notify();
            return;
        }
        cx.emit(PaletteEvent::Do(doing));
    }

    /// ⌘⏎: the highlighted row's first action.
    pub(crate) fn first_act(&mut self, cx: &mut Context<Self>) {
        if let Some(r) = self.row().cloned()
            && let Some(a) = r.first_act()
        {
            self.run(&r, a.doing.clone(), cx);
        }
    }

    /// Tab: into the highlighted stack or service. False when the row is a command.
    pub(crate) fn scope_in(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(enter) = self.row().and_then(|r| r.enter.clone()) else {
            return false;
        };
        self.scope = Some(enter);
        self.copied = None;
        self.input.update(cx, |i, cx| i.set_value("", window, cx));
        self.refresh(cx);
        true
    }

    /// One step out of the scope. False at the top.
    pub(crate) fn scope_out(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(sc) = self.scope.take() else {
            return false;
        };
        self.scope = sc.out();
        self.copied = None;
        self.refresh(cx);
        true
    }

    /// ⌫: steps out when the box is empty; otherwise the box's own.
    pub(crate) fn backspace(&mut self, cx: &mut Context<Self>) -> bool {
        self.query(cx).is_empty() && self.scope_out(cx)
    }

    /// Esc: clears the box, else steps out, else (false) the palette closes.
    pub(crate) fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.query(cx).is_empty() {
            self.input.update(cx, |i, cx| i.set_value("", window, cx));
            self.refresh(cx);
            return true;
        }
        self.scope_out(cx)
    }

    /// The row's place in the list was clicked.
    pub(super) fn click(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.on = i;
        self.choose(window, cx);
    }

    /// Puts `q` in the box (a shot's query).
    #[cfg(feature = "capture")]
    pub(crate) fn type_in(&mut self, q: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.input
            .update(cx, |i, cx| i.set_value(q.to_owned(), window, cx));
        self.refresh(cx);
    }

    /// Narrows to `scope` (a shot's).
    #[cfg(feature = "capture")]
    pub(crate) fn set_scope(&mut self, scope: Scope, cx: &mut Context<Self>) {
        self.scope = Some(scope);
        self.refresh(cx);
    }
}

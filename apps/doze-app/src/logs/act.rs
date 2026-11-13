//! What the logs page does when asked: step through the hits, copy the lines, let go on Esc, the keys it answers,
//! and (for `shot`) a page set up as asked.

use gpui::{ClipboardItem, Context, Focusable as _, Window, prelude::*};

use super::LogsView;
use crate::actions::{
    CopyShown, FocusFilter, LevelDebug, LevelError, LevelInfo, LevelWarn, LogsEnd, LogsTop,
    NewerRun, NextHit, OlderRun, PrevHit, ToggleClock, ToggleFind, ToggleFollow,
};

impl LogsView {
    /// Steps to the next hit (`back`: the one before), scrolling it into view.
    pub(crate) fn step_hit(&mut self, back: bool, cx: &mut Context<Self>) {
        if !self.find {
            return;
        }
        if let Some((row, at)) = self.index.next_hit(self.hit.map(|h| h.0), back) {
            self.hit = Some((row, at));
            self.follow = false;
            self.scroll
                .scroll_to_item(row, gpui::ScrollStrategy::Center);
            cx.notify();
        }
    }

    /// Copies the picked lines, else the lines in view, as they read.
    pub(crate) fn copy_shown(&mut self, cx: &mut Context<Self>) {
        let range = match self.picked {
            Some((a, b)) => {
                let (a, b) = (self.index.row_of(a.min(b)), self.index.row_of(a.max(b)));
                match (a, b) {
                    (Some(a), Some(b)) => a..b + 1,
                    _ => self.drawn.clone(),
                }
            }
            None => self.drawn.clone(),
        };
        let (idx, now, zone, clock) = (&self.index, self.now, self.zone, self.clock);
        let text = self
            .hub
            .with_logs(&self.key, |r| {
                idx.rows(r, range, now, zone)
                    .iter()
                    .map(|l| l.copied(clock))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        cx.notify();
    }

    /// Sets the page up as `shot` asks: a level, words in the filter box, find mode (stepped to the first hit).
    #[cfg(feature = "capture")]
    pub(crate) fn stage(
        &mut self,
        level: Option<u8>,
        filter: Option<&str>,
        find: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(l) = level {
            self.level = l.min(3);
        }
        self.find = find;
        if let Some(f) = filter {
            f.clone_into(&mut self.filter_text);
            let input = self.input(window, cx);
            input.update(cx, |i, cx| i.set_value(f.to_owned(), window, cx));
        }
        let small = self
            .hub
            .with_logs(&self.key, |r| r.len() < 5_000)
            .unwrap_or(true);
        self.rebuild(cx);
        if small && find {
            self.step_hit(false, cx);
        }
    }

    /// Shows only the lines with `words` in them (⌘K's "Search api's logs for …").
    pub(crate) fn search(&mut self, words: &str, window: &mut Window, cx: &mut Context<Self>) {
        words.clone_into(&mut self.filter_text);
        let input = self.input(window, cx);
        input.update(cx, |i, cx| i.set_value(words.to_owned(), window, cx));
        self.rebuild(cx);
    }

    /// Esc: out of the filter box, else the picked lines go; says whether it did either.
    pub(crate) fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self
            .input
            .as_ref()
            .is_some_and(|i| i.read(cx).focus_handle(cx).is_focused(window))
        {
            window.focus(&self.focus, cx);
            return true;
        }
        if self.picked.take().is_some() {
            cx.notify();
            return true;
        }
        false
    }

    pub(super) fn actions(
        &self,
        d: gpui::Stateful<gpui::Div>,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        d.on_action(cx.listener(|v, _: &FocusFilter, w, cx| {
            let i = v.input(w, cx);
            i.update(cx, |i, cx| i.focus(w, cx));
        }))
        .on_action(cx.listener(|v, _: &LevelDebug, _, cx| v.set_level(0, cx)))
        .on_action(cx.listener(|v, _: &LevelInfo, _, cx| v.set_level(1, cx)))
        .on_action(cx.listener(|v, _: &LevelWarn, _, cx| v.set_level(2, cx)))
        .on_action(cx.listener(|v, _: &LevelError, _, cx| v.set_level(3, cx)))
        .on_action(cx.listener(|v, _: &CopyShown, _, cx| v.copy_shown(cx)))
        .on_action(cx.listener(|v, _: &ToggleClock, _, cx| {
            v.clock = !v.clock;
            cx.notify();
        }))
        .on_action(cx.listener(|v, _: &ToggleFollow, _, cx| v.set_follow(!v.follow, cx)))
        .on_action(cx.listener(|v, _: &OlderRun, _, cx| v.set_run_back(v.run_back + 1, cx)))
        .on_action(
            cx.listener(|v, _: &NewerRun, _, cx| v.set_run_back(v.run_back.saturating_sub(1), cx)),
        )
        .on_action(cx.listener(|v, _: &ToggleFind, _, cx| {
            v.find = !v.find;
            v.rebuild(cx);
        }))
        .on_action(cx.listener(|v, _: &NextHit, _, cx| v.step_hit(false, cx)))
        .on_action(cx.listener(|v, _: &PrevHit, _, cx| v.step_hit(true, cx)))
        .on_action(cx.listener(|v, _: &LogsTop, _, cx| {
            v.follow = false;
            v.scroll.scroll_to_item(0, gpui::ScrollStrategy::Top);
            cx.notify();
        }))
        .on_action(cx.listener(|v, _: &LogsEnd, _, cx| v.set_follow(true, cx)))
    }
}

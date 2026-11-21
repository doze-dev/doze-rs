//! The page's read: the journal laid out off the UI thread, in the grain the window reads in (`shell::clock`).

use std::collections::HashMap;

use doze_desk::activity::{Activity, Ask, Span, activity_of, events_of};
use gpui::Context;

use super::{ActivityView, remember_ram};

impl ActivityView {
    /// Reads the journal and lays it out, off the UI thread; the page keeps what it shows until the answer lands.
    pub(crate) fn read(&mut self, cx: &mut Context<Self>) {
        let Some(st) = self.hub.stack(&self.key) else {
            return;
        };
        let ram = remember_ram(&st, cx);
        self.asked += 1;
        let (n, span, only, now) = (self.asked, self.span, self.only.clone(), self.hub.now());
        let zone = crate::instance::zone();
        // Glanced at, its ages to the minute, as the rest of the window (`shell::clock`); it is read once a minute then.
        let glanced = crate::shell::attention::mode(cx.background_executor().now())
            == crate::shell::attention::Mode::Glance;
        let job = cx.background_executor().spawn(async move {
            if glanced {
                return doze_display::coarse(|| activity_now(&st, span, &only, &ram, now, zone)).0;
            }
            activity_now(&st, span, &only, &ram, now, zone)
        });
        cx.spawn(async move |this, cx| {
            let a = job.await;
            let _ = this.update(cx, |v, cx| {
                if v.asked == n {
                    v.landed_model(a, cx);
                }
            });
        })
        .detach();
    }
}

/// The page laid out from the stack's journal, over `span` ending at `now` (off the UI thread).
fn activity_now(
    st: &doze_desk::StackView,
    span: Span,
    only: &str,
    ram: &HashMap<String, i64>,
    now: std::time::SystemTime,
    zone: doze_desk::page::time::Zone,
) -> Activity {
    let events = events_of(st);
    let ask = Ask {
        span,
        inst: only,
        ram,
        now,
        zone,
    };
    activity_of(st, events, ask)
}

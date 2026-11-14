//! The page's clock: what changes with time alone (a countdown, meters that may go quiet) is read again once a second
//! while the window is in use (`shell::attention`), and only then.

use std::time::Duration;

use gpui::Context;

use super::InstanceView;

#[cfg(test)]
thread_local! {
    /// How many times an instance page's clock has read the page again, for the idle tests.
    pub(crate) static TICKS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

impl InstanceView {
    /// Something on the page changes with time alone: a countdown (dozing, the next try), or meters that may go quiet.
    fn counting(&self) -> bool {
        self.page.dozing.is_some()
            || self.metered
            || self
                .page
                .crash
                .as_ref()
                .is_some_and(|c| !c.next_in.is_empty())
    }

    /// Repaints once a second while a countdown shows and the window is in use (`shell::attention`), and only then.
    /// One loop at a time: `ticking` stays set while the loop's own read runs, so that read cannot start another.
    pub(super) fn tick(&mut self, cx: &mut Context<Self>) {
        let used = crate::shell::attention::attended(cx.background_executor().now());
        if !self.counting() || self.ticking || !used {
            return;
        }
        self.ticking = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let more = this.update(cx, |v, cx| {
                    #[cfg(test)]
                    TICKS.with(|t| t.set(t.get() + 1));
                    v.refresh(cx);
                    let used = crate::shell::attention::attended(cx.background_executor().now());
                    let more = v.counting() && used;
                    v.ticking = more;
                    more
                });
                if !matches!(more, Ok(true)) {
                    return;
                }
            }
        })
        .detach();
    }
}

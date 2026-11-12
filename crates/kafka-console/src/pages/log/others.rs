//! "Other logs on this broker": the topics whose log has a story, each with the way to its own Log page.

use maud::{Markup, html};

use super::log_url;
use crate::base::Base;
use crate::model::{Cleanup, LogOtherLink, LogView};

fn policy(c: Cleanup) -> &'static str {
    match c {
        Cleanup::Delete => "delete",
        Cleanup::Compact => "compact",
        Cleanup::Both => "compact,delete",
    }
}

fn button(l: LogOtherLink) -> &'static str {
    match l {
        LogOtherLink::NotDue => "Why not yet?",
        LogOtherLink::Retention => "Retention",
        LogOtherLink::Empty => "Empty log",
        LogOtherLink::Plain => "Log",
    }
}

/// The section; nothing when there is no other topic.
#[must_use]
pub(super) fn others(v: &LogView, base: &Base) -> Markup {
    if v.others.is_empty() {
        return html! {};
    }
    html! {
        div class="sec" {
            h2 { "Other logs on this broker" }
            p class="lede" { "The Log page of the other topics, with what is worth knowing of each." }
            table class="t dt-t" {
                colgroup { col style="width:28%"; col style="width:18%"; col; col style="width:10%"; }
                thead { tr { th scope="col" { "Topic" } th scope="col" { "Policy" } th scope="col" { "Its log" } th scope="col" {} } }
                tbody {
                    @for o in &v.others {
                        tr {
                            td class="name mono" { (o.topic) }
                            td { (policy(o.cleanup)) }
                            td { (o.line) @if let Some(s) = &o.sub { span class="sub" { (s) } } }
                            td class="r" { a class="btn sm" href=(log_url(base, &o.topic, 0)) { (button(o.link)) } }
                        }
                    }
                }
            }
        }
    }
}

//! What an open transaction holds back: the section of the Producers page, and the page that says who is held (`txn-holding`).

use maud::{Markup, html};

use super::tail::ruler;
use super::txn::{txn_head, txn_tabs};
use super::txn_url;
use crate::base::{Base, segment};
use crate::format::number;
use crate::model::{ProducersView, TxnView};
use crate::producers_words::holding_sentence;
use crate::ui::glyph::Glyph;
use crate::ui::notice::{Tone, callout};
use crate::ui::write::spans;

/// `/topics/orders/transactions/orders-eos/held`.
#[must_use]
pub fn txn_held_url(base: &Base, topic: &str, id: &str) -> String {
    format!("{}/held", txn_url(base, topic, id))
}

/// `What the open transaction holds back` on the Producers page.
#[must_use]
pub(super) fn section(v: &ProducersView, base: &Base) -> Markup {
    let x = v.open.first();
    html! {
        div class="sec" {
            div class="row sb" {
                h2 { "What the open transaction holds back" }
                @if let Some(x) = x { a class="btn sm" href=(txn_url(base, &v.topic, &x.id)) { "Open the transaction" } }
            }
            p class="lede" {
                "The tail of " (v.topic) "-" (v.partition) ". Records from " (number(v.last_stable)) " belong to "
                @if let Some(x) = x { span class="mono" { (x.id) } } @else { "an open transaction" } ", still undecided."
            }
            (ruler(&v.tail, v.last_stable, v.end))
        }
    }
}

/// The page that says who is held: the two isolation levels side by side and what a Fetch answers.
#[must_use]
pub fn txn_held_page(v: &TxnView, base: &Base) -> Markup {
    let p = v.parts.iter().find(|p| p.partition == v.tail_partition);
    let (end, stable) = p.map_or((0, 0), |p| (p.end, p.last_stable));
    let messages = format!(
        "{}?reading=committed&partition={}",
        base.url(&format!("/topics/{}/messages", segment(&v.topic))),
        v.tail_partition
    );
    html! {
        (txn_head(v, base, spans(&holding_sentence(v)), html! {
            a class="btn" href=(messages) { "The same, as a consumer sees it" }
        }))
        (txn_tabs(v, base))
        div class="g2" {
            div class="card" { h2 { "read_uncommitted" } p class="lede" { "The default of most clients." }
                dl class="kv sm" {
                    dt { "Reads up to" } dd class="mono" { (number(end - 1)) }
                    dt { "Sees" } dd { "every record, including the " (v.held()) " of an undecided transaction" }
                } }
            div class="card" { h2 { "read_committed" } p class="lede" { "Exactly-once consumers." }
                dl class="kv sm" {
                    dt { "Reads up to" } dd class="mono" { (number(stable - 1)) }
                    dt { "Waits for" } dd { "the transaction to commit (then sees " (v.held()) " more) or abort (then skips them)" }
                } }
        }
        div class="sec" style="margin-top:28px" {
            h2 { "What a Fetch answers on " (v.topic) "-" (v.tail_partition) }
            p class="lede" { "The last stable offset in a Fetch answer is the real one at every isolation level." }
            table class="t" {
                thead { tr { th scope="col" { "Field" } th scope="col" { "Value" } } }
                tbody {
                    tr { td class="mono" { "high_watermark" } td class="mono" { (number(end)) } }
                    tr { td class="mono" { "last_stable_offset" } td class="mono" { (number(stable)) } }
                    tr { td class="mono" { "ListOffsets latest, read_committed" } td class="mono" { (number(stable)) } }
                }
            }
        }
        div style="margin-top:24px" {
            (callout(Tone::Plain, Glyph::Info, html! {
                "Which group's fetches are read_committed is a property of each Fetch, not of the group. The wire shows it per request; "
                a class="lnk" href=(base.url("/live")) { "see the wire" } "."
            }))
        }
    }
}

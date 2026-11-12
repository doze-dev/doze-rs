//! What the stream watches, page by page: the decision, and the reason (docs/port/console/live-updates.md).
//!
//! A page is calm unless a person watches it change as it happens. A calm page is fetched when it is opened, when the person asks
//! (Refresh, a filter), and nothing is swapped under their eyes; its relative times ("4 s ago") are kept by the browser
//! (`console.js`, `rel`). Only these fragments are live:
//! the Live tape and the tapes of a topic and a group (a tape is what Live is for), Home's status and attention, the callout of a
//! rebalancing group on the list, and the Now card of a group's Story (the countdown of a rebalance).

/// Home: the status, the attention callout (an open transaction's timeout runs here), the figures.
pub const HOME: &[&str] = &["home-live"];
/// The topics list: calm.
pub const TOPICS: &[&str] = &[];
/// A topic's Messages page, header and table: calm (the wire dock is on the Live page).
pub const TOPIC: &[&str] = &[];
/// The groups list: only the line about a rebalancing group (its countdown); the table is calm.
pub const GROUPS: &[&str] = &["groups-live"];
/// A group's page and its Lag tab: calm.
pub const GROUP: &[&str] = &[];
/// A group's Story: the Now card is a countdown; the sentence is calm.
pub const STORY: &[&str] = &["story-now"];

/// Text of a page with its relative times ("4 s ago", "0.2 s ago") made alike: a fragment's ETag must not change because time passed.
#[must_use]
pub fn without_relative_times(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = rest.find(" ago") {
        let (before, after) = rest.split_at(at);
        // Walk back over `<number> <unit>` (and a closing `</b>` or `</strong>` between the unit and " ago").
        let mut head = before;
        for close in ["</b>", "</strong>"] {
            head = head.strip_suffix(close).unwrap_or(head);
        }
        let cut = head
            .strip_suffix(['s', 'm', 'h', 'd'])
            .and_then(|h| h.strip_suffix(' '))
            .and_then(|h| {
                let n = h.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
                (n.len() < h.len()).then_some(n.len())
            });
        match cut {
            Some(n) => {
                out.push_str(&before[..n]);
                out.push('T');
            }
            None => out.push_str(before),
        }
        out.push_str(" ago");
        rest = &after[4..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_time_is_the_same_whatever_it_says() {
        let a = without_relative_times(
            "<p>last written 4 s ago, to x; <b>0.2 s</b> ago; 12 m ago; 3 d ago</p>",
        );
        let b = without_relative_times(
            "<p>last written 12 s ago, to x; <b>1.9 s</b> ago; 13 m ago; 4 d ago</p>",
        );
        assert_eq!(a, b, "{a}");
    }

    #[test]
    fn what_is_not_a_relative_time_is_left_alone() {
        for s in [
            "<p>a long ago</p>",
            "<p>5 messages ago</p>",
            "<p>x ago</p>",
            "<p>3 s</p>",
            "",
        ] {
            assert_eq!(without_relative_times(s), s);
        }
        assert_ne!(
            without_relative_times("<p>3 records</p>"),
            without_relative_times("<p>4 records</p>"),
            "a counter is data"
        );
    }
}

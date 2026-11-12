//! The calls the Groups forms send and the answers they get, written as the boards draw them: a name, then the values. Pure
//! functions from the values of a form to lines of tokens. What a person typed reaches these lines, so group and topic names go
//! through [`visible`](crate::calls::visible) first.

use crate::calls::visible;
use crate::model::{Lines, Refusal, Tok};

type Line = Vec<(Tok, String)>;

fn name(s: &str) -> String {
    visible(s, 80)
}

fn answer(r: &Refusal) -> Line {
    vec![(Tok::Comment, format!("answer: {} ({})", r.name, r.code))]
}

fn keyword(api: &str, version: i16) -> Line {
    vec![
        (Tok::Keyword, api.to_owned()),
        (Tok::Plain, format!(" v{version}")),
    ]
}

fn pairs(topic: &str, offsets: &[(i32, i64)]) -> String {
    let t = name(topic);
    let shown: Vec<String> = offsets
        .iter()
        .take(8)
        .map(|(p, o)| format!("{t}-{p}={o}"))
        .collect();
    if offsets.len() > 8 {
        format!("{} ... ({} partitions)", shown.join(" "), offsets.len())
    } else {
        shown.join(" ")
    }
}

/// The preview of a reset: how the targets were asked, then the commit.
#[must_use]
pub fn reset(version: i16, group: &str, topic: &str, asked: &str, offsets: &[(i32, i64)]) -> Lines {
    let mut lines: Lines = Vec::new();
    if !asked.is_empty() {
        lines.push(vec![
            (Tok::Comment, "ListOffsets".to_owned()),
            (Tok::Plain, format!(" {} {asked}", name(topic))),
        ]);
    }
    lines.push(vec![
        (Tok::Comment, "OffsetCommit".to_owned()),
        (
            Tok::Plain,
            format!(" v{version} group={} generation=-1", name(group)),
        ),
    ]);
    lines.push(vec![(Tok::Plain, format!("  {}", pairs(topic, offsets)))]);
    lines
}

/// A reset that Kafka refused, with the answer.
#[must_use]
pub fn reset_refused(
    version: i16,
    group: &str,
    topic: &str,
    offsets: &[(i32, i64)],
    r: &Refusal,
) -> Lines {
    vec![
        keyword("OffsetCommit", version),
        vec![(Tok::Plain, format!("  group={}", name(group)))],
        vec![(Tok::Plain, "  generation=-1 member=\"\"".to_owned())],
        vec![(Tok::Plain, format!("  {}", pairs(topic, offsets)))],
        answer(r),
    ]
}

/// DeleteGroups, with the answer if it was refused.
#[must_use]
pub fn delete_group(version: i16, group: &str, refused: Option<&Refusal>) -> Lines {
    let mut lines = vec![
        keyword("DeleteGroups", version),
        vec![(Tok::Plain, format!("  groups=[{}]", name(group)))],
    ];
    if let Some(r) = refused {
        lines.push(answer(r));
    }
    lines
}

/// OffsetDelete for the partitions of some topics.
#[must_use]
pub fn offset_delete(group: &str, topics: &[(String, Vec<i32>)]) -> Lines {
    let mut lines: Lines = vec![vec![
        (Tok::Comment, "OffsetDelete".to_owned()),
        (Tok::Plain, format!(" group={}", name(group))),
    ]];
    for (t, ps) in topics {
        let list: Vec<String> = ps.iter().take(16).map(ToString::to_string).collect();
        let more = if ps.len() > 16 { ", ..." } else { "" };
        lines.push(vec![(
            Tok::Plain,
            format!("  {} = [{}{more}]", name(t), list.join(", ")),
        )]);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(l: &Lines) -> String {
        l.iter()
            .map(|line| line.iter().map(|(_, s)| s.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn a_reset_names_the_group_with_no_generation_and_every_partition_it_moves() {
        let t = text(&reset(
            9,
            "analytics",
            "orders",
            "latest",
            &[(0, 16_104), (1, 16_051)],
        ));
        assert_eq!(
            t,
            "ListOffsets orders latest\nOffsetCommit v9 group=analytics generation=-1\n  orders-0=16104 orders-1=16051"
        );
    }

    #[test]
    fn a_long_reset_is_cut_and_a_hostile_name_cannot_make_a_second_line() {
        let many: Vec<(i32, i64)> = (0..20).map(|p| (p, 1)).collect();
        let t = text(&reset(9, "g\nevil", "t", "", &many));
        assert!(t.contains("(20 partitions)"), "{t}");
        assert!(t.contains("g\\nevil"), "{t}");
        assert_eq!(t.lines().count(), 2, "{t}");
    }

    #[test]
    fn a_refused_call_ends_with_kafkas_code() {
        let r = crate::groups_words::kafka_refusal(68);
        let t = text(&delete_group(2, "search-indexer", Some(&r)));
        assert_eq!(
            t,
            "DeleteGroups v2\n  groups=[search-indexer]\nanswer: NON_EMPTY_GROUP (68)"
        );
        let t = text(&reset_refused(
            9,
            "g",
            "clickstream",
            &[(0, 5)],
            &crate::groups_words::kafka_refusal(25),
        ));
        assert!(t.ends_with("answer: UNKNOWN_MEMBER_ID (25)"), "{t}");
    }
}

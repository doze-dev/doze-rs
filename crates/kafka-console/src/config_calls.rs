//! The IncrementalAlterConfigs call, written as the config boards draw it: the API and its version, the topic, and one line
//! for each change (`key  OP  value`). Every value goes through [`visible`] so that a newline or a control character
//! typed into a form cannot make a second line.

use crate::calls::visible;
use crate::model::{ConfigOp, Lines, Tok};

/// One change of a call: key, operation, value.
pub type Change = (String, ConfigOp, Option<String>);

/// The lines of a call that sends `changes` for `topic`.
#[must_use]
pub fn alter(version: i16, topic: &str, changes: &[Change]) -> Lines {
    let mut out: Lines = vec![
        vec![
            (Tok::Keyword, "IncrementalAlterConfigs".to_owned()),
            (Tok::Plain, format!(" v{version}")),
        ],
        vec![
            (Tok::Plain, "  TOPIC ".to_owned()),
            (Tok::Str, visible(topic, 80)),
        ],
    ];
    out.extend(changes.iter().map(|c| vec![(Tok::Plain, change_line(c))]));
    out
}

/// `  key  SET  value`, a delete without a value.
#[must_use]
pub fn change_line((key, op, value): &Change) -> String {
    let key = visible(key, 60);
    match value {
        Some(v) => format!("  {key}  {}  {}", op.word(), visible(v, 80)),
        None => format!("  {key}  {}", op.word()),
    }
}

/// The same changes as text, one per line, without the call's head: what "What would pass" draws.
#[must_use]
pub fn change_text(changes: &[Change]) -> Vec<String> {
    changes
        .iter()
        .map(|c| change_line(c).trim_start().to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: &Lines) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.iter().map(|(_, t)| t.as_str()).collect())
            .collect()
    }

    #[test]
    fn a_call_is_the_api_the_topic_and_a_line_for_each_change_as_the_boards_draw_it() {
        let call = alter(
            1,
            "inventory.levels",
            &[
                (
                    "min.compaction.lag.ms".into(),
                    ConfigOp::Set,
                    Some("7200000".into()),
                ),
                ("segment.bytes".into(), ConfigOp::Delete, None),
            ],
        );
        assert_eq!(
            text(&call),
            [
                "IncrementalAlterConfigs v1",
                "  TOPIC inventory.levels",
                "  min.compaction.lag.ms  SET  7200000",
                "  segment.bytes  DELETE",
            ]
        );
        assert_eq!(call[0][0].0, Tok::Keyword);
    }

    #[test]
    fn what_was_typed_cannot_start_a_line_of_its_own() {
        let call = alter(
            1,
            "t\nu",
            &[("k".into(), ConfigOp::Set, Some("1\n  evil  SET  2".into()))],
        );
        assert_eq!(
            call.len(),
            3,
            "a newline typed is written out, not a new line"
        );
        assert!(text(&call)[2].contains("\\n"), "{call:?}");
        assert!(text(&call)[1].contains("\\n"));
    }
}

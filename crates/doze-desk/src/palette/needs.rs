//! What the service adds to the palette's model (Go `Service.Palette`): a stack whose doze.hcl does not load says so
//! on its Up rows and opens its page instead, and the empty, unnarrowed palette lists Needs you's red stack-level
//! items (`paletteNeeds`), each opening the bell, where its fixes are. The troubled services are already there.

use std::collections::BTreeSet;

use super::{DoKind, Doing, Palette, Row, Section};
use crate::attention::Item;

/// The Up rows of a stopped stack whose doze.hcl does not load (`broken`, by key): they say so and open its page.
pub fn broken_up(p: &mut Palette, broken: &BTreeSet<String>) {
    for sec in &mut p.sections {
        for r in &mut sec.rows {
            if r.doing.kind == DoKind::Stack
                && r.doing.verb.starts_with("up")
                && broken.contains(&r.doing.stack_key)
            {
                r.note = "doze.hcl does not load; open it to see why".into();
                r.doing = Doing {
                    kind: DoKind::Page,
                    stack_key: r.doing.stack_key.clone(),
                    page: "home".into(),
                    ..Doing::default()
                };
            }
        }
    }
}

/// Needs you's red items that are not a service's, under "Needs you" (first), for an empty query with no scope.
pub fn stack_needs(p: &mut Palette, items: &[Item]) {
    let rows: Vec<Row> = items
        .iter()
        .filter(|it| it.instance.is_empty() && !it.fixed && it.red)
        .map(|it| Row {
            glyph: "dot",
            tone: "r",
            label: it.title.clone(),
            note: if it.stack.is_empty() {
                it.detail.clone()
            } else {
                format!("{} · {}", it.stack, it.detail)
            },
            key: "⏎ fix".into(),
            bad: true,
            doing: Doing {
                kind: DoKind::App,
                text: "bell".into(),
                ..Doing::default()
            },
            ..Row::default()
        })
        .collect();
    if rows.is_empty() {
        return;
    }
    if let Some(sec) = p.sections.iter_mut().find(|s| s.title == "Needs you") {
        sec.rows.extend(rows);
        return;
    }
    p.sections.insert(
        0,
        Section {
            title: "Needs you".into(),
            rows,
        },
    );
}

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing)]
    use super::*;

    fn up_row(key: &str) -> Row {
        Row {
            label: "Up acme as last time".into(),
            doing: Doing {
                kind: DoKind::Stack,
                stack_key: key.into(),
                verb: "up-last".into(),
                ..Doing::default()
            },
            ..Row::default()
        }
    }

    #[test]
    fn a_broken_stack_up_row_opens_its_page_in_go_words() {
        let mut p = Palette {
            sections: vec![Section {
                title: "Commands".into(),
                rows: vec![up_row("/a"), up_row("/b")],
            }],
            ..Palette::default()
        };
        broken_up(&mut p, &BTreeSet::from(["/a".to_owned()]));
        let r = &p.sections[0].rows;
        assert_eq!(r[0].note, "doze.hcl does not load; open it to see why");
        assert_eq!(
            (r[0].doing.kind, r[0].doing.page.as_str()),
            (DoKind::Page, "home")
        );
        assert_eq!(
            r[1].doing.kind,
            DoKind::Stack,
            "a stack that loads keeps its Up"
        );
    }

    #[test]
    fn red_stack_items_join_needs_you_and_open_the_bell() {
        let item = |instance: &str, red: bool| Item {
            title: "acme failed to start".into(),
            stack: "acme".into(),
            detail: "port 5432 is taken".into(),
            instance: instance.into(),
            red,
            ..Item::default()
        };
        let mut p = Palette::default();
        stack_needs(
            &mut p,
            &[item("", true), item("api", true), item("", false)],
        );
        assert_eq!(p.sections.len(), 1);
        let s = &p.sections[0];
        assert_eq!(s.title, "Needs you");
        assert_eq!(
            s.rows.len(),
            1,
            "services are there already; yellow waits in the bell"
        );
        assert_eq!(s.rows[0].note, "acme · port 5432 is taken");
        assert_eq!(s.rows[0].key, "⏎ fix");
        assert_eq!(
            (s.rows[0].doing.kind, s.rows[0].doing.text.as_str()),
            (DoKind::App, "bell")
        );
    }
}

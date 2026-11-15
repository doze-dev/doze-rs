//! The oracle: the Go map (`internal/desk/view/flow.go`) laid out the Larder and shop fixtures and
//! two hundred generated stacks (cycles, failed and running tasks, long names, wide bands), and
//! wrote what it placed to `data/go-flow.json`. Small stacks must come out the same here, card for
//! card and line for line: positions, chips, captions, every path's SVG data, the detail's place.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_map::{CardId, Grouping, Instance, Kind, Layout, Mode, Options, Role, layout};
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    name: String,
    #[serde(rename = "in")]
    ins: Vec<In>,
    map: Map,
}

#[derive(Deserialize)]
struct In {
    name: String,
    kind: String,
    #[serde(default)]
    needs: Option<Vec<String>>,
    #[serde(default)]
    run: String,
    #[serde(default)]
    engine: String,
    #[serde(default)]
    busy: bool,
}

#[derive(Deserialize, Debug, PartialEq)]
struct Cap {
    text: String,
    sub: String,
    x: i64,
    y: i64,
}

#[derive(Deserialize, Debug, PartialEq)]
struct Node {
    name: String,
    x: i64,
    y: i64,
    w: i64,
    h: i64,
    #[serde(default)]
    lane: bool,
    #[serde(default)]
    task: bool,
    #[serde(default)]
    lang: String,
    chips: Vec<String>,
    #[serde(default)]
    more: u32,
    #[serde(default)]
    quiet: bool,
    #[serde(default)]
    fold: bool,
}

#[derive(Deserialize, Debug, PartialEq)]
struct Shared {
    name: String,
    x: i64,
    y: i64,
    w: i64,
    h: i64,
}

#[derive(Deserialize, Debug, PartialEq)]
struct Line {
    from: String,
    to: String,
    d: String,
}

#[derive(Deserialize, Debug, PartialEq)]
struct At {
    x: i64,
    y: i64,
}

#[derive(Deserialize, Debug, PartialEq)]
struct Map {
    w: i64,
    h: i64,
    caps: Vec<Cap>,
    band: i64,
    nodes: Vec<Node>,
    shared: Vec<Shared>,
    edges: Vec<Line>,
    lines: Vec<Line>,
    detail: At,
}

fn kind(k: &str) -> Kind {
    match k {
        "app" => Kind::App,
        "task" => Kind::Task,
        "service" => Kind::Service,
        _ => Kind::Module,
    }
}

fn instances(c: &Case) -> Vec<Instance> {
    c.ins
        .iter()
        .map(|i| {
            let needs: Vec<&str> = i.needs.iter().flatten().map(String::as_str).collect();
            let mut v = Instance::new(&i.name, kind(&i.kind), &needs);
            v.run.clone_from(&i.run);
            v.engine.clone_from(&i.engine);
            v.busy = i.busy;
            v
        })
        .collect()
}

#[allow(clippy::cast_possible_truncation)]
fn int(f: f64) -> i64 {
    f as i64
}

/// The layout written the way the Go map writes it.
fn as_go(l: &Layout) -> Map {
    let name = |id: &CardId| l.card(*id).map(|c| c.name.clone()).unwrap_or_default();
    let nodes = l.cards.iter().filter(|c| c.role != Role::Shared);
    let line = |a: &CardId, b: &CardId, d: String| Line {
        from: name(a),
        to: name(b),
        d,
    };
    Map {
        w: int(l.bounds.w),
        h: int(l.bounds.h),
        caps: l
            .captions
            .iter()
            .map(|c| Cap {
                text: c.text.clone(),
                sub: c.sub.clone(),
                x: int(c.at.x),
                y: int(c.at.y),
            })
            .collect(),
        band: l.band.map_or(0, int),
        nodes: nodes
            .clone()
            .map(|c| Node {
                name: c.name.clone(),
                x: int(c.rect.x),
                y: int(c.rect.y),
                w: int(c.rect.w),
                h: int(c.rect.h),
                lane: c.role == Role::Lane,
                task: c.kind == Kind::Task,
                lang: c.lang.map(|g| g.badge().to_string()).unwrap_or_default(),
                chips: c.chips.clone(),
                more: c.more,
                quiet: c.quiet,
                fold: c.fold,
            })
            .collect(),
        shared: l
            .cards
            .iter()
            .filter(|c| c.role == Role::Shared)
            .map(|c| Shared {
                name: c.name.clone(),
                x: int(c.rect.x),
                y: int(c.rect.y),
                w: int(c.rect.w),
                h: int(c.rect.h),
            })
            .collect(),
        edges: l
            .edges
            .iter()
            .map(|e| line(&e.from, &e.to, e.path.d()))
            .collect(),
        lines: (0..l.cards.len())
            .map(|i| CardId(u32::try_from(i).unwrap()))
            .filter(|id| l.card(*id).is_some_and(|c| c.role != Role::Shared))
            .flat_map(|id| l.use_lines(id))
            .map(|(a, b, p)| line(&a, &b, p.d()))
            .collect(),
        detail: At {
            x: int(l.detail.x),
            y: int(l.detail.y),
        },
    }
}

fn first_difference(want: &Map, got: &Map) -> String {
    macro_rules! cmp {
        ($f:ident) => {
            if want.$f != got.$f {
                return format!("{}: want {:?}\n got {:?}", stringify!($f), want.$f, got.$f);
            }
        };
    }
    cmp!(w);
    cmp!(h);
    cmp!(band);
    cmp!(caps);
    for (w, g) in want.nodes.iter().zip(&got.nodes) {
        if w != g {
            return format!("node: want {w:?}\n got {g:?}");
        }
    }
    cmp!(nodes);
    cmp!(shared);
    for (w, g) in want
        .edges
        .iter()
        .zip(&got.edges)
        .chain(want.lines.iter().zip(&got.lines))
    {
        if w != g {
            return format!("line: want {w:?}\n got {g:?}");
        }
    }
    cmp!(edges);
    cmp!(lines);
    cmp!(detail);
    String::new()
}

#[test]
fn small_stacks_come_out_as_the_go_map_draws_them() {
    let raw = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/go-flow.json"
    ))
    .unwrap();
    let cases: Vec<Case> = serde_json::from_str(&raw).unwrap();
    assert!(cases.len() > 200);
    let opts = Options {
        classic_max: usize::MAX,
        grouping: Grouping::None,
        ..Options::default()
    };
    let mut failed = Vec::new();
    for c in &cases {
        let l = layout(&instances(c), &opts);
        assert_eq!(l.mode, Mode::Classic);
        let got = as_go(&l);
        if got != c.map {
            failed.push(format!("{}: {}", c.name, first_difference(&c.map, &got)));
        }
    }
    assert!(
        failed.is_empty(),
        "{} of {} differ from Go; first:\n{}",
        failed.len(),
        cases.len(),
        failed.first().unwrap()
    );
}

/// TestLarderMap's own checks, kept readable: the board's places and the counts.
#[test]
fn larder_matches_the_board() {
    let raw = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/go-flow.json"
    ))
    .unwrap();
    let cases: Vec<Case> = serde_json::from_str(&raw).unwrap();
    let larder = cases.iter().find(|c| c.name == "larder").unwrap();
    let l = layout(&instances(larder), &Options::default());
    let at = |n: &str| l.card(l.card_named(n).unwrap()).unwrap().rect;
    for (n, x, y, w) in [
        ("storefront", 24., 192., 214.),
        ("identity", 596., 52., 250.),
        ("catalog", 1204., 192., 214.),
        ("payments", 24., 444., 214.),
    ] {
        assert_eq!((at(n).x, at(n).y, at(n).w), (x, y, w), "{n}");
    }
    assert_eq!(l.band, Some(534.0));
    assert_eq!(l.edges.len(), 9);
    assert_eq!((l.detail.x, l.detail.y), (970.0, 14.0));
}

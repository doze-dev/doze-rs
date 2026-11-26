//! The tree's ids, filter, paging and media, without a broker (the broker's side: tests/module_blocks.rs).
#![allow(clippy::unwrap_used)]

use super::*;

#[test]
fn an_id_reads_back_as_what_it_was_made_from() {
    let all = [
        At::Top,
        At::Topics,
        At::Groups,
        At::Topic("orders.placed_v2-x".into()),
        At::Topic("odd/50%".into()),
        At::Partition("orders".into(), 7),
        At::Message("odd/50%".into(), 0, 1042),
        At::Group("team/a b".into()),
    ];
    for at in all {
        assert_eq!(At::parse(&at.id()), Some(at.clone()), "{}", at.id());
    }
    assert_eq!(At::Message("a.b".into(), 2, 9).id(), "t/a.b/2/9");
    for bad in [
        "t/",
        "t//1",
        "t/x/-1",
        "t/x/1/y",
        "t/x/1/2/3",
        "g/",
        "x",
        "t/x/99999999999",
    ] {
        assert_eq!(At::parse(bad), None, "{bad}");
    }
}

#[test]
fn a_filter_is_any_part_of_a_name_in_any_case() {
    assert!(matches("Orders.Placed", "placed"));
    assert!(matches("orders", "ORD "));
    assert!(matches("orders", ""));
    assert!(!matches("payments", "order"));
}

#[test]
fn a_page_reads_back_from_its_cursor_to_where_the_partition_starts() {
    assert_eq!(window(0, 120, 50), (70, Some(70)));
    assert_eq!(window(0, 70, 50), (20, Some(20)));
    assert_eq!(window(0, 20, 50), (0, None));
    assert_eq!(
        window(30, 60, 50),
        (30, None),
        "retention took the first 30"
    );
    assert_eq!(window(5, 5, 50), (5, None), "an empty partition");
}

#[test]
fn a_value_is_json_text_or_bytes() {
    assert_eq!(media_of(br#" {"id": 1}"#), media::JSON);
    assert_eq!(media_of(b"[1,2]"), media::JSON);
    assert_eq!(media_of(b"{not json"), media::TEXT);
    assert_eq!(media_of(b"42"), media::TEXT, "a bare number reads as it is");
    assert_eq!(media_of("héllo".as_bytes()), media::TEXT);
    assert_eq!(media_of(b""), media::TEXT);
    assert_eq!(media_of(&[0x89, b'P', 0xff]), media::BYTES);
    assert_eq!(shown(&[0xff, 0xfe]), "0xfffe");
    assert_eq!(shown(b"trace-1"), "trace-1");
}

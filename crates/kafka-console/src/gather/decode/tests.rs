use super::*;

fn of(bytes: &[u8]) -> Shown {
    shown(Some(bytes), bytes.len() as u64)
}

#[test]
fn nothing_and_empty_are_told_apart() {
    assert_eq!(shown(None, 0).kind, Kind::Null);
    assert_eq!(of(b"").kind, Kind::Empty);
    assert_eq!(of(b"").bytes, 0);
}

#[test]
fn text_is_text_and_a_complete_json_object_or_array_is_json() {
    for (bytes, kind) in [
        (&b"ord-88412"[..], Kind::Text),
        (br#"{"order_id":"ord-88412","qty":2}"#, Kind::Json),
        (
            b"  [1, 2.5e3, -0, \"a\\u00e9\", true, null, {}]  ",
            Kind::Json,
        ),
        (b"42", Kind::Text),
        (b"\"a string\"", Kind::Text),
        (br#"{"a":1"#, Kind::Text),
        (br#"{"a":1} trailing"#, Kind::Text),
        (br#"{a:1}"#, Kind::Text),
        (br#"{"a":01}"#, Kind::Text),
        (br#"[1,]"#, Kind::Text),
        (b"{\"a\":\"\x01\"}", Kind::Text),
        ("héllo wörld ✓".as_bytes(), Kind::Text),
    ] {
        assert_eq!(of(bytes).kind, kind, "{:?}", String::from_utf8_lossy(bytes));
    }
}

#[test]
fn json_nested_past_the_depth_is_not_recognised_and_does_not_overflow_the_stack() {
    let deep = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
    assert!(!looks_like_json(&deep));
    let ok = format!("{}{}", "[".repeat(60), "]".repeat(60));
    assert!(looks_like_json(&ok));
    // Depths 0 to 64 are recognised: 65 brackets, not 66.
    let at = |n: usize| format!("{}{}", "[".repeat(n), "]".repeat(n));
    assert!(looks_like_json(&at(65)));
    assert!(!looks_like_json(&at(66)));
}

/// Every literal, every escape, a number's fraction and exponent with its sign, and a container closed by its own bracket.
#[test]
fn json_is_read_to_the_letter_of_rfc_8259() {
    for (text, json) in [
        ("[false]", true),
        ("[true, null, false]", true),
        (r#"["a\"b", "c\\d", "e\/f", "\b\f\n\r\t"]"#, true),
        (r#"{"k\"ey": "v"}"#, true),
        (r#"["\x"]"#, false),
        ("[1e+5, 1E-2, 2.5e3, -0.5]", true),
        ("[1.]", false),
        ("[1e]", false),
        ("[1e+]", false),
        ("[-]", false),
        ("[1}", false),
        (r#"{"a":1]"#, false),
        ("[[1],[2]]", true),
    ] {
        assert_eq!(looks_like_json(text), json, "{text}");
    }
}

/// A cut that lands inside a character steps back to the character's start.
#[test]
fn a_cut_inside_a_character_steps_back_to_its_start() {
    let odd = format!("a{}", "é".repeat(TEXT_CAP));
    let s = of(odd.as_bytes());
    assert!(s.clipped);
    assert_eq!(s.text.len(), TEXT_CAP - 1);
    assert!(s.text.starts_with("aé"));
}

/// Only five bytes or more that begin with a zero byte may be a schema registry's frame.
#[test]
fn only_five_bytes_or_more_from_a_zero_byte_may_be_framed() {
    let framed = of(&[0, 0, 0, 0, 1, 2]);
    assert_eq!(
        framed.framed,
        Some(Framed {
            bytes: vec![0, 0, 0, 0, 1, 2],
            whole: true
        })
    );
    assert!(of(&[0, 0, 0, 1]).framed.is_none(), "four bytes");
    assert!(of(b"hello").framed.is_none(), "no zero byte first");
    let part = shown(Some(&[0, 0, 0, 0, 7]), 9);
    assert_eq!(part.framed.map(|f| f.whole), Some(false));
}

/// A cell counts characters, not bytes: 47 four-byte characters fit whole, and more are cut with `…`.
#[test]
fn a_cell_counts_characters_not_bytes() {
    let fits = "🦇".repeat(PREVIEW_CHARS);
    assert_eq!(of(fits.as_bytes()).preview, fits);
    let over = "🦇".repeat(60);
    let p = of(over.as_bytes()).preview;
    assert_eq!(p, format!("{fits}…"));
}

#[test]
fn bytes_that_are_not_utf8_are_binary_with_a_hex_dump_and_a_preview_of_eight_bytes() {
    let bytes = [
        0x0a, 0x09, 0x6f, 0x72, 0x64, 0x2d, 0x38, 0x38, 0x34, 0xff, 0x00,
    ];
    let s = of(&bytes);
    assert_eq!(s.kind, Kind::Binary);
    assert_eq!(s.bytes, 11);
    assert_eq!(s.preview, "binary · 0a 09 6f 72 64 2d 38 38 …");
    assert!(
        s.text
            .starts_with("0000  0a 09 6f 72 64 2d 38 38  ..ord-88\n0008  34 ff 00"),
        "{}",
        s.text
    );
    assert!(!s.clipped);
    // Eight bytes or fewer have nothing after them.
    assert_eq!(of(&[0xff, 0xfe]).preview, "binary · ff fe");
}

#[test]
fn a_value_is_shown_in_full_to_64_kib_and_then_cut_with_its_true_size() {
    let exact = vec![b'a'; TEXT_CAP];
    let s = of(&exact);
    assert!(!s.clipped && s.text.len() == TEXT_CAP && s.bytes == TEXT_CAP as u64);
    let over = vec![b'a'; TEXT_CAP + 1];
    let s = of(&over);
    assert!(s.clipped && s.text.len() == TEXT_CAP && s.bytes == TEXT_CAP as u64 + 1);
    // A cut never splits a character.
    let wide = "é".repeat(TEXT_CAP);
    let s = of(wide.as_bytes());
    assert!(s.clipped && s.text.len() == TEXT_CAP && s.text.chars().all(|c| c == 'é'));
}

#[test]
fn a_prefix_that_stopped_in_the_middle_of_a_character_is_still_text() {
    let whole = "ab✓cd".as_bytes();
    let cut = &whole[..4]; // a, b, and two bytes of a three-byte character
    let s = shown(Some(cut), whole.len() as u64);
    assert_eq!(s.kind, Kind::Text);
    assert_eq!(s.text, "ab");
    assert!(s.clipped);
    assert_eq!(s.bytes, 7);
    // The same cut of something that was whole is invalid, so it is binary.
    assert_eq!(shown(Some(cut), cut.len() as u64).kind, Kind::Binary);
}

#[test]
fn hex_is_cut_at_16_kib() {
    let bytes = vec![0xffu8; HEX_CAP + 8];
    let s = of(&bytes);
    assert!(s.clipped && s.text.lines().count() == HEX_CAP / 8);
}

#[test]
fn what_could_act_on_the_page_is_written_out() {
    let hostile = "a\u{0}b\u{202e}c\u{2066}d\u{7f}e\u{85}f\u{feff}\u{2028}g\tn\nr\rz";
    let s = of(hostile.as_bytes());
    assert_eq!(
        s.text,
        "a⟨U+0000⟩b⟨U+202E⟩c⟨U+2066⟩d⟨U+007F⟩e⟨U+0085⟩f⟨U+FEFF⟩⟨U+2028⟩g\tn\nr\rz"
    );
    assert!(
        !s.preview.contains('\u{202e}') && !s.preview.contains('\n') && !s.preview.contains('\t')
    );
}

#[test]
fn a_cell_is_one_line_of_47_characters() {
    let s = of(br#"{"order_id":"ord-88418","customer":"c-3092","items":[1,2,3]}"#);
    assert_eq!(
        s.preview,
        r#"{"order_id":"ord-88418","customer":"c-3092","it…"#
    );
    assert_eq!(s.preview.chars().count(), 48);
    let exact = "x".repeat(47);
    assert_eq!(of(exact.as_bytes()).preview, exact);
    assert_eq!(
        of("x".repeat(48).as_bytes()).preview,
        format!("{}…", "x".repeat(47))
    );
    assert_eq!(of(b"two\nlines\there").preview, "two lines here");
}

#[test]
fn no_bytes_make_it_panic_and_the_size_is_always_the_true_one() {
    for seed in 0..4000_u32 {
        let n = (seed % 70) as usize;
        let bytes: Vec<u8> = (0..n)
            .map(|i| (seed.wrapping_mul(2_654_435_761).wrapping_add(i as u32 * 97) >> 11) as u8)
            .collect();
        let s = of(&bytes);
        assert_eq!(s.bytes, n as u64);
        assert!(!s.preview.contains('\n'));
    }
}

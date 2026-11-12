use super::*;

/// RFC 4648 section 10.
const VECTORS: [(&str, &str); 7] = [
    ("", ""),
    ("f", "Zg=="),
    ("fo", "Zm8="),
    ("foo", "Zm9v"),
    ("foob", "Zm9vYg=="),
    ("fooba", "Zm9vYmE="),
    ("foobar", "Zm9vYmFy"),
];

#[test]
fn rfc_4648_vectors_round_trip() {
    for (plain, text) in VECTORS {
        assert_eq!(encode(plain.as_bytes()), text);
        assert_eq!(decode(text.as_bytes()).unwrap(), plain.as_bytes());
    }
}

#[test]
fn every_byte_value_and_length_round_trips() {
    let all: Vec<u8> = (0..=255).collect();
    for len in 0..=all.len() {
        let text = encode(&all[..len]);
        assert_eq!(decode(text.as_bytes()).unwrap(), &all[..len], "{len}");
    }
    assert_eq!(encode(&[0xfb, 0xff, 0xbf]), "+/+/");
}

#[test]
fn text_that_is_not_canonical_padded_base64_is_refused() {
    let refused = [
        "Zg",        // padding missing
        "Zg=",       // padding short
        "Zg===",     // padding long
        "Z===",      // three pads
        "Zm9v\n",    // white space
        "Zm9v ",     // white space
        "Zm9 v",     // inside
        "Zm9v=AAA",  // padding in the middle
        "Zg==Zg==",  // padding in the middle
        "Zg=A",      // pad then data
        "Zm9-",      // url-safe alphabet
        "Zm9_",      // url-safe alphabet
        "Zh==",      // spare bits set: "Zg==" is the canonical "f"
        "Zm9=",      // spare bits set: "Zm8=" is the canonical "fo"
        "====",      // nothing
        "!!!!",      // not base64
        "AAA\u{e9}", // not ASCII
    ];
    for text in refused {
        assert_eq!(decode(text.as_bytes()), Err(Invalid), "{text:?}");
    }
    assert_eq!(decode(&[0xff, 0xfe, 0xfd, 0xfc]), Err(Invalid));
}

/// Java's decoder, which Kafka's SCRAM messages go through, ignores bits set past the data; the lax decoder
/// does too, and refuses everything else a strict one refuses.
#[test]
fn the_lax_decoder_accepts_spare_bits_and_nothing_else() {
    assert_eq!(decode_lax(b"Zh=="), Ok(b"f".to_vec()));
    assert_eq!(decode_lax(b"Zm9="), Ok(b"fo".to_vec()));
    for (plain, text) in VECTORS {
        assert_eq!(decode_lax(text.as_bytes()).unwrap(), plain.as_bytes());
    }
    for text in [
        "Zg",
        "Zg===",
        "Zm9v\n",
        "Zg==Zg==",
        "Zm9-",
        "====",
        "AAA\u{e9}",
    ] {
        assert_eq!(decode_lax(text.as_bytes()), Err(Invalid), "{text:?}");
    }
}

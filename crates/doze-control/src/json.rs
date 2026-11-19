//! JSON written as Go's `encoding/json` writes it, so a frame the Rust side encodes is the frame the Go side would
//! have: `<`, `>`, `&`, U+2028 and U+2029 escaped (Go's HTML-safe default), and floats in Go's shortest form (`0`
//! rather than `0.0`, `1e-7`, `1e+21`). Plus the small predicates the types' `omitempty` fields use.

use serde::Serialize;
use std::io::{self, Write};

/// A `serde_json` formatter that writes what Go's `json.Encoder` writes.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct GoFormatter;

impl serde_json::ser::Formatter for GoFormatter {
    fn write_string_fragment<W: ?Sized + Write>(
        &mut self,
        w: &mut W,
        fragment: &str,
    ) -> io::Result<()> {
        let mut start = 0;
        for (i, c) in fragment.char_indices() {
            let esc = match c {
                '<' => "\\u003c",
                '>' => "\\u003e",
                '&' => "\\u0026",
                '\u{2028}' => "\\u2028",
                '\u{2029}' => "\\u2029",
                _ => continue,
            };
            w.write_all(fragment.get(start..i).unwrap_or_default().as_bytes())?;
            w.write_all(esc.as_bytes())?;
            start = i + c.len_utf8();
        }
        w.write_all(fragment.get(start..).unwrap_or_default().as_bytes())
    }

    fn write_f64<W: ?Sized + Write>(&mut self, w: &mut W, value: f64) -> io::Result<()> {
        w.write_all(go_float(value).as_bytes())
    }

    fn write_f32<W: ?Sized + Write>(&mut self, w: &mut W, value: f32) -> io::Result<()> {
        w.write_all(go_float(f64::from(value)).as_bytes())
    }
}

/// A float as Go's `strconv.FormatFloat(f, 'f' or 'e', -1, 64)` with `encoding/json`'s choice between them.
fn go_float(v: f64) -> String {
    let abs = v.abs();
    if abs == 0.0 || (1e-6..1e21).contains(&abs) {
        // Rust's Display is the shortest round-trip form without an exponent, as Go's 'f' with precision -1.
        return format!("{v}");
    }
    // Rust writes `1e21` and `1e-7`; Go writes `1e+21` and `1e-07`, then trims the latter back to `1e-7`.
    let s = format!("{v:e}");
    match s.split_once('e') {
        Some((m, e)) if !e.starts_with('-') => format!("{m}e+{e}"),
        _ => s,
    }
}

/// Go's `json.Encoder` with `SetIndent("", "  ")`: the structure of serde's pretty printer (two spaces, `[]` and `{}`
/// for empty ones), the strings and floats of [`GoFormatter`].
#[derive(Debug, Default)]
pub(crate) struct GoPretty<'a>(serde_json::ser::PrettyFormatter<'a>);

macro_rules! structure {
    ($($name:ident($($arg:ident: $ty:ty),*);)*) => {
        $(fn $name<W: ?Sized + Write>(&mut self, w: &mut W $(, $arg: $ty)*) -> io::Result<()> {
            serde_json::ser::Formatter::$name(&mut self.0, w $(, $arg)*)
        })*
    };
}

impl serde_json::ser::Formatter for GoPretty<'_> {
    structure! {
        begin_array();
        end_array();
        begin_array_value(first: bool);
        end_array_value();
        begin_object();
        end_object();
        begin_object_key(first: bool);
        begin_object_value();
        end_object_value();
    }

    fn write_string_fragment<W: ?Sized + Write>(
        &mut self,
        w: &mut W,
        fragment: &str,
    ) -> io::Result<()> {
        GoFormatter.write_string_fragment(w, fragment)
    }

    fn write_f64<W: ?Sized + Write>(&mut self, w: &mut W, value: f64) -> io::Result<()> {
        w.write_all(go_float(value).as_bytes())
    }

    fn write_f32<W: ?Sized + Write>(&mut self, w: &mut W, value: f32) -> io::Result<()> {
        w.write_all(go_float(f64::from(value)).as_bytes())
    }
}

/// `v` as Go's `json.Marshal` writes it: one line, no newline.
pub fn go_line<T: Serialize + ?Sized>(v: &T) -> serde_json::Result<String> {
    let mut out = Vec::new();
    v.serialize(&mut serde_json::Serializer::with_formatter(
        &mut out,
        GoFormatter,
    ))?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// `v` as Go's `json.Encoder` with a two-space indent writes it, without the final newline.
pub fn go_pretty<T: Serialize + ?Sized>(v: &T) -> serde_json::Result<String> {
    let mut out = Vec::new();
    v.serialize(&mut serde_json::Serializer::with_formatter(
        &mut out,
        GoPretty::default(),
    ))?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Serialises `v` as one line of Go-shaped JSON, newline included, onto `out`.
pub(crate) fn write_line<T: Serialize + ?Sized>(
    v: &T,
    out: &mut Vec<u8>,
) -> serde_json::Result<()> {
    let mut ser = serde_json::Serializer::with_formatter(&mut *out, GoFormatter);
    v.serialize(&mut ser)?;
    out.push(b'\n');
    Ok(())
}

// The `omitempty` predicates for `skip_serializing_if`, which passes a reference.
#[allow(clippy::trivially_copy_pass_by_ref)]
pub(crate) fn is_false(b: &bool) -> bool {
    !*b
}

#[allow(clippy::trivially_copy_pass_by_ref)]
pub(crate) fn is_zero_i64(n: &i64) -> bool {
    *n == 0
}

#[allow(clippy::trivially_copy_pass_by_ref)]
pub(crate) fn is_zero_u64(n: &u64) -> bool {
    *n == 0
}

#[allow(clippy::trivially_copy_pass_by_ref)]
pub(crate) fn is_zero_f64(n: &f64) -> bool {
    *n == 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_as_go_writes_them() {
        let cases = [
            (0.0, "0"),
            (3.0, "3"),
            (12.5, "12.5"),
            (0.1, "0.1"),
            (0.000_001, "0.000001"),
            (1e-7, "1e-7"),
            (1.5e-10, "1.5e-10"),
            (1e20, "100000000000000000000"),
            (1e21, "1e+21"),
            (-2.25, "-2.25"),
        ];
        for (v, s) in cases {
            assert_eq!(go_float(v), s);
        }
    }

    #[test]
    fn pretty_is_go_encoders_indent() {
        let v = serde_json::json!({"a": [1.0, 0.5], "b": {}, "c": [], "d": "<x>"});
        assert_eq!(
            go_pretty(&v).ok().as_deref(),
            Some(
                "{\n  \"a\": [\n    1,\n    0.5\n  ],\n  \"b\": {},\n  \"c\": [],\n  \"d\": \"\\u003cx\\u003e\"\n}"
            )
        );
        assert_eq!(
            go_line(&v).ok().as_deref(),
            Some("{\"a\":[1,0.5],\"b\":{},\"c\":[],\"d\":\"\\u003cx\\u003e\"}")
        );
    }

    #[test]
    fn strings_escaped_as_go_escapes_them() {
        let mut out = Vec::new();
        assert!(write_line("<a&b> é\t\u{2028}\u{1}\"\\", &mut out).is_ok());
        assert_eq!(
            String::from_utf8_lossy(&out),
            "\"\\u003ca\\u0026b\\u003e é\\t\\u2028\\u0001\\\"\\\\\"\n"
        );
    }
}

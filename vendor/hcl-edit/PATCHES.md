# Patches to hcl-edit 0.10.0

This is `hcl-edit` 0.10.0 from crates.io, vendored and pinned for doze-hcl (the core plan, risk "HCL
semantics": one maintainer). Each change is marked `PATCH (doze)` in the source. Upstream:
https://github.com/martinohmann/hcl-rs

doze-hcl uses hcl-edit for syntax and spans only. It evaluates nothing with it (hcl-rs's evaluator
uses f64 and panics on `5 % 0`; see the HCL spike report).

## 1. Number literals that overflow f64 (superseded by patch 5)

`1e400` was a parse error: the float parser refused a literal whose f64 value is not finite. Patch 5
replaces the whole number parser, which also covers integers of more than 309 digits (the first
version of this patch missed that branch; found by the H1 audit).

## 2. A raw newline in a quoted string (`src/parser/string.rs`)

hcl-edit accepted a line break inside a quoted string's literal text (`"x` newline `y"`, and around
template directives). Go refuses it ("Quoted strings may not be split over multiple lines"). The literal
now ends at `\n` or `\r`, so the missing closing quote is reported there, where Go reports it. Newlines
inside `${ }` and `%{ }` are unaffected.

## 3. Redefined object keys (`src/parser/expr.rs`)

`{a = 1, a = 2}` was a parse error ("unique object key; found redefined object key"). Go accepts it and
the later item wins. The parser now replaces the earlier item. Patch 6 keeps the earlier item too.

## 4. Directive bodies inside quoted strings (`src/parser/template.rs`, `src/parser/nesting.rs`)

The body of a `%{if}`/`%{else}`/`%{for}` was always parsed with the heredoc rules, even inside a quoted
string: escapes stayed raw (`"%{if x}a\nb%{endif}"` kept a backslash and an `n`), a raw newline was
accepted, and a `"` did not end it. The parser state now records whether it is inside a quoted string
(set by the quoted-string parser, cleared by the heredoc parser, both restored on the way out), and a
directive body in a quoted string uses the quoted-string literal rules, as Go's scanner does.

## 5. Number literals as Go lexes them (`src/parser/number.rs`)

Go's scanner reads a number token as a digit followed by any run of digits, `.` and exponents (`e`,
an optional sign, a digit), not ending in `.`, then checks it with `big.ParseFloat`. hcl-edit required
digits after a `.` (so `1.e5`, which Go reads as 100000, failed), read `1.5.2` as `1.5` then an index,
and refused literals beyond f64. The parser now takes Go's token and refuses an invalid one where it
starts, as Go does. The value it stores is a placeholder: doze-hcl reads the literal's text. Upstream's
unit tests for the old parser are replaced by one for the new grammar.

## 6. Redefined object keys, kept (`src/expr/object.rs`, `src/parser/expr.rs`)

Patch 3 dropped the earlier item. Go keeps every item: it evaluates the earlier value (so an error in
it is reported) and `typeexpr` refuses a redefined attribute ("Object constructor map keys must be
unique"). `Object` now keeps the replaced items, in source order, in `Object::redefined()`; the parser
inserts through `Object::insert_redefining`, which puts the later item in the earlier one's place.
Nothing else in hcl-edit reads the new field.

## Not patched: the slow path on unclosed nesting

hcl-edit backtracks heavily on unclosed nested brackets (`[{[{…`: 1.3 KB took 1.3 s). doze-hcl does
not patch the parser for it: its own Go-shaped syntax check (`crates/doze-hcl/src/check`) runs first
and refuses every unbalanced file, in linear time, before hcl-edit sees it
(`crates/doze-hcl/tests/hostile.rs`).

## Tests

`tests/` and `examples/` are upstream's, kept so the crate's integration tests run on the patched code
(`cargo test --test parse --test regressions --test spans` in this directory); `tests/parse.rs`'s
`redefined_object_keys` is changed to patch 3's behaviour. Upstream's unit tests in
`src/parser/tests.rs` do not compile on the workspace's toolchain (unchanged upstream code), so they are
not run.

# Patches to hcl-edit 0.10.0

This is `hcl-edit` 0.10.0 from crates.io, vendored and pinned for doze-hcl (the core plan, risk "HCL
semantics": one maintainer). Each change is marked `PATCH (doze)` in the source. Upstream:
https://github.com/martinohmann/hcl-rs

doze-hcl uses hcl-edit for syntax and spans only. It evaluates nothing with it (hcl-rs's evaluator
uses f64 and panics on `5 % 0`; see the HCL spike report).

## 1. Number literals that overflow f64 (`src/parser/number.rs`)

`1e400` was a parse error ("expected newline or eof"): the float parser refused any literal whose f64
value is not finite. Go's HCL reads number literals at 512 bits and accepts it. The literal is now kept,
with a placeholder value; doze-hcl reads every number from the literal's text (its `repr`), never from
this value.

## 2. A raw newline in a quoted string (`src/parser/string.rs`)

hcl-edit accepted a line break inside a quoted string's literal text (`"x` newline `y"`, and around
template directives). Go refuses it ("Quoted strings may not be split over multiple lines"). The literal
now ends at `\n` or `\r`, so the missing closing quote is reported there, where Go reports it. Newlines
inside `${ }` and `%{ }` are unaffected.

## 3. Redefined object keys (`src/parser/expr.rs`)

`{a = 1, a = 2}` was a parse error ("unique object key; found redefined object key"). Go accepts it and
the later item wins. The parser now replaces the earlier item. One difference remains: Go also evaluates
the earlier value (so an error inside it is still reported); here it is gone before evaluation.

## 4. Directive bodies inside quoted strings (`src/parser/template.rs`, `src/parser/nesting.rs`)

The body of a `%{if}`/`%{else}`/`%{for}` was always parsed with the heredoc rules, even inside a quoted
string: escapes stayed raw (`"%{if x}a\nb%{endif}"` kept a backslash and an `n`), a raw newline was
accepted, and a `"` did not end it. The parser state now records whether it is inside a quoted string
(set by the quoted-string parser, cleared by the heredoc parser, both restored on the way out), and a
directive body in a quoted string uses the quoted-string literal rules, as Go's scanner does.

## Tests

`tests/` and `examples/` are upstream's, kept so the crate's integration tests run on the patched code
(`cargo test --test parse --test regressions --test spans` in this directory); `tests/parse.rs`'s
`redefined_object_keys` is changed to patch 3's behaviour. Upstream's unit tests in
`src/parser/tests.rs` do not compile on the workspace's toolchain (unchanged upstream code), so they are
not run.

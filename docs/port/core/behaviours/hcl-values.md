# Behaviours: HCL values, conversion and the evaluator (core plan phase H1)

What Go's `hashicorp/hcl` v2.24.0 and `go-cty` v1.18.1 (the versions doze pins) do, distilled from their
source, and where doze-hcl holds each behaviour. The oracle is doze's `tools/oracle-dump` on the
`rust-oracle` branch; its output is checked in under `crates/doze-hcl/testdata/`.

| Corpus | Size | Test |
|---|---|---|
| Spike expressions (`testdata/eval/exprs-spike.json`) | 653 | `tests/oracle_eval.rs` `spike_corpus_matches_go` |
| Audit expressions (`testdata/eval/exprs-audit*.json`, written by the independent H1 audit) | 767 | same test; reviewed differences in `testdata/eval/known-differences.json` are counted apart and fail if they start matching |
| Audit conversions (`testdata/convert/cases-audit.json`) | 102 | `tests/oracle_convert.rs` |
| H1 expressions (`testdata/eval/exprs-h1.json`) | 217 | `tests/oracle_eval.rs` `h1_corpus_matches_go` |
| Conversions and unification (`testdata/convert/cases.json`) | 100 | `tests/oracle_convert.rs` |
| Real sources, accept or refuse (`testdata/sources/`, 190 files) | 190 | `tests/oracle_parse.rs` |
| H2 function calls (`testdata/eval/exprs-funcs.json`: every function with typical, edge and error arguments) | 1401 | `tests/oracle_eval.rs`; Functions below |
| H2 function edges (`testdata/eval/exprs-funcs2.json`: Unicode, number verbs, regex syntax, JSON, CSV, argument positions) | 777 | same |
| H2 generated function cases (`testdata/eval/exprs-funcs3.json`, seeded: number verb grid, case mapping by block, random clusters, random JSON) | 815 | same |

## Numbers (`src/number*`)

| # | Behaviour | Held by |
|---|---|---|
| N1 | A number is a binary float with its own precision: 512 bits when read from text, 64 for host integers, 53 for host floats; `+`, `-`, `/` take the larger precision of their operands; `*` rounds to 512 bits and keeps every bit the product needs (so two host integers multiply exactly) | `number/tests.rs` `host_numbers_keep_their_precision`; corpus `numr.*` (400 seeded random expressions) |
| N2 | Every result is rounded once, to nearest, ties to even | corpus `num.*`, `numr.*` |
| N3 | Text is read by Go's `big.ParseFloat(s, 10, 512)`: sign, digits with an optional point, `e` (decimal) or `p` (binary) exponent; `Inf`/`inf` with a sign; no spaces, no `0x`, no `_`; an exponent beyond int32 is "a number is required"; exponents of 5 beyond 5^27 are raised by squaring at 576 bits | `number/tests.rs` `parses_as_go_does`; `h1num.17`–`h1num.25` |
| N4 | Printing is `Text('f', -1)` by Go's `roundShortest` (the fewest digits that read back at the value's precision), `+Inf`, `-Inf`, `-0` | `prints_the_shortest_decimal_that_reads_back`; `num.02` (`0.1 + 0.2` → `0.3`), `num.05` (`1 / 3`: 155 digits) |
| N5 | `x / 0` is ±Inf; `0 / 0` and `Inf / Inf`, `Inf - Inf`, `0 × Inf` are errors with cty's words | `h1num.04`–`h1num.07` |
| N6 | `x % 0` is `x` (also `x % -0`); otherwise `x - trunc(x / y) × y`, the last two steps at `x`'s precision, through a sign-less integer zero (so `-0 % y` can be `-0`) | `divides_and_takes_remainders_like_cty`; `numr.377`, `h1num.10` |
| N7 | Any infinity in `%` is an error (Go panics inside `big.Float` and cty reports the panic) | `h1num.08`, `h1num.09` |
| N8 | Number equality: whole numbers compare exactly; others by their printed form (so 0.1 at 512 bits equals 0.1 from an f64); `-0 == 0` | `compares_by_value_and_equals_by_print`; `h1num.26`–`h1num.28` |
| N9 | Deliberate difference: the binary exponent is limited to ±2^17 (about 10^±39,000), not Go's ±2^31; beyond it a result is ±Inf or ±0. Printing is an exact decimal conversion, quadratic in its size: Go takes minutes for 10^±300,000, and the fuzzer timed out on 10^±96,000 here before the limit | `huge_exponents_stay_bounded`; `h1num.00`, `h1num.01` |

## Types and values (`src/types.rs`, `src/value*`)

| # | Behaviour | Held by |
|---|---|---|
| V1 | Types: dynamic, string, number, bool, list, set, map, tuple, object (with optional attributes in constraints); the JSON encoding is cty's | every corpus line compares the type as JSON |
| V2 | Strings are NFC on creation, so `"café" == "café"` | `h1str.00`, `h1str.01` |
| V3 | Map and object keys iterate sorted by bytes; lists and tuples by index | `for_.*`, `h1for.18` |
| V4 | A set drops an element equal (`Equals`) to one with the same serialization (hash buckets, so building is linear), and iterates in bucket (CRC-32) order stably sorted: knowns, then unknowns, then nulls; strings, numbers and bools by value; anything else by cty's serialization (`makeSetHashBytes`, numbers as `%g` with 10 digits, strings Go-quoted) | `coll.07`–`coll.12` (convert); `h1for.00` |
| V5 | `Equals`: null against a known non-null is false; unknowns make it unknown unless the types alone decide; different types are unequal; collections compare element-wise and stop at the first difference | `h1op.05`–`h1op.13` |
| V6 | `RawEquals`: same type, same value, nulls and unknowns equal to their kind | used by conditionals and sets; `h1cond.04`–`h1cond.06` |

## Conversion and unification (`src/convert*`)

| # | Behaviour | Held by |
|---|---|---|
| C1 | Safe conversions: number and bool to string. Unsafe: string to number (N3's rules) and to bool (`true`/`1`, `false`/`0`; `"TRUE"` gets its own message) | `prim.*` |
| C2 | A null or unknown converts to a null or unknown of the target type, with any `dynamic` in the target filled from the source type | `prim.09`–`prim.11`, `dyn.*`, `obj.09`, `obj.10` |
| C3 | Lists and sets convert element by element; a list becomes a set only unsafely; empty collections keep a dynamic target's source element type; null elements lose optional markers | `coll.00`–`coll.15`, `coll.26`–`coll.30` |
| C4 | Tuples to lists and sets, and objects to maps: a dynamic target element type is the unification of the element types (all dynamic, or none); after converting, a list or map's elements are unified again | `coll.03`, `coll.04`, `coll.16`–`coll.25` |
| C5 | Objects convert attribute by attribute; extra attributes are dropped; a missing attribute is an error unless optional, which becomes null | `obj.*` |
| C6 | Maps convert to objects only unsafely, by the object's attributes | `coll.34`–`coll.37` |
| C7 | Unification: maps, lists, sets, objects and tuples unify by kind (objects with different names, or tuples of different lengths, fall back to a map or list); a mix with `dynamic` becomes dynamic with every value the dynamic unknown; otherwise the most general type every other converts to, by cty's preference order (string first) | `un.*` |
| C8 | Mismatch messages follow cty's shapes ("attribute "x" is required", "incorrect list element type: …", "a number is required") | wording re-baselined later (decision O6); positions checked |

## Refinements (`src/value/refine.rs`, `src/eval/arith.rs`)

| # | Behaviour | Held by |
|---|---|---|
| R1 | An unknown can carry what is known of it: not null, a string prefix, number bounds, a collection length range; a range that is one inclusive number is that number | `audit2.refine.*` |
| R2 | Operator, equality, template and short-circuit results that are unknown are known not null; a template keeps the text before its first unknown part (up to 128 bytes, cut where a later character could not change the last grapheme) | `audit2.refine.01`, `.12`, `.16` |
| R3 | `==` is false when the null-ness, prefix, bounds or length of an unknown rule the other side out | `audit2.refine.*` |
| R4 | Comparisons are decided when the operands' ranges do not overlap; `+ - *` over ranges give a range (cty's `numericRangeArithmetic`); a conditional with an unknown condition gives the union of its results' ranges | `audit2.refine.07`, `.08` |
| R5 | A splat wraps an unknown that cannot be null in a one-element tuple, as a known value | `audit2.refine.04`–`.06` |
| R6 | Conversions keep not-null and gain or keep a length range (cty's `prepareUnknownResult`) | convert corpus |

## Evaluation (`src/eval*`)

| # | Behaviour | Held by |
|---|---|---|
| E1 | Number literals are read from their text at 512 bits (hcl-edit's f64 value is ignored); `1e400` is a number | `h1num.00` (with vendor patch 1) |
| E2 | Variables: the innermost scope first; "Variables not allowed" when no scope has any; "Unknown variable" otherwise, with a near name suggested | `h1idx.24`–`h1idx.26` |
| E3 | Operators convert operands to their parameter type first (a failure is reported per side, with both sides' own diagnostics after); then cty's function rules: no nulls except for `==`/`!=`, an unknown operand gives an unknown result | `h1op.*` |
| E4 | `&&` and `||` decide from one side when they can; a null bool counts as "not true" and as "false", so `null || true` is true and `true && null` is false (Go's behaviour, kept) | `h1op.00`–`h1op.04`, `null_.12` |
| E5 | Conditionals evaluate both results, unify their types (a bare `null` takes the other side's type), then the condition: null is an error, unknown gives an unknown (or null when both results are null), non-bool is an error; only the chosen side's diagnostics are kept | `h1cond.*` |
| E6 | Tuple and object constructors; object keys: a bare name or keyword is a literal name (`{null = 1}`), a dotted name is "Ambiguous attribute key", anything else is evaluated and converted to a string; a null key is an error; a key error makes the whole object the dynamic unknown; a repeated key: the later wins | `h1obj.*`, `coll.18` (vendor patch 3) |
| E7 | Indexing (`hcl.Index`) and attribute access (`hcl.GetAttr`) with Go's messages, including Go's quirk that only an index equal to the length gets the "greater than or equal to the length" detail | `h1idx.*` |
| E8 | Splats: `[*]` takes every following step, `.*` only the attribute steps after it; a non-sequence is wrapped in a one-element tuple and a null in an empty one; null sequences are an error; an unknown source gives an unknown of the computed type; list and set sources give a list, tuples a tuple | `h1spl.*`, `splat.*` |
| E9 | `for`: the `if` clause is checked once with dynamic unknowns before iterating; null, non-iterable and unknown collections; per element, an `if` that is null, not bool or unknown makes the result unknown (only the first error is reported); object keys: null, non-string, duplicates (unless grouped with `...`) | `h1for.*`, `for_.*` |
| E10 | Templates: a lone interpolation passes its value through (any type); otherwise every part converts to a string, a null part is an error, an unknown part makes the string unknown | `h1str.*`, `tpl.*` |
| E11 | `~` trims the literal token right beside it; `%{if}` is a conditional between two templates; `%{for}` joins one template per element (null and non-string results are errors) | `h1tpl.*` |
| E12 | Heredocs: Go's literal tokens are one per line, `$${`/`%%{` unescaped; `<<-` removes the smallest indentation of the lines that start with a literal (a line starting with anything else counts as 0, blank lines do not count), after `~` trimming | `h1here.*` |
| E13 | A raw newline inside a quoted string (also inside a directive's body there) is a syntax error | `h1tpl.09`, `tpl.05` (vendor patches 2 and 4) |
| E14 | Function calls: lookup through the scopes ("Function calls not allowed" / "Call to unknown function" with a suggestion; namespaced names), `...` expansion of a tuple, list or set, argument count, conversion to each parameter's type, then cty's call rules (null, dynamic-typed and unknown arguments, `RefineResult`). The library: Functions, F1–F24 | `h1fn.*`; `src/function.rs` |

## Functions (`src/funcs*`, core plan phase H2)

doze.hcl's table (`internal/config/funcs.go`): go-cty v1.18.1's stdlib functions under doze's names (`reverse` is
`ReverseListFunc`), doze's `to*` conversions and its own `env` and `env_of`; built by `funcs::doze_functions()`, or
`doze_functions_with_env(lookup)` to give `env` another environment (the oracle test gives it Go's `env -i
PATH=/usr/bin:/bin`). Corpus ids are `fn.*` (`exprs-funcs.json`), `fn2.*` (`exprs-funcs2.json`) and `fn3.*`
(`exprs-funcs3.json`, seeded and generated).

| # | Behaviour | Held by |
|---|---|---|
| F1 | cty's calling rules plus `RefineResult`: an unknown result (from the call, or because an argument was not ready) is known not null, `strlen`'s also at least 0; the dynamic unknown is never refined. A parameter that allows a dynamic type gives the function's static type (`upper(var.any)` is an unknown string), any other gives the dynamic unknown (`trim(var.any, "x")`). A cty refinement that pins a collection's length makes it known: a list of that many unknowns, an empty collection, a one-element set | `fn.upper.06`, `.14`, `.15`, `fn.trim.08`, `fn.abs.13`, `fn.sort.09` (`sort(["b", var.later])` is a known list of two unknowns), `fn.strlen.14`–`.19` |
| F2 | Argument diagnostics point at Go's `StartRange`: inside the quotes of a string (just after them for `""`), a template's first part (a lone `${…}` is the whole string), a binary operator's left operand, a conditional's condition, the `[` of a non-literal index, the marker of a splat; the expanding-argument error keeps the whole range | `fn2.argpos.*` (38 cases), `fn.parseint.11`; `src/eval/start.rs` |
| F3 | Case mapping is Go's (Unicode's simple mappings, character by character): `ß` and `ŉ` stay, `ᾳ` → `ᾼ`, `İ` lowers to `i`; `title` uses titlecase (`ǆ` → `ǅ`, Georgian Mkhedruli stays) after a separator (any ASCII but letters, digits and `_`; outside ASCII only white space) | `fn.upper.*`, `fn.lower.*`, `fn.title.*`, `fn2.case.*`, `fn3.case.*` (every letter of 25 cased blocks through `upper`, `lower` and `title`) |
| F4 | Grapheme clusters are go-textseg v15's (Unicode 15.0): `unicode-segmentation`'s clusters split again where rule GB9c (Unicode 15.1) joined a consonant after a virama, the viramas found from the crate's own data. `strlen`, `substr`, and `format`'s widths and `%.Ns` count them. `strlen` of an unknown is at least its known prefix's clusters | `fn2.graphemes.*`, `fn3.graph.*` (150 random strings of marks, ZWJ, emoji, Hangul jamo, viramas, prepend and spacing marks) |
| F5 | `substr`: a negative offset counts from the end, and one still negative is the start; an offset past the end is ""; a zero length is "" except after a negative offset, where (Go's loop never counting to zero) it takes the rest; offsets and lengths must fit Go's `int` | `fn.substr.*`, `fn2.graphemes.10` |
| F6 | `trim` takes a set of characters; `trimspace` Unicode white space; `replace` with an empty substring inserts before every character and at the end; `split` by "" gives characters (none for ""); `chomp` drops every trailing `\r` and `\n`; `indent` of a negative count is Go's `strings.Repeat` panic, reported | `fn.trim.*`–`fn.indent.*` |
| F7 | `join`: lists of strings, a null element is an error at its list's argument ("element 1 of list 2"), any unknown makes the result unknown; `sort`: byte order, a null element is an error, an unknown list keeps its length | `fn.join.*`, `fn.sort.*`, `fn2.coll2.*` |
| F8 | `format`'s verbs by cty's state machine: flags `0 # - + space`, a width that starts 1–9, a precision (digits optional), `[n]`, a letter; `%%`. Too few arguments, an unknown verb or a null for anything but `%v` is a call error; too many is an argument error at the first unused one. Any argument not wholly known makes an unknown, prefixed by the (safe) text before the first `%` | `fn.format_basic.*`, `fn.format_unknown.*`, `fn2.fmtmisc.*` |
| F9 | `%v`: a string as it is, a number in shortest `%g` (`1.234567e+06`), anything else as cty JSON; `%#v` always JSON; `%t` through bool conversion; `%s`/`%q` through string conversion, precision in clusters, `%q` JSON-quoted (HTML-safe); cty pads `%v %s %q` to a width in clusters, with zeros if `0` (even before a sign) | `fn.format_v.*`, `fn.format_s.*`, `fn.format_t.*` |
| F10 | Number verbs print as Go's `fmt` with `*big.Int` (`d x X o b`: an exact integer is required; sign, `#` prefix, precision as a digit minimum, `-` beats `0`, `%.0d` of 0 prints nothing) and `*big.Float` (`e E f g G`: precision 6 by default for `e` `f`, shortest for `g`; rounding half to even on the exact binary value; `0` beats `-`, never pads an infinity; `+Inf` keeps its sign) | `fn.format_num.*`, `fn2.fmtnum.*`, `fn3.fmtgrid.*` (300 seeded verb × number cases), `fn3.fmtint.*` |
| F11 | `formatlist`: lists, sets and tuples (not null) give one line per element, everything else repeats on every line; lengths must agree (an argument error); an unknown sequence, a set whose length is not yet known or the dynamic unknown make the whole result unknown; a line with an unknown argument is an unknown element; a line's error is "error on format iteration n" | `fn.formatlist.*`, `fn2.fmtlist2.*` |
| F12 | `regex` reads Go's RE2 syntax with Go's parser rules (escapes incl. octal and `\Q…\E`, classes with POSIX names and `]`/`-` placement, Unicode names matched as Go canonicalizes them with category and script aliases, flags `imsU` with `-`, `{` literal when no repetition follows, nested repetition refused, counts to 1000 and `repeatIsValid`), then writes the crate's syntax: `\d \s \w \b` ASCII, literals as `\x{…}` (so `&&`, `[` and `\<` stay literal), named groups unnamed. No groups: the match; unnamed: a tuple; named: an object; mixed is an error; a group outside the match is null; no match is a call error | `fn.regex.*`, `fn2.regex2.*`, `fn2.regex3.*` |
| F13 | `length`: a tuple's from its type, even unknown; an unknown's within its length range; a set holding unknowns between 1 and its stored count; strings and objects refused. `contains` by `Equals` (unknowns make it unknown). `keys`/`values`: objects give tuples in name order, maps lists. `lookup` takes three arguments (two is an arity error): an object attribute's own type, else the default's; a map's default must convert to its element type | `fn.length.*`, `fn.contains.*`, `fn.keys.*`, `fn.values.*`, `fn.lookup.*`, `fn2.coll2.*` |
| F14 | `merge`: all arguments of one type give that type; otherwise an object of every attribute (later wins), or the dynamic type while a map's keys are unknown; nulls skipped. `distinct` by `Equals`, in order. `reverse`: tuples stay tuples, lists and sets give lists | `fn.merge.*`, `fn.distinct.*`, `fn.reverse.*`, `fn2.coll2.*` |
| F15 | `concat`: all lists unify to one list type, else a tuple (an unknown list makes the type dynamic). `flatten`: nested lists, sets and tuples, nulls kept, an unknown nested sequence makes it unknown. `slice`: indexes checked against the length when it is known (sets refused). `element`: Go's modulo, negatives from the end. `coalesce`: unsafe unification, first non-null converted. `range`: 1–3 arguments, at most 1,024 values, a zero step not caught early (cty compares it with its own zero by identity) so it counts to the limit | `fn.concat.*`–`fn.range.*`, `fn2.seq2.*` |
| F16 | `abs` keeps the precision (`-0` → `0`); `ceil`/`floor` at the argument's precision (`ceil(-0.5)` is `0`, infinities stay); `max`/`min` return the winning argument itself; `parseint` is `big.Int.SetString`: a sign, digits of the base (case-blind to base 36, `a`–`z` then `A`–`Z` above), no prefix or `_`, base 2–62, the result exact at 64 bits or more | `fn.abs.*`, `fn.ceil_floor.*`, `fn.max_min.*`, `fn.parseint.*`, `fn2.num2.*` |
| F17 | `jsonencode`: cty's JSON (object names sorted, numbers in shortest decimal, infinities an error, Go's string escapes with `<>&`, U+2028/9, `\b`, `\f`); null is `"null"`; not wholly known: an unknown string, with the prefix `"` `{` or `[` by type unless it could be null | `fn.jsonencode.*`, `fn2.json2.*` |
| F18 | `jsondecode`: one strict JSON value (Go's decoder), then only white space or a closing bracket (`"1]"` is 1); numbers at 512 bits; arrays are tuples, objects objects, `null` the dynamic null; a repeated key must keep its type (the later wins); a key that is not NFC is refused (cty normalized the attribute it looks up); a lone surrogate is U+FFFD. An unknown string's type comes from the first character of its known prefix | `fn.jsondecode.*`, `fn2.json2.*`, `fn3.json.*` (80 random documents, decoded, re-encoded and `%#v`-printed) |
| F19 | `csvdecode`: `encoding/csv` with its defaults (empty lines skipped, `\r\n` as `\n` and a final `\r` dropped, quoted fields across lines, `""` for a quote, strict quotes), the header names the object attributes (duplicates refused), every record as long as the header, errors with Go's line numbers; empty input is "missing header line" | `fn.csvdecode.*`, `fn2.csv2.*` |
| F20 | `toset tolist tomap tostring tonumber tobool`: the same type passes; otherwise an unsafe conversion must exist (an error at the argument) and the result has the wanted type, element type dynamic where cty leaves it so (`tolist(var.any)` is an unknown list of dynamic); nulls convert | `fn.toset.*`–`fn.tobool.*`, `fn2.conv2.*` |
| F21 | `env(name, defaults...)`: the variable, else the first default, else ""; through the injected lookup (the default reads the process environment; a name the OS cannot hold is unset). `env_of(blocks...)`: objects with `env`, named by `name` or "argument n"; two that set one variable is an error at the later; any unknown inside makes an unknown map; a value that is not a string is an error (Go's recovered panic) | `fn.env.*`, `fn.env_of.*`, `fn2.env2.*`; `tests/funcs_limits.rs` `env_reads_through_its_lookup`, `env_defaults_to_the_process_environment` |
| F22 | Deliberate difference (bounds; Go allocates or recurses as far as asked): `format` widths and precisions at most 1,000,000; `indent` output at most 64 MiB; JSON nesting at most 512; regex groups at most 250 deep; the regex crate's compiled size at most 64 MiB (Go counts a class as one instruction, the crate compiles it to UTF-8 automata, so three `\pL{0,1000}` are refused where Go takes them) | `tests/funcs_limits.rs`; `fn2.regex3.07` (known difference) |
| F23 | Deliberate difference (versions): the regex crate's Unicode 16 lacks the four Unicode 17 scripts Go 1.27 knows (Beria_Erfe, Sidetic, Tai_Yo, Tolong_Siki); numbers past N9's exponent limit decode as ±Inf | `fn2.regex2.129`, `fn2.json2.04` (known differences) |
| F24 | No input panics or runs away: `fuzz_funcs` calls every function with arguments built from the fuzz bytes (values of every kind, the bytes as a string literal, or as expression text) | `fuzz/fuzz_targets/fuzz_funcs.rs` |

## Syntax (`src/syntax.rs`, vendored hcl-edit)

| # | Behaviour | Held by |
|---|---|---|
| S1 | The 190 real sources are accepted or refused exactly as Go does | `tests/oracle_parse.rs` |
| S2 | Positions are Go's: 1-based line, 1-based column counted in grapheme clusters segmented token by token (a punctuation character followed by a combining mark is two columns), 0-based byte | `audit2.pos.*` | every diagnostic comparison |
| S3 | Closed in H2: syntax errors come from `check` (below), so every refused case is placed where Go places it. H1 left 27 open (12 audit, 2 audit2, 4 h1, 9 spike); 0 remain | `tests/oracle_eval.rs` (no "placed differently" bucket left) |
| S4 | Rules Go's parser has and hcl-edit lacks, checked after parsing: chained legacy index `a.1.0`, an attribute splat inside another, a number literal Go cannot read (Go's number token and `1.e5` come from vendor patch 5) | `audit.idx.30`, `audit2.trav.*`, `audit2.numlit.*` |
| S5 | Deliberate difference: hcl-edit refuses more than 64 levels of nesting and 128 chained operators (stack safety); Go has no limit. No real configuration comes near | `audit3.05`–`.12` (known differences) |
| S6 | Closed in H2: hcl-edit backtracks on unclosed nested `[ {` (1.3 KB took 1.3 s, upstream too). It is not patched; `check` runs first, in linear time, and refuses every unbalanced file before hcl-edit sees it | `tests/hostile.rs` (13 hostile shapes, each under 300 ms in debug) |

## Tokens and syntax errors (H2: `src/lex*`, `src/check*`)

| # | Behaviour | Held by |
|---|---|---|
| L1 | Go's scanner by hand: the same tokens with the same byte ranges, lines and columns (columns from the gap in bytes before a token and grapheme clusters inside it), including Ragel's longest-match fallbacks (`/*` unterminated is `/` `*`; `<<EOT` without a newline is `<` `<` `EOT`) and its held character in heredocs (`$x` then a line break is one token, `$xy` is two) | `tests/oracle_syntax.rs` `tokens_match_go`: 5,827 sources (2,500 random fragment sequences, 1,500 mutants of the real sources, the sources, every eval expression) |
| L2 | Identifiers are Unicode ID_Start/ID_Continue as Go's tables have them; U+200C/U+200D are not identifier characters (Go's tables predate Unicode 15.1) | same |
| L3 | Deliberate difference: Ragel accepts overlong and surrogate UTF-8 sequences as characters; doze-hcl reads them the same way when lexing, but counts each invalid byte as one column | fuzz only |
| K1 | Every diagnostic Go's parser gives, in Go's order (token checks first, then the parser's, with Go's recovery and its "recovery mode" that silences later ones), with Go's range and summary; details follow Go's words (re-baselining is the CLI's, O6) | `tests/oracle_syntax.rs` `diagnostics_match_go`: 5,827 sources, 2,554 refused, about 8,300 diagnostics, all identical |
| K2 | Go's aliasing kept: "Missing close bracket on index" (and on a splat) points where recovery stopped, because Go's diagnostic holds a pointer to the variable recovery overwrites | `mut.0510` |
| K3 | Deliberate difference: nesting deeper than 128 levels is refused ("Nesting too deep") to bound the stack; Go has no limit, and hcl-edit already refuses beyond 64 (S5) | `tests/hostile.rs` |

## Type expressions (H2: `src/typeexpr.rs`)

| # | Behaviour | Held by |
|---|---|---|
| T1 | `typeexpr.TypeConstraint` (what doze reads a variable's `type` with) and `typeexpr.Type`: keywords, `list/set/map/object/tuple`, `optional(type)` in constraints, every refusal with Go's detail and range | `tests/oracle_typeexpr.rs`: 112 expressions in both modes, identical |
| T2 | A redefined attribute in `object({...})` is refused ("must be unique"), which needs the redefined item hcl-edit used to drop (vendor patch 6) | `te.039`, `te.110` |
| T3 | Not ported: `optional(type, default)` (Terraform's defaults); doze does not use it, and Go refuses its second argument in constraint mode too | `te.036` |

## Diagnostics (H2: `src/diag.rs`)

| # | Behaviour | Held by |
|---|---|---|
| D1 | Positions are Go's (above); a diagnostic can be rendered with its line and the subject underlined, and duplicates (same summary, detail and subject) dropped, as doze's `dedupeDiags` does | `diag::tests` |
| D2 | Did-you-mean is hcl's `nameSuggestion` (Levenshtein under 3) | `eval::suggestion`; `te.*`, `h1idx.25` |

## Format, edit, decode (H3: `src/fmt.rs`, `src/edit.rs`, `src/decode*`)

| # | Behaviour | Held by |
|---|---|---|
| H1 | `fmt` is `hclwrite.Format` on Go's tokens: indentation from the brackets each line opens and closes, one space or none between tokens by hclwrite's rules (the minus heuristic, `in`, `${ {`), `=` and trailing comments aligned over consecutive lines by columns counted in Go's grapheme clusters (Unicode 15.0: no GB9c conjuncts; a malformed UTF-8 sequence is one), spaces before end of file kept, a leading BOM turned into spaces | `tests/oracle_fmt.rs`: 177/177 sources, 5,827/5,827 syntax cases (random, broken, mutated), 412 audit cases (Indic text), 7 raw-byte files |
| H2 | `edit` does what doze's Edit does with hclwrite: AppendBlock (the snippet formatted, its lead comments kept, a newline first when the file has items) and SetStack (appended to an empty body, otherwise `stack = "…"` and a blank line before the file); the whole file is then formatted, and ends in one newline. Labels the scanner splits into several literal tokens are left out of the names, as hclwrite leaves them out. Stack names are NFC and escaped as `escapeQuotedStringLit` escapes them | `tests/oracle_edit.rs`: 1,729 + 492 (audit) + 52 (labels) cases |
| H3 | Deliberate difference: Go escapes every character `unicode.IsPrint` refuses; doze-hcl has no general-category tables and escapes controls, separators, format characters it lists, private use and non-characters, but writes unassigned code points (U+0378) and a few format characters (U+1D173) as they are | `KNOWN` in `tests/oracle_edit.rs` (6 cases) |
| H4 | `decode`: `Content`, `PartialContent` (and the rest of the body), `JustAttributes` and `MergeFiles` with hcl's diagnostics, ranges, files and did-you-mean; a one-line block's missing-item range is its attribute (also after a block comment that spans lines); a file with a BOM reads as Go reads it | `tests/oracle_decode.rs`: 691 + 1,033 (audit) reads |
| H5 | `decode::value` is gohcl's `DecodeExpression`: the expression's diagnostics, then "Unsuitable value type" at the expression's `StartRange` when the value does not convert, is not known, or is null for a target that cannot hold null | H3 audit corpus `value-adv` (87 cases, 81 identical; the 6 others differ only in where the subject ends for a negative literal and a heredoc) |

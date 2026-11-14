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
| E14 | Function calls: lookup through the scopes ("Function calls not allowed" / "Call to unknown function" with a suggestion; namespaced names), `...` expansion of a tuple, list or set, argument count, conversion to each parameter's type, then cty's call rules (null, dynamic-typed and unknown arguments). The function library itself is H2 | `h1fn.*`; `src/function.rs` |

## Syntax (`src/syntax.rs`, vendored hcl-edit)

| # | Behaviour | Held by |
|---|---|---|
| S1 | The 190 real sources are accepted or refused exactly as Go does | `tests/oracle_parse.rs` |
| S2 | Positions are Go's: 1-based line, 1-based column counted in grapheme clusters segmented token by token (a punctuation character followed by a combining mark is two columns), 0-based byte | `audit2.pos.*` | every diagnostic comparison |
| S3 | Open: where both refuse, hcl-edit places 13 of the corpus's syntax errors elsewhere than Go (an unterminated string at its start, not its end). Mapping syntax errors is H2's | counted, not hidden, by `oracle_eval.rs` |
| S4 | Rules Go's parser has and hcl-edit lacks, checked after parsing: chained legacy index `a.1.0`, an attribute splat inside another, a number literal Go cannot read (Go's number token and `1.e5` come from vendor patch 5) | `audit.idx.30`, `audit2.trav.*`, `audit2.numlit.*` |
| S5 | Deliberate difference: hcl-edit refuses more than 64 levels of nesting and 128 chained operators (stack safety); Go has no limit. No real configuration comes near | `audit3.05`–`.12` (known differences) |
| S6 | Open: hcl-edit backtracks heavily on unclosed nested `[ {` (a 1.3 KB fuzz input takes 1.3 s, the same in upstream 0.10.0; `fuzz_parse` timed out on it under load). Not a crash; a slow parse of a hostile file. To fix in the parser when H2 maps syntax errors | fuzz artifact `timeout-82aa6510…` |

Not in H1 (H2): the function library, `typeexpr`, the diagnostics renderer and the syntax-error mapping.

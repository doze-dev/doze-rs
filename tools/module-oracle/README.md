# The Go module oracle

Step 5 of `docs/port/modules/PLAN.md`. The Go modules (`doze-modules`, branch `pgext`, SDK v0.11.0) are the reference
the Rust ports are checked against, case by case.

- `main.go`: runs the six Go modules in-process. `oracle describe` prints each module's published description;
  `oracle run corpus.jsonl` decodes each block the way the Go plugin server does and records the connection string,
  attributes, objects and plan against a fixed instance (`/tmp/dzo/...`, `t.shop.doze`, `127.0.0.1:7777`, toolchain
  `/tc/bin` at `9.9.9`). A plan's environment is recorded as what the module adds to the inherited one.
- `corpus.ts`: generates the cases from the descriptions: the example, every version, every argument alone (two values)
  and together, pairs, each nested block (two, a duplicated label, labelled and unlabelled, with every argument), version
  gates, an unknown argument, a type mismatch.
- `corpus.jsonl`, `golden.jsonl`: 184 cases (postgres 71, ferret 32, mariadb 24, valkey 21, temporal 19, kvrocks 17);
  126 decode and plan, 58 are refused. Committed, so the Rust comparison runs anywhere without Go.
- The comparison itself is `doze_module_test::oracle::compare`: refusals must agree (not their wording), and so must the
  connection variable and URL, objects (kind and name), and each planned process's program, arguments, added
  environment, order, readiness check and hooks. A module lists deliberate departures (a Go bug it fixes) with reasons.
  The answers were recorded on Unix; path separators are compared as `/` so the same check holds on Windows.

Regenerate (needs `../../../doze-modules` and Go):

```
(cd tools/module-oracle && GOFLAGS=-mod=mod GOPROXY=off go build -o /tmp/oracle .)
/tmp/oracle describe > /tmp/describe.jsonl
bun tools/module-oracle/corpus.ts /tmp/describe.jsonl > tools/module-oracle/corpus.jsonl
/tmp/oracle run tools/module-oracle/corpus.jsonl > tools/module-oracle/golden.jsonl
```

## What building it found in the Go modules

- **Descriptions that do not match the decoders** (the registry's docs are generated from them): ferret lists `seed`,
  `collection` and `index` as top-level, but they exist only inside `database`; postgres describes `grant` as labelled
  ("role"), but a grant takes no label. In Rust both come from one `#[derive(Spec)]` type and cannot disagree.
- **kvrocks's published example does not decode without the core**: it uses an expression only the core can evaluate,
  so a module's own `boot_example` could never test it.
- These modules predate declarable listeners (SDK v0.12): every one has only the default `main`.

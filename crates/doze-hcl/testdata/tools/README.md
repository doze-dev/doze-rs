# Regenerating Go's side of the doze-hcl corpora

Go's answers come from doze's `tools/oracle-dump` on the `rust-oracle` branch (never on main). Build it
once: `go build -o /tmp/oracle-dump ./tools/oracle-dump` in a doze checkout of `rust-oracle`. Run every
command from this directory with bun, and with an empty environment for the oracle (`env -i
PATH=/usr/bin:/bin`), as the scripts do.

| Corpus | Regenerate |
|---|---|
| `syntax/` (tokens and syntax diagnostics: random fragments, mutants of `sources/`, the sources, every eval expression) | `bun gen-mut.ts ../.. 1500 11 && bun build-syntax-corpus.ts ../.. /tmp/oracle-dump && bun compact.ts ../syntax` |
| `typeexpr/` | `bun gen-typeexpr.ts ../typeexpr/exprs.json && env -i PATH=/usr/bin:/bin /tmp/oracle-dump typeexpr ../typeexpr/exprs.json > ../typeexpr/go.jsonl` |
| `eval/go-<set>.jsonl` | `env -i PATH=/usr/bin:/bin /tmp/oracle-dump eval ../eval/exprs-<set>.json > ../eval/go-<set>.jsonl` |

The generators are seeded, so the same command gives the same corpus.
| `fmt/*.hcl` (each accepted source formatted) | `/tmp/oracle-dump fmt ../sources ../fmt` |
| `fmt/go-cases.jsonl` (every syntax case formatted, as hashes) | `/tmp/oracle-dump fmtcases ../syntax/cases.json > /tmp/f.jsonl && bun fmthash.ts /tmp/f.jsonl ../fmt/go-cases.jsonl` |
| `edit/go.jsonl` (`doze add` and `doze init` edits) | `env -i PATH=/usr/bin:/bin /tmp/oracle-dump editcases ../edit/cases.json > ../edit/go.jsonl` |
| `decode/` (bodies against schemas, merged files) | `bun gen-content.ts ../.. > ../decode/cases.json && env -i PATH=/usr/bin:/bin /tmp/oracle-dump content ../decode/cases.json > ../decode/go.jsonl` |

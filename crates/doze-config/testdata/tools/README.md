# The H4 oracle corpus

`testdata/oracle/files/*.hcl` holds 224 doze.hcl sources, deduplicated by hash (`index.json` says where each came from):
- every Go raw string in doze's `internal/config/*_test.go` that looks like a doze.hcl;
- every `hcl` or `terraform` fence in the README, CONTRIBUTING, `docs/getting-started.md` and the website docs;
- `examples/*/doze.hcl`.

`go-config.jsonl` is Go's answer for each file: the loaded `Config` as JSON, or the diagnostics it is refused with, positions included. Go accepts 106 of the files and refuses 118. Absolute paths are replaced: the home by `<home>`, the files' folder by `<dir>`.

To regenerate, from a doze checkout on `rust-oracle` (build `tools/oracle-dump` first):

    bun extract.ts <doze> ../oracle
    cd ../oracle/files && env -i PATH=/usr/bin:/bin HOME=<scratch home> <oracle-dump> config $(ls *.hcl) | sed -e "s|<scratch home>|<home>|g" -e "s|$PWD|<dir>|g" > ../go-config.jsonl

# The H4 oracle corpus

`oracle/files/*.hcl` holds 257 doze.hcl sources, deduplicated by hash (the id is the first 10 hex digits of the source's sha256; `index.json` says where each was first seen):

- every Go raw string in doze's `internal/config/*_test.go` that looks like a doze.hcl (118; `from` is the bare file name, as in the first corpus);
- every `hcl` or `terraform` fence in doze's README, CONTRIBUTING, `docs/getting-started.md` and `website/src/content/docs` (105);
- doze's `examples/*/doze.hcl` (1);
- Go raw strings from every other `*_test.go` in the doze checkout (root package, `cmd/doze`, `internal/*`, `engine/*`, `test/*`; 32, `from` = `doze/<path>`). These pass a stricter filter that drops Go code, JSON, shell and prose. Some are Go format templates (`port = %d`) and are kept as written; Go refuses them;
- every `hcl`/`terraform` fence in doze-website's `src` content files (`from` = `doze-website/<path>`). All of them are already in doze's in-repo website copy, so this source adds no new files today;
- every `doze.hcl` and `*.doze.hcl` under doze-examples (`from` = `doze-examples/larder`; 1).

All sources must mention `module`, `app`, `service`, `task`, `stack`, `variable` or `locals`. The first 224 entries are the original corpus with the same ids and `from`.

`go-config.jsonl` is Go's answer for each file in `ls *.hcl` order: the loaded `Config` as JSON, or the diagnostics it is refused with, positions included. `go-shape.jsonl` is the same for `config -shape`. Go accepts 120 and refuses 137 in `config` mode; it accepts 167 and refuses 90 in `-shape` mode. Absolute paths are replaced: the home by `<home>`, the files' folder by `<dir>`. `DataDir` has a project hash of the files folder's absolute path in it , which no path scrub removes. After the commands below it is masked as `files-<slug>` in both files (`s/files-[0-9a-f]{8}/files-<slug>/g`), and the Rust test masks its own the same way.

To regenerate, from a doze checkout on `rust-oracle` (build `tools/oracle-dump` first):

    bun extract.ts <doze> <doze-website> <doze-examples> ../oracle
    H=<fresh empty scratch home>; RH=$(cd $H && pwd -P)
    cd ../oracle/files; RD=$(pwd -P)
    S=(-e "s|$RH|<home>|g" -e "s|${RH#/private}|<home>|g" -e "s|$H|<home>|g" -e "s|$RD|<dir>|g" -e "s|${RD#/private}|<dir>|g" -e "s|$PWD|<dir>|g")
    env -i PATH=/usr/bin:/bin HOME=$H <oracle-dump> config $(ls *.hcl) | sed "${S[@]}" > ../go-config.jsonl
    env -i PATH=/usr/bin:/bin HOME=$H <oracle-dump> config -shape $(ls *.hcl) | sed "${S[@]}" > ../go-shape.jsonl

The realpath and the variant without the `/private` prefix matter on macOS, where the temporary folder is a link. Afterwards `cargo test -p doze-config --test no_local_paths` must pass: it fails on any home-folder or temporary-folder path under any `testdata` in the workspace. (`files/6ee88236e1.hcl` holds a literal `data_dir = "/tmp/ephemeral"`, which is the test source, not a leak.)

## The multi-file corpus

`oracle/multi/<case>/` holds 13 stacks of several files each, loaded as a folder: overrides (`*.override.doze.hcl`, and each way one is refused), siblings merged (a duplicate argument, a second `defaults`), `*.auto.doze.vars`, and a folder with no `.hcl`. `go-multi.jsonl` and `go-multi-shape.jsonl` are Go's answers, made by `bun multi.ts <oracle-dump> <fresh empty scratch home>`; the data folder's hash is masked as `multi-<slug>`. The oracle's generic module driver cannot read an overridden module body, so the module override is tested on the Rust side only (`tests/differences.rs`).

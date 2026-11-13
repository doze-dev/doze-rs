#!/bin/sh
# Regenerates the recorded Go fixtures that doze-projects and doze-journal test against.
# Usage: tools/doze-oracle/fixtures.sh [doze checkout, default ../doze beside doze-rs]
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
doze=${1:-$root/../doze}

j="$root/crates/doze-journal/tests/fixtures"
rm -f "$j"/journal.jsonl* "$j"/go-*.jsonl
"$here/run.sh" "$doze" journal "$j/journal.jsonl" > "$j/go-read.jsonl"
"$here/run.sh" "$doze" lines "$j/edges.txt" > "$j/go-edges.jsonl"

p="$root/crates/doze-projects/tests/fixtures"
rm -rf "$p/home" "$p"/go-list.jsonl "$p"/slugs.txt && mkdir -p "$p"
"$here/run.sh" "$doze" projects "$p/home" > "$p/go-list.jsonl"
"$here/run.sh" "$doze" slug /code/shop/doze.hcl "/Users/me/My Shop!/doze.hcl" /a/b/../c/./doze.hcl \
	"/x/__/doze.hcl" /doze.hcl "/code/Ünïcode app/doze.hcl" "/code/trailing/" "" > "$p/slugs.txt"

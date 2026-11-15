#!/bin/sh
# Runs the doze oracle (main.go) inside a doze checkout, through a build overlay, so it can use doze's internal
# packages without changing the checkout. Usage: run.sh <doze checkout> <command> <args...>
set -eu
here=$(cd "$(dirname "$0")" && pwd)
doze=$(cd "$1" && pwd)
shift
overlay=$(mktemp)
trap 'rm -f "$overlay"' EXIT
printf '{"Replace":{"%s/cmd/doze-rs-oracle/main.go":"%s/main.go"}}' "$doze" "$here" > "$overlay"
cd "$doze"
exec go run -overlay "$overlay" ./cmd/doze-rs-oracle "$@"

#!/usr/bin/env bash
# Module acceptance (docs/port/modules/PLAN.md D12): each module boots the engine build doze-binaries publishes, on
# every target the module declares (ci.yml `modules` on Linux, and part of the one `macos` job). DOZE_ACCEPTANCE turns
# a missing build into a failure, so a skip cannot pass for a pass; a release's `--accepted` may only come from here.
# Usage: modules.sh <target triple>. Needs GH_TOKEN for the downloads.
set -euo pipefail
TARGET=${1:?usage: modules.sh <target triple>}
step() { echo "::endgroup::"; echo "::group::$*"; "$@"; }
echo "::group::start"
export DOZE_ACCEPTANCE=1

# Every major a module declares is accepted on its newest build: valkey 9 and 8, kvrocks 2.
VALKEY=9.1.1 VALKEY8=8.1.9 KVROCKS=2.16.0 TEMPORAL=1.8.2
MARIADB=11.4.13 MARIADB12=12.2.2 MARIADB118=11.8.9
# ferret is two builds: the FerretDB gateway (the version a block names) and PostgreSQL with DocumentDB (pinned).
FERRETDB=2.7.0 DOCUMENTDB=0.112-0
# postgres: each major's newest build; the release is `postgres`, its files `postgresql-<version>-<target>`.
POSTGRES18=18.7.0 POSTGRES17=17.12.0 POSTGRES16=16.16.0

engines=${RUNNER_TEMP:-${TMPDIR:-/tmp}}/engines
# fetch <variable> <release> <file prefix>: download, check and unpack one build; <variable> names its bin/.
fetch() {
  local var=$1 release=$2 base=$3 dir="$engines/$3" art="$3-$TARGET.tar.gz"
  mkdir -p "$dir"
  gh release download "$release" -R doze-dev/doze-binaries -p "$art" -p "$art.sha256" -D "$dir" --clobber
  (cd "$dir" && shasum -a 256 -c "$art.sha256")
  tar -xzf "$dir/$art" -C "$dir"
  printf -v "$var" '%s' "$dir/$base-$TARGET/bin"
  export "${var?}"
}
echo "::endgroup::"; echo "::group::the engine builds from doze-binaries"
fetch DOZE_VALKEY_BINDIR valkey "valkey-$VALKEY"
fetch VALKEY8_BINDIR valkey "valkey-$VALKEY8"
fetch DOZE_KVROCKS_BINDIR kvrocks "kvrocks-$KVROCKS"
fetch DOZE_TEMPORAL_BINDIR temporal "temporal-$TEMPORAL"
fetch DOZE_MARIADB_BINDIR mariadb "mariadb-$MARIADB"
fetch MARIADB12_BINDIR mariadb "mariadb-$MARIADB12"
fetch MARIADB118_BINDIR mariadb "mariadb-$MARIADB118"
fetch DOZE_FERRET_BINDIR ferretdb "ferretdb-$FERRETDB"
fetch DOZE_DOCUMENTDB_BINDIR documentdb "documentdb-$DOCUMENTDB"
fetch DOZE_POSTGRES_BINDIR postgres "postgresql-$POSTGRES18"
fetch POSTGRES17_BINDIR postgres "postgresql-$POSTGRES17"
fetch POSTGRES16_BINDIR postgres "postgresql-$POSTGRES16"

step cargo test -p doze-module-valkey -p doze-module-kvrocks -p doze-module-temporal -p doze-module-mariadb \
  -p doze-module-ferret -p doze-module-postgres -p doze-names -p doze-hcl --no-fail-fast -- --nocapture
# valkey's acceptance on its other major (8).
DOZE_VALKEY_BINDIR=$VALKEY8_BINDIR step cargo test -p doze-module-valkey --test acceptance --no-fail-fast -- --nocapture
# mariadb's acceptance on its newest line (12.2), then its middle line (11.8).
DOZE_MARIADB_BINDIR=$MARIADB12_BINDIR step cargo test -p doze-module-mariadb --test acceptance --no-fail-fast -- --nocapture
DOZE_MARIADB_BINDIR=$MARIADB118_BINDIR step cargo test -p doze-module-mariadb --test acceptance --no-fail-fast -- --nocapture
# postgres's acceptance on 16 (the oldest major the published example is promised on), then 17.
pg=(-p doze-module-postgres --test acceptance --test wire --test declared --no-fail-fast -- --nocapture)
DOZE_POSTGRES_BINDIR=$POSTGRES16_BINDIR step cargo test "${pg[@]}"
DOZE_POSTGRES_BINDIR=$POSTGRES17_BINDIR step cargo test "${pg[@]}"

# The kafka and aws module faces (core plan M3) against real clients: kcat through the granted address, and the aws CLI.
echo "::endgroup::"; echo "::group::kcat and the aws CLI"
if [ "$(uname -s)" = Linux ]; then
  command -v kcat >/dev/null || { sudo apt-get update -qq && sudo apt-get install -y -qq kcat; }
  if ! command -v aws >/dev/null; then
    curl -fsSL "https://awscli.amazonaws.com/awscli-exe-linux-$(uname -m).zip" -o "$engines/awscli.zip"
    unzip -q -o "$engines/awscli.zip" -d "$engines" && sudo "$engines/aws/install" >/dev/null
  fi
else
  command -v kcat >/dev/null || brew install kcat >/dev/null
  command -v aws >/dev/null || brew install awscli >/dev/null
fi
step cargo test -p doze-kafka --test module_kcat --no-fail-fast -- --nocapture
step cargo test -p doze-aws --test module_boot --test module_godata --no-fail-fast -- --nocapture
echo "::endgroup::"

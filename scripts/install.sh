#!/bin/sh
# Installs doze, the CLI-only binary, from doze-dev/doze's GitHub releases (docs/port/core/RELEASE.md).
#
#   curl -fsSL https://raw.githubusercontent.com/doze-dev/doze-rs/main/scripts/install.sh | sh
#
# It picks the build for this OS and architecture (macOS on Apple silicon, Linux on x86_64 or arm64), checks it
# against the release's checksums.txt, and puts `doze` in ~/.local/bin. It never asks for root: a directory you
# cannot write is an error that says so, not a sudo prompt.
#
#   DOZE_INSTALL_DIR   where doze goes (default: ~/.local/bin)
#   DOZE_VERSION       a version to install, 0.3.0 or v0.3.0 (default: the latest release)
#
# Through a pipe, pass the same as arguments: `| sh -s -- --version 0.3.0 --dir /opt/doze/bin`.
# The desktop app is the Doze download (https://doze-dev.com/download) or `brew install --cask doze-dev/tap/doze`.
set -eu

REPO=doze-dev/doze

die() {
	echo "install.sh: $*" >&2
	exit 1
}

have() { command -v "$1" >/dev/null 2>&1; }

usage() {
	cat <<'EOF'
Installs doze from https://github.com/doze-dev/doze/releases.

  install.sh [--version X.Y.Z] [--dir PATH]

  --version X.Y.Z   install this release (default: the latest; or DOZE_VERSION)
  --dir PATH        install into PATH (default: ~/.local/bin; or DOZE_INSTALL_DIR)
  -h, --help        this help
EOF
}

version=${DOZE_VERSION:-}
dir=${DOZE_INSTALL_DIR:-}
while [ $# -gt 0 ]; do
	case $1 in
	--version)
		[ $# -ge 2 ] || die "--version needs a value"
		version=$2
		shift 2
		;;
	--version=*)
		version=${1#*=}
		shift
		;;
	--dir)
		[ $# -ge 2 ] || die "--dir needs a value"
		dir=$2
		shift 2
		;;
	--dir=*)
		dir=${1#*=}
		shift
		;;
	-h | --help)
		usage
		exit 0
		;;
	*) die "unknown option $1 (--help lists them)" ;;
	esac
done
if [ -z "$dir" ]; then
	[ -n "${HOME:-}" ] || die "HOME is not set; name a directory with DOZE_INSTALL_DIR or --dir"
	dir=$HOME/.local/bin
fi

# The platform, in the release's own words (doze_<version>_<os>_<arch>.tar.gz).
os=$(uname -s)
arch=$(uname -m)
case $os in
Darwin) os=darwin ;;
Linux) os=linux ;;
*) die "no doze build for $os; releases are for macOS (Apple silicon) and Linux: https://github.com/$REPO/releases" ;;
esac
case $arch in
x86_64 | amd64) arch=amd64 ;;
arm64 | aarch64) arch=arm64 ;;
*) die "no doze build for $arch; see https://github.com/$REPO/releases" ;;
esac
[ "$os/$arch" != darwin/amd64 ] || die "doze runs on Apple silicon Macs only (this one is Intel)"

if have curl; then
	get() { curl --proto '=https' --tlsv1.2 --retry 3 -fsSL "$1"; }
	save() { curl --proto '=https' --tlsv1.2 --retry 3 -fsSL -o "$2" "$1"; }
elif have wget; then
	get() { wget -qO- "$1"; }
	save() { wget -qO "$2" "$1"; }
else
	die "needs curl or wget"
fi
if have sha256sum; then
	sha256() { sha256sum "$1" | cut -d' ' -f1; }
elif have shasum; then
	sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
	die "needs sha256sum or shasum to check the download"
fi
have tar || die "needs tar"

if [ -z "$version" ]; then
	version=$(get "https://api.github.com/repos/$REPO/releases/latest" |
		sed -n 's/^ *"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1) || true
	[ -n "$version" ] || die "could not find the latest release (GitHub's API did not answer); name one with DOZE_VERSION=X.Y.Z"
fi
version=${version#v}
asset=doze_${version}_${os}_${arch}.tar.gz
base=https://github.com/$REPO/releases/download/v$version

tmp=$(mktemp -d 2>/dev/null || mktemp -d -t doze)
trap 'rm -rf "$tmp"' EXIT
trap 'exit 1' INT TERM

echo "doze $version for $os/$arch"
save "$base/$asset" "$tmp/$asset" || die "could not download $base/$asset"
save "$base/checksums.txt" "$tmp/checksums.txt" || die "could not download $base/checksums.txt"
want=$(awk -v f="$asset" '$2 == f || $2 == "*" f { print $1 }' "$tmp/checksums.txt")
[ -n "$want" ] || die "checksums.txt does not list $asset"
got=$(sha256 "$tmp/$asset")
[ "$want" = "$got" ] || die "$asset does not match checksums.txt (expected $want, got $got); nothing was installed"
echo "checksum ok"

tar -xzf "$tmp/$asset" -C "$tmp" doze || die "$asset has no doze in it"
if ! mkdir -p "$dir" 2>/dev/null || [ ! -w "$dir" ]; then
	die "cannot write to $dir. Choose a directory you own (DOZE_INSTALL_DIR=PATH or --dir PATH), or run this again yourself as a user who can write there"
fi
# Into place by rename, so a doze that is running keeps its old file and the new one is whole.
cp "$tmp/doze" "$dir/.doze.new"
chmod 0755 "$dir/.doze.new"
mv -f "$dir/.doze.new" "$dir/doze"
echo "installed $dir/doze"
"$dir/doze" version || echo "install.sh: warning: $dir/doze did not run; is this the right build for this machine?" >&2

case ":${PATH:-}:" in
*":$dir:"*) ;;
*)
	echo
	echo "$dir is not on your PATH. Add it to your shell's profile:"
	# shellcheck disable=SC2016 # $PATH is meant literally
	printf '  export PATH="%s:$PATH"\n' "$dir"
	;;
esac

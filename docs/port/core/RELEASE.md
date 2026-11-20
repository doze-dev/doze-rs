# Releasing the Rust doze

How a doze release is built and published from doze-rs, what each step needs, and what the owner has to do before the
first real one. Written 2026-11-19 with the pipeline itself (`.github/workflows/release.yml`,
`.github/workflows/modules-release.yml`, `scripts/install.sh`, `packaging/`).

## Where releases go

The owner's decision (2026-11-19): the Rust doze is published to **doze-dev/doze**, the repository and URLs Go's
releases used. So nothing that points at doze changes:

- the release files keep Go's names: `doze_<v>_darwin_arm64.tar.gz`, `doze_<v>_linux_amd64.tar.gz`,
  `doze_<v>_linux_arm64.tar.gz`, `checksums.txt`, `Doze-<v>.dmg`, `Doze.app.zip`;
- the app's update check asks `api.github.com/repos/doze-dev/doze/releases/latest` (doze-desk `settings/update.rs`),
  and the CLI's "update by hand" link is `github.com/doze-dev/doze/releases` (doze-cli `install.rs`); a test in each
  crate holds them to the repository `release.yml` publishes to;
- the Homebrew tap (doze-dev/homebrew-tap) keeps `Formula/doze.rb` and gains `Casks/doze.rb`.

doze-rs CI builds; a token with write access on doze-dev/doze uploads there.

Modules are different: each goes to its own rolling release **in doze-rs** (see "Modules" below).

## What a release contains

| File | Built by | Notes |
|---|---|---|
| `doze_<v>_darwin_arm64.tar.gz` | job `cli`, macos-latest | `doze` (profile `release-doze`), `LICENSE`, `NOTICE` |
| `doze_<v>_linux_amd64.tar.gz`, `_linux_arm64` | job `cli`, ubuntu-22.04 and ubuntu-22.04-arm | built on 22.04 so the glibc it asks for stays old; the job prints the newest `GLIBC_` symbol |
| `Doze-<v>.dmg`, `Doze.app.zip` | job `app-macos` | Doze.app from `cargo xtask app-bundle --id dev.doze.app --cli <doze>`: `Contents/MacOS/doze-app` (the GUI and the whole CLI, O1) and `Contents/MacOS/doze` (the CLI-only binary, identifier `dev.doze.cli`, which the cask links onto the PATH), each signed with the hardened runtime; the bundle and the image are notarised and stapled |
| `doze_<v>_amd64.deb`, `doze-<v>-1.x86_64.rpm` (and arm64) | job `linux-packages`, nfpm | `/usr/bin/doze`; config `packaging/linux/nfpm-doze.yaml` |
| `doze-app_<v>_amd64.deb`, `doze-app-<v>-1.x86_64.rpm` (and arm64) | job `linux-packages`, **gated** | `/usr/bin/doze-app`, `doze.desktop` (the `doze://` scheme), the icon; depends on `doze` and GPUI's libraries; config `packaging/linux/nfpm-doze-app.yaml` |
| `checksums.txt` | job `assemble` | `sha256sum` over every file above; `install.sh` checks against it |
| `Formula/doze.rb`, `Casks/doze.rb` | job `assemble` | from `packaging/homebrew/*.tmpl`; a real release opens a pull request on the tap with them |

`doze version` (and `doze-app version`) say the release's version: the workflow builds with `DOZE_VERSION=<v>`, without
the `v`, as Go's `doze 0.1.1` did. Each build job checks it.

### The Linux app gate

The Linux desktop app ships at the cut-over (owner, 2026-11-19), but it does not run on Linux yet: another builder is
making it. Its build and packages are therefore behind a gate, off by default:

- the `linux_app` input of a manual run, or
- the repository variable `DOZE_LINUX_APP=true` (for tag-started releases).

With the gate off, the Linux packages are the CLI's only and the run says so in a notice. With it on, `linux-packages`
installs GPUI's development libraries, builds `doze-app` with `release-app`, prints its `ldd` (check the `depends` list
in `nfpm-doze-app.yaml` against it the first time), and packages it. Unchecked until the app builds on Linux: the
`depends` names, `StartupWMClass` in `doze.desktop` (it must match the window's app id), and whether the app needs
anything else installed (Go's package carried an AppArmor profile for WebKit's sandbox; GPUI starts no sandbox, so
there is none).

Turn the gate on for good (the variable) once the Linux app passes its checklist; then remove the gate.

## How to cut a release

1. Main is green, and the version is decided (`X.Y.Z`; numbers only, as Doze.app's bundle version must be).
2. A dry run of exactly that commit, to see every file built:
   `gh workflow run release --ref main -f dry_run=true -f version=X.Y.Z`. Its artifacts are `release-X.Y.Z` (every
   file and `checksums.txt`) and `homebrew-X.Y.Z` (the formula and cask); the `assemble` job's log shows the diff each
   would make to the tap.
3. Tag and push: `git tag vX.Y.Z && git push origin vX.Y.Z`. The tag starts `release.yml` for real:
   - `plan` refuses to start unless every secret below is set and doze-dev/doze's own (Go) release workflow is
     disabled;
   - the builds run as in the dry run, but signing and notarising are required;
   - `publish` creates release `vX.Y.Z` in doze-dev/doze with every file (it refuses if that release exists; a
     published version is never replaced);
   - `tap` opens a pull request on doze-dev/homebrew-tap with the formula and cask. Merge it by hand.
   A manual run with `dry_run=false` does the same for its `version` input.
4. Check: `curl -fsSL https://raw.githubusercontent.com/doze-dev/doze-rs/main/scripts/install.sh | sh` on a Mac and a
   Linux machine, `brew install doze-dev/tap/doze` and `brew install --cask doze-dev/tap/doze` once the tap pull
   request is merged.

What a release does not do yet: the CLI's ambient update line reads the registry's signed `latest.yaml`
(compat.md rule 7; doze-fetch `registry/latest.rs`), which doze-registry does not publish yet. Until a step in its
`publish.mjs` writes it, `doze update` finds nothing newer and says nothing; the app's About page (GitHub's API) does see
the release.

## Secrets and who adds them

None of these exist in doze-rs today (checked 2026-11-19; the only secrets visible to it are the organisation's
Cloudflare pair). A dry run needs none of them.

| Secret | Used by | What it is | Who |
|---|---|---|---|
| `MACOS_CERT_P12` | `app-macos` (and ci.yml's `app`) | the Developer ID Application certificate and key, `.p12`, base64 | owner |
| `MACOS_CERT_PASSWORD` | `app-macos` | the `.p12`'s password | owner |
| `DOZE_SIGN_ID` | `app-macos` | the identity's name, `Developer ID Application: … (TEAMID)` | owner |
| `NOTARY_KEY_P8` | `app-macos` | an App Store Connect API key (`.p8` text) for `notarytool` | owner |
| `NOTARY_KEY_ID`, `NOTARY_ISSUER_ID` | `app-macos` | that key's id and issuer | owner |
| `DOZE_RELEASE_TOKEN` | `plan`, `publish` | a fine-grained token, repository doze-dev/doze only, **Contents: read and write** (and Actions: read, for the workflow check) | owner |
| `HOMEBREW_TAP_TOKEN` | `tap` | a fine-grained token, repository doze-dev/homebrew-tap only, **Contents** and **Pull requests: read and write** | owner |

The Go repository used other names for the signing secrets (`DEVELOPER_ID_P12`, `NOTARY_KEY`, …) but has none of them
set either; doze-rs uses ci.yml's names so one set serves both the CI `app` job and releases. One token may serve both
`DOZE_RELEASE_TOKEN` and `HOMEBREW_TAP_TOKEN` if it is scoped to both repositories, but two narrow ones are safer.

`modules-release.yml` needs no secret: it uploads to doze-rs with `github.token`. The registry hook it leaves commented
needs `REGISTRY_DISPATCH_TOKEN` (Contents: read and write on doze-dev/doze-registry) when it is turned on.

## Owner steps before the first real release

1. Add the seven secrets above to doze-dev/doze-rs (Settings › Secrets and variables › Actions).
2. **Disable Go's release workflow in doze-dev/doze**: `gh workflow disable release -R doze-dev/doze`. Publishing
   creates the tag `vX.Y.Z` there, and a tag made with a personal token starts that repository's workflows: Go's
   `release.yml` would run goreleaser and upload Go's tarballs and `checksums.txt` over the Rust ones. The `plan` and
   `publish` jobs refuse to run while it is active.
3. Decide what the `vX.Y.Z` tags in doze-dev/doze point at. `gh release create` tags doze-dev/doze's default branch
   (frozen Go code); the release notes name the doze-rs commit it was built from. If the tags should mean something in
   that repository (a pointer commit, an archive notice), that is a change there.
4. Give the tap a `Casks/` directory's first file by merging the first tap pull request (the tap has only
   `Formula/doze.rb` today, generated by goreleaser; the new formula replaces it).
5. Optionally set the repository variable `DOZE_LINUX_APP=true` once the Linux app runs (above).
6. Point the website's `https://doze-dev.com/install.sh` (a placeholder on the download page today) at
   `scripts/install.sh`, if the short URL is wanted.

## install.sh

`scripts/install.sh` (POSIX sh, shellcheck clean) installs the CLI-only doze:

- picks `darwin/arm64`, `linux/amd64` or `linux/arm64` from `uname` (Intel Macs are refused, as Go's releases were);
- resolves the latest release through GitHub's API, or takes `DOZE_VERSION` / `--version`;
- downloads the tarball and `checksums.txt` from doze-dev/doze and refuses a file that does not match;
- installs to `~/.local/bin`, or `DOZE_INSTALL_DIR` / `--dir`, by rename (a running doze keeps its old file);
- never uses sudo: an unwritable directory is an error naming the fix; a directory not on `PATH` is printed, not
  written into any profile.

Until the first Rust release, "latest" is Go's v0.1.1 (same file names), which it installs correctly.

## Modules

`.github/workflows/modules-release.yml` releases one module: postgres, mariadb, ferret, valkey, kvrocks, temporal, and
kafka and aws, which are the module faces of the doze-kafka and doze-aws binaries (their acceptance is the binary's unit
tests and `tests/module_boot.rs`; they need no engine from doze-binaries, and answer `--describe` under their plugin
name):

- trigger: a tag `module/<name>/v<X.Y.Z>`, or a manual run with `module`, `version` and `dry_run` (default true);
- on each target (x86_64 and aarch64 Linux on 22.04, aarch64 macOS) it downloads the engine builds from
  doze-binaries (the same versions as ci.yml's `modules` job: keep the two in step), runs the module's tests with
  `DOZE_ACCEPTANCE=1`, and builds the module;
- `package` fetches the module's published `index.yaml` (404: a first release; any other failure stops the run, or
  older versions would be dropped from the index) and runs `doze-module-release --accepted` once per target, the
  x86_64 Linux binary answering `--describe`; the result (archives, `index.yaml`, `meta.yaml`) is the run's artifact;
- `publish` (real runs) uploads it to the module's rolling release **in doze-dev/doze-rs, tag = the module's name**,
  creating it the first time.

### The hand-off to doze-registry

doze-registry signs; doze-rs only uploads. Since doze-registry #7 (2026-11-21) its `publish.yml` reads each module from
its rolling release here (`https://github.com/doze-dev/doze-rs/releases/download/<name>`: `index.yaml`, `meta.yaml` and
the archives), and its hourly schedule is back: it lists this repository's bare-name releases, so a new module or a
new version is signed within the hour with no token in doze-rs. To sign at once:

    gh workflow run publish.yml -R doze-dev/doze-registry -f modules="<name> ..."

The commented dispatch step at the end of the `publish` job is the faster path; it needs `REGISTRY_DISPATCH_TOKEN`
(owner), which does not exist.

Versions: the Rust modules start at **0.3.0**, above every Go-built version (the highest was aws 0.2.0, postgres
0.2.4). Those Go versions are withdrawn (doze-registry `withdrawn.yaml`, which now names versions) and may never be
published again. Every Rust release is protocol 2; the Go doze takes only protocol-1 releases, so it never selects one.

## Testing the pipeline

GitHub starts a manual run only of a workflow that is already on main, so a change to either workflow is tried on its
pull request with a label: `release-dry-run` runs `release.yml` as a dry run of the pull request's code, and
`modules-release-dry-run` runs `modules-release.yml` as a dry run of valkey. (Like any label, each also starts ci.yml
with nothing to do; put `ci` on last.) Once on main:

- `gh workflow run release --ref <branch> -f dry_run=true` builds every file and keeps it as the run's artifacts; with
  no signing secrets on the branch it signs ad hoc and says so.
- `gh workflow run modules-release --ref <branch> -f module=valkey -f version=0.0.1 -f dry_run=true` likewise.
- Never push a `v*` or `module/*` tag to try something: tags are real releases.

# Behaviours: fetching (phase F1)

From Go's `internal/modules` and `internal/binaries` tests at doze `origin/main 23b73d6`. Each Go test names the Rust
test that holds the same behaviour, or why it is elsewhere.

## Modules (`doze_fetch::modules`)

| Go test | Behaviour | Rust test |
|---|---|---|
| `TestResolveSignedModule` | a signed index and archive resolve to the plugin, cached and pinned | `modules::tests::a_fresh_resolve_pins_the_newest_fit_and_a_warm_pin_needs_no_network` |
| `TestSelectionPrefersNewestCompatible` | the stable head if it fits, else the newest that fits | same; `registry::select::tests::picks_as_the_go_doze_does` |
| `TestSelectionSkipsWhatThisDozeCannotRun` | releases needing a newer doze are left out; when nothing is left, which doze to get | `registry::select::tests::picks_as_the_go_doze_does` |
| `TestEngineSupportGates` | a pin that does not run a declared major says to upgrade | `modules::tests::a_pin_is_held_to_its_engines_and_its_checksum` |
| `TestProtocolGates` | a release of another protocol is never taken; nothing for this protocol says so | `modules::tests::nothing_published_for_this_protocol_says_so` |
| `TestUpgradeMovesPin` | `modules upgrade` re-resolves ignoring the pin and moves it | `modules::tests::an_upgrade_moves_the_pin_to_the_newest_fit` |
| `TestRejectUnsignedIndex`, `TestRejectRegistrySignedByAnotherKey` | an index without a signature, or signed by another key, is refused | `modules::tests::an_unsigned_index_or_one_signed_by_another_key_is_refused` |
| `TestRejectTamperedMetadata` | a changed release (protocol, engines, checksums) fails the index signature; a changed archive fails its checksum | `modules::tests::a_tampered_archive_or_index_is_refused` |
| `TestOneRegistryOneNamespace` | modules come only from `doze/…` | `modules::tests::modules_come_from_the_doze_namespace_only` |
| `TestOfflineUsesTheCache` | registry unreachable, no pin: the newest cached build, unpinned, with a note | `modules::tests::offline_with_no_pin_the_newest_cached_build_will_do` |
| `TestUseLocalPluginIsScopedAndSkipsSupportCheck` | a test's locally built module | not ported: Rust modules are tested through `doze-module-test`, which runs a module binary directly |

## Engine builds and archives (`doze_fetch::mirror`, `archive`)

| Go test | Behaviour | Rust test |
|---|---|---|
| `TestManifestParse` | the doze-binaries manifest | `mirror::tests::reads_the_manifest`, `fuzz_manifest` |
| `TestEnsureDownloadTarGzWithOneTopLevelDirectory` | a nested `bin/` comes to the top; the archive's sha is kept | `archive::tests::a_nested_bin_comes_to_the_top_and_the_sha_is_kept` |
| `TestAChangedDownloadIsRefused`, `TestCacheHitHonoursTheLock` | a download or a cached build that differs from the lock's pin is refused | `mirror::tests::a_build_is_fetched_verified_cached_and_held_to_its_pin` |
| `TestADownloadCannotEscapeItsDirectory` | an archive entry naming `../…` is not written outside | `archive::tests::an_archive_cannot_write_outside_its_folder` |
| `TestEnsureDownloadZipAndBareBinary`, `TestTheLockPinsADownloadOnce`, `TestADownloadStopsWithItsContext` | `download(…)` inputs of tasks (any URL, zip or bare binary) | E2 (tasks and `inputs`); the lock's `downloads` record is here (`Lock::record_download`) |

## The lock (`doze_fetch::lock`)

| Go test | Rust test |
|---|---|
| `TestLockRoundTrip`, `TestLockModulePinRoundTrip` | `tests/yaml_oracle.rs` (byte for byte against Go's yaml.v3: 1,320 scalars, 215 keys, three whole locks), `lock_tests::reads_what_go_writes` |
| `TestLockMissingFile` | `Lock::load` (a missing file is an empty lock), `lock_tests::save_writes_only_a_change_and_names_the_newest_doze` |
| `TestLockDropsPreRedesignModuleEntries` | `lock_tests::reads_what_go_writes` (a versionless pin is dropped and marks the lock dirty) |
| `TestLockResolvedChangeResetsHashes` | `lock_tests::records_change_it_only_when_something_differs` |

## Where Rust differs from Go, on purpose

| Go | Rust | Why |
|---|---|---|
| A pin to another module protocol is an error ("run doze modules upgrade") | a pin to an older protocol (a lock written by the Go doze, protocol 1) is resolved again and replaced, with a note; a newer one says to update doze | the Rust doze speaks protocol 2 only; every Go project's lock would otherwise stop working on upgrade |
| Downloads run in the daemon | in `doze __fetch`, a child per request (`ChildDownloads`) | the daemon never holds an HTTPS client or its memory (PLAN.md weight rules) |
| The registry has no override | `DOZE_REGISTRY` (hosts, `file://` or directories) | tests, mirrors, and the Pages host while `registry.doze-dev.com` has no DNS answer; the built-in key still decides what runs |
| Fields of `meta.yaml` and the index outside what it reads are ignored on read | the same, and the cache keeps the published bytes | the module contract grows (`client`, `detect`); a newer reader finds them |

## Live

`tests/live_registry.rs` (CI `os` job, `DOZE_LIVE_REGISTRY=1`): all 8 modules at https://doze-registry.pages.dev
(aws, ferret, kafka, kvrocks, mariadb, postgres, temporal, valkey) verify with the built-in key, and each one's newest
build for the runner downloads, matches its checksum and signature, and unpacks to a plugin (macOS and Linux; the
registry publishes no Windows builds yet, so Windows checks the indexes). Checked locally on macOS 2026-11-14: 8 of 8.

# Behaviours: fetching (phase F1)

From Go's `internal/modules` and `internal/binaries` tests at doze `origin/main 23b73d6`. Each Go test names the Rust
test that holds the same behaviour, or why it is elsewhere.

## Modules (`doze_fetch::modules`)

| Go test | Behaviour | Rust test |
|---|---|---|
| `TestResolveSignedModule` | a signed index and archive resolve to the plugin, cached and pinned | `modules::tests::a_fresh_resolve_pins_the_newest_fit_and_a_warm_pin_needs_no_network` |
| `TestSelectionPrefersNewestCompatible` | the stable head if it fits, else the newest that fits | same; `registry::select::tests::picks_as_the_go_doze_does` |
| `TestSelectionSkipsWhatThisDozeCannotRun` | releases needing a newer doze are left out; when nothing is left, which doze to get | `registry::select::tests::picks_as_the_go_doze_does` |
| `TestEngineSupportGates` | a pin that does not run a declared major says to upgrade; with no pin, nothing that runs the majors says, in Go's words, what the newest release supports | `modules::tests::a_pin_is_held_to_its_engines_and_its_checksum`, `modules::gates_tests::a_fresh_resolve_names_what_the_newest_release_supports`. Go's `CheckSupport` (the post-decode pass over every block of a type) reads the config's declared versions: H4, which calls the same pin gate |
| `TestProtocolGates` | a release of another protocol is never taken; nothing for this protocol says so, and releases only for a newer protocol say to update doze | `modules::tests::nothing_published_for_this_protocol_says_so`, `modules::tests::releases_for_a_newer_protocol_say_to_update_doze` |
| `TestUpgradeMovesPin` | a pin holds while a newer release appears; `modules upgrade` re-resolves ignoring the pin and moves it; a second upgrade changes nothing (the lock is not dirty) | `modules::tests::an_upgrade_moves_the_pin_to_the_newest_fit`, `modules::gates_tests::a_pin_holds_until_an_upgrade_and_a_second_upgrade_changes_nothing` |
| `TestRejectUnsignedIndex`, `TestRejectRegistrySignedByAnotherKey` | an index without a signature, or signed by another key, is refused | `modules::tests::an_unsigned_index_or_one_signed_by_another_key_is_refused` |
| `TestRejectTamperedMetadata` | a changed release (protocol, engines, checksums) fails the index signature; a changed archive fails its checksum | `modules::tests::a_tampered_archive_or_index_is_refused`, `modules::gates_tests::a_release_changed_after_signing_is_refused` (each of the three) |
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

## The `__fetch` child (`doze_fetch::child`)

| Behaviour | Rust test |
|---|---|
| A module's `fetch.ensure` names plain folder names, and a platform doze builds for (or this machine's); anything else is refused before it becomes a cache path | `child::tests::an_ensure_for_an_unknown_platform_is_refused` |
| Parallel children filling one build take its file lock (`<build>.lock`) in turn, and one that finds it there once it holds the lock takes it as cached | `modules::tests::parallel_fills_of_one_build_all_end_with_it` (fails without the lock) |
| Children resolving different modules at once all save into one `doze.lock`: each re-reads it under a lock in the doze home and changes only its own pin | `child::tests::concurrent_saves_keep_every_pin` (fails without the lock) |
| A request that persists writes the pin and says "pinned … in doze.lock — commit the lockfile …" once, after the save (Go logged it after `Save`); a read command writes nothing and says nothing | `child::tests::a_persisted_module_request_pins_it_and_says_so_once` |
| A lock written by a newer doze is said with each resolve that reads it (Go's `loadLock`) | `child::tests::a_lock_from_a_newer_doze_is_said` |
| A relative artifact address is fetched from the registry base that served the index | `modules::gates_tests::a_relative_artifact_comes_from_the_base_that_answered` |
| An unpacked `bin` must be a real folder (a link out of the build is not taken); an archive that inflates past 4 GiB is refused | `archive::tests::a_linked_bin_and_an_inflating_archive_are_refused` |
| The cache keeps a `meta.yaml` as published, fields this doze does not read included | `registry::tests::the_cache_keeps_the_published_bytes` |

## The lock (`doze_fetch::lock`)

| Go test | Rust test |
|---|---|
| `TestLockRoundTrip`, `TestLockModulePinRoundTrip` | `tests/yaml_oracle.rs` (byte for byte against Go's yaml.v3: 1,320 scalars, 215 keys, three whole locks), `lock_tests::reads_what_go_writes` |
| `TestLockMissingFile` | `modules::gates_tests::a_missing_lock_reads_as_empty` |
| `TestLockDropsPreRedesignModuleEntries` | `lock_tests::reads_what_go_writes` (a versionless pin is dropped and marks the lock dirty) |
| `TestLockResolvedChangeResetsHashes` | `lock_tests::records_change_it_only_when_something_differs` |

## Where Rust differs from Go, on purpose

| Go | Rust | Why |
|---|---|---|
| A pin to another module protocol is an error ("run doze modules upgrade") | a pin to an older protocol (a lock written by the Go doze, protocol 1) is resolved again and replaced, with a note; a newer one says to update doze | the Rust doze speaks protocol 2 only; every Go project's lock would otherwise stop working on upgrade |
| Downloads run in the daemon | in `doze __fetch`, a child per request (`ChildDownloads`) | the daemon never holds an HTTPS client or its memory (PLAN.md weight rules) |
| The registry has no override | `DOZE_REGISTRY` (hosts, `file://` or directories) | tests, mirrors, and the Pages host while `registry.doze-dev.com` has no DNS answer; the built-in key still decides what runs |
| Fields of `meta.yaml` and the index outside what it reads are ignored on read | the same, and the cache keeps the published bytes | the module contract grows (`client`, `detect`); a newer reader finds them |
| The unpack folder is `<build>.tmp` | `<build>.tmp<pid>`, beside the build | parallel children must not share it |
| A `doze.lock` that does not parse is dropped and the resolve goes on | the child fails with the parse error, naming where | a lock that is half a merge conflict must be fixed, not silently replaced |
| A pin's version is joined into the cache path | a version that is not a plain dotted name (`/abs`, `../x`) refuses the lock | an absolute version replaced the whole path and ran a file from the repo |
| A development build writes `doze: dev` | never: only a released version is stamped | `dev` compares above every release as text and would stick |

Known limits, shared with Go: a release's `doze:` field and any release field this doze does not know are outside the
signed payload (a host can strip the first and steer an older doze to a release it cannot run; a field the signer adds
later would fail verification here until the payload learns it). Engine builds are checked against the mirror's
unsigned manifest (`Mirror.key` is not set), as Go did.

## Live

`tests/live_registry.rs` (CI `os` job, `DOZE_LIVE_REGISTRY=1`): all 8 modules at https://doze-registry.pages.dev
(aws, ferret, kafka, kvrocks, mariadb, postgres, temporal, valkey) verify with the built-in key, and each one's newest
build for the runner downloads, matches its checksum and signature, and unpacks to a plugin (macOS and Linux; the
registry publishes no Windows builds yet, so Windows checks the indexes). Checked locally on macOS 2026-11-14: 8 of 8.

The same test resolves every module the way this doze will (`Modules::resolve`, protocol 2): it must take a protocol-2
release or say "no release of module … speaks module protocol 2 … it has to be published for this doze". **Every release
published today is protocol 1** (the Go doze's), so today every module says the second: protocol-2 releases come with the
M track (the Rust modules), and until they are published a project cannot run its modules on the Rust doze. This test
turns the day they are published into a visible change (it never takes a protocol-1 build).

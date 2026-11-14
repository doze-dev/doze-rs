# Phase 11: doze-kafka's module face

Status: built 2026-11-14 on branch `p11-module` (from main `2f1bb662`). The cutover decision is the owner's (last section).

## Goal, and what changed from the original Phase 11

The original row 11 was "`serve` mode (unix sockets) so doze's Go module can re-exec the Rust binary; Rust-edition docs;
inherited gates; cutover decision", done when "doze runs the Larder example on the Rust broker". That was Go/Rust stitching: the Go
kafka module (`doze-kafka/module`, go-plugin, protocol 1) would have started the Rust broker. On 2026-11-12 the owner decided
**all Rust, no Go stitching** (docs/port/modules/PLAN.md): the SDK and every module move to Rust and speak protocol 2. So Phase 11
became step 14 of the module plan: the `doze-kafka` binary itself gains its doze module face on the Rust module SDK, and the Go kafka
module is the oracle, read-only.

"doze runs the Larder example" cannot be proven yet: the Rust doze core does not exist. What stands in for it is the core's boot
sequence as `doze-module-test` implements it (launch the module, decode, network, grant, resolve, provision, plan, start with
readiness, converge when fresh, stop), run on the published example and on the Larder example's kafka block.

## Design

**One binary, two faces.** `doze-kafka` is the module when it runs under the name a module release packages it as (`bin/kafka-plugin`,
which doze starts with no arguments; `kafka-plugin --describe` prints the manifest for the release tool) or as `doze-kafka module`.
Anything else is the broker, as before. The plan starts the same binary with the broker's own flags (`--listen`, `--data-dir`,
`--kafka-version`, ...), so a `kafka-plugin` with arguments is the broker: no hidden `__serve` mode as Go had, and no second set of
flags to keep in step. The face is the cargo feature `module`, on by default like the console; `--no-default-features` leaves it out.

**Listeners.** `main` (the Kafka port), `console` (port 9093, route `@`: the instance's own name) and `registry` (8081, route
`registry`), Go's three. All three are `tcp`: the Rust broker and its console listen on TCP, not sockets (D13). Clients bootstrap at
the address doze gives them and dial what Metadata advertises, so the broker advertises that same address
(`--advertised-host/--advertised-port` from the main grant). The console is told the names doze reaches it by
(`--console-allow-host`, its DNS-rebinding guard); with no console granted the plan passes `--console=false`, so the broker does not
take 127.0.0.1:9093 for itself.

**The schema registry.** A parallel agent is building it (branch `p10-registry`); main's broker has no registry flag yet. The module
asks the broker it runs in: the registry listener is declared, and `--schema-registry-addr <backend>` planned, only when the broker's
own command line has `--schema-registry-addr` (the Go edition's flag; the Rust port keeps the flags' meaning, PLAN.md). Until then the
instance has no `registry.<name>` that would answer nothing. No attribute is involved: the Go block had none for the registry (it
always served one). Nothing to change in the module when the registry merges, as long as the flag keeps Go's name; a test
(`the_listeners_are_the_go_modules_less_the_registry_until_the_broker_has_one`) follows whichever the broker has.

**The block** (`#[derive(Spec)]`, exactly Go's arguments): `auto_create_topics` (default true), `default_partitions` (1),
`retention` (duration), `retention_bytes`, and `topic "<name>" { partitions = 1, config = { ... } }`. `version` (1-4, default 4)
and `port` are the core's. Go had no users, SASL, ACL, console or registry arguments, so neither does this.

**Connection.** `KAFKA_BROKERS` = the main listener's address, a bare `host:port`, as Go. No extra variables. Manifest:
`default_version = "4"`, `detect` with the Go core's lists (`internal/detect/detect.go`), targets: the five.

**Resolve, provision, plan.** Nothing to fetch: the toolchain is this binary at the declared major. Provision makes the data
directory (0700). The plan is one process, ready when its TCP port accepts (the broker opens its data before it binds), stopped with
SIGTERM (the broker stops cleanly on it).

**Converge.** Creates a missing topic with its partitions and configs, as Go. A topic that exists is brought to the declaration
(better than Go): fewer partitions than declared are raised (CreatePartitions); declared configs that differ are set
(DescribeConfigs, then AlterConfigs with the topic's own configs kept and the changes laid over them). Nothing is ever deleted: Kafka
cannot remove partitions, so a topic with more keeps them and the log says so; the module offers no prune, so a topic left out of the
block keeps its data. Requests use fixed versions inside what all four majors offer (`kafka-wire`'s table), over one connection to the
main backend (`src/module/client.rs`): nothing follows advertised addresses, as Go's dialer did not.

**Console and admin** (the Go module's `DescribeConsole`, `Resources`, `Actions`, `Tail`): panels `topics` (name, partitions,
messages, rate over the high-water total), `groups` (state, members, lag) and `tail` (stream of the selected topic: the last 10 of
each partition, then what arrives; values cut at 240 bytes); one action, `produce`. Each is a real query against the broker:
Metadata and ListOffsets; ListGroups, DescribeGroups and OffsetFetch; Fetch; Produce. `front = "console"` as Go; new:
`web_url = "http://{address:console}"`, so the app can open the broker's own web console. No `tabs` and no `extend`: the Go module
declared none, and the app's generic page is the fallback.

**For the app's fixture** (`apps/doze-app/fixtures/consoles.ts`, owned by another session): the kafka instance's spec and actions
should be regenerated from `crates/doze-kafka/src/module/console.snapshot.json`, which a test pins. The fixture today copies Go
doze's dash fixture `KafkaConsole()`, which is not what the Go module declared: it has lowercase titles, a `topic` column on groups and
a `keep` action ("Keep topic in doze.hcl"). The module cannot produce either honestly: a group may consume many topics (one `topic`
cell would be invented), and "keep in doze.hcl" is a core feature that edits the user's file, not a module action. Both are left
out.

## Better than Go (D16), each an acceptance test

| # | Go | Rust | test |
|---|---|---|---|
| 1 | a topic Kafka refuses (name, partition count, unknown or bad config) failed the converge after boot | refused before boot at its line, by the broker's own config table, with "did you mean" | `what_the_broker_would_refuse_is_refused_at_its_line`, `a_mistake_is_refused_before_boot_at_its_line`, oracle |
| 2 | `partitions = 0` or negative, `default_partitions = 0`, negative `retention` quietly read as defaults | refused, with the way out | same |
| 3 | `version = "3.7"` ran Kafka 4; `version = 5` failed only when the broker started | refused before boot (`version = 3` is the hint) | `a_version_is_a_kafka_major_from_1_to_4`, oracle |
| 4 | `retention = "7d"` refused (Go durations have no days) | accepted | `a_retention_is_a_go_duration_or_whole_days_and_never_negative`, oracle |
| 5 | converge only created missing topics | raises partitions, sets changed configs, keeps the topic's other configs, never removes | `converge_raises_partitions_and_sets_changed_configs_and_never_removes` |
| 6 | no objects | the declared topics are objects, so `doze plan` shows them; no prune, so nothing is deleted for being left out | oracle, `the_listeners_are...` (no PRUNE bit) |
| 7 | a topic declared twice was created once, silently | refused | oracle (`018-block-topic-duplicate`) |
| 8 | the app could not open the web console | `web_url` | snapshot |

## The oracle

`tools/module-oracle/kafka/` is a Go program of its own (the kafka module needs SDK v0.12 and the broker it embeds, the other six
modules' oracle is on v0.11) that runs `doze-kafka/module` in-process: 43 cases, 23 generated by `corpus.ts` from Go's description and
20 hand-written in `extra.ts` (real topic configs, names and counts Kafka refuses, versions, durations, the Larder block). Go's answers
are committed (`golden.jsonl`). `tests/module_oracle.rs` compares through `doze_module_test::oracle::compare`.

The two editions' brokers spell their flags differently (Go: `__serve --socket --datadir --version --advertise --console-socket`;
Rust: the broker's own flags), so every planned case is a difference by argv, and the test reads both command lines for what they
mean (data directory, major, advertised address, console, registry, auto-create, default partitions, retention in time and bytes)
and requires the same meaning, the same variable and address, the same process name, environment, order and hooks.

Result: **43 cases. 22 plan the same broker as Go** (meaning, variable, address); every one differs from Go only in the listed ways:
the declared topics as objects, TCP readiness instead of a socket, and no registry until the broker has one. **3 refused by both.**
**17 refused by Rust only**, each listed with its reason (better-than-Go items 1, 2, 3, 7; 4 of them are the generated corpus's
`tuning` config, which is no Kafka config). **1 accepted by Rust only** (`7d`). A deliberate break (dropping `--retention-ms`) fails
the test.

## What it proves and what it cannot yet

Proven on the real broker through the core's boot sequence (`doze-module-test`): the published example boots and converges (3 and 1
partitions, `cleanup.policy=compact`), its console answers on the instance's name, health follows the broker, and it stops gracefully;
**the Larder example's kafka block** boots with its four topics of three partitions, and `auto_create_topics = false` reached the
broker (a topic nobody declared is not created by asking for it); converge on a running broker; resources, lag, produce and tail;
a sleep and a wake keep the messages.

Not provable until the Rust core exists: the real core's HCL evaluation (variables, functions, `module.kafka.address` in an app's
env), its proxy and names (clients through `kafka.<stack>.doze:9092` and the console at `http://kafka.<stack>.doze`), sleep on idle
and wake on a client's connection, the whole Larder stack (apps, postgres, aws), and the app drawing the console. The harness grants
addresses nobody listens on, so no external client follows the advertised address in these tests (interop does, against the broker
alone).

## Files

```
crates/doze-kafka/src/module/mod.rs        the face: when it runs, the block, listeners, connection, resolve, provision, plan
crates/doze-kafka/src/module/check.rs      what is refused before boot
crates/doze-kafka/src/module/client.rs     the Kafka client the module speaks to its broker with (client/data.rs: offsets, groups, produce, fetch)
crates/doze-kafka/src/module/converge.rs   converge, objects, health
crates/doze-kafka/src/module/admin.rs      resources, produce, tail, the console layout
crates/doze-kafka/src/module/tests.rs      unit tests (14), including the console snapshot
crates/doze-kafka/src/module/console.snapshot.json
crates/doze-kafka/tests/module_oracle.rs   the oracle (2 tests); module_boot.rs: the boot-sequence tests (5); module_support/
crates/doze-kafka/src/main.rs              the hook: module or broker
crates/kafka-broker/src/topic_config.rs    `check_topic_configs`, `topic_config_names` (public, additive)
tools/module-oracle/kafka/                 the Go oracle, corpus, extra cases, Go's answers
```

## Cutover readiness

Facts, 2026-11-14:

- **Gates** on this branch: `cargo xtask gates` (fmt, clippy, codegen, lint-size, fuzz-check, test), clippy lean and `--features tls`,
  the module oracle and boot tests, `interop/run.sh`, `cargo xtask weight doze-kafka`: all exit 0 (exit codes in the branch's report).
- **Oracle:** 43 cases against the Go kafka module; 22 the same broker, 3 refused by both, 18 differing only by listed improvements.
- **Interop:** `interop/README.md` records majors 1 to 4 (plain and SASL) green on 2026-11-14 with sarama, franz-go, kafka-go and
  librdkafka; `interop/run.sh` green again on this branch with the module face in the binary.
- **Size:** 6,574,304 B on darwin arm64 with the module face (+562,736 B, +9.4%), 171 packages, 7.1 MB idle; Linux limit re-based to
  8.4 MB (docs/port/weight.md), under the owner's 10 MB ceiling.
- **Known gaps:** no schema registry until `p10-registry` merges (the module then plans it by itself, if the flag is
  `--schema-registry-addr`); the console's link to the registry (Go's `--registry-url`) waits for that branch's flag; the tail shows
  time in UTC (Go showed local time); `produce` writes to partition 0 (Go's client spread unkeyed records); the Windows build of the
  module is declared but its acceptance has not run on Windows; nothing of the Rust core exists, so the module is proven against the
  harness only.

Decisions for the owner (not made here):

1. **Cutover**: whether the Rust `doze-kafka` with this face replaces the Go kafka module as what doze's registry offers for `kafka`
   (protocol 2, so the Go doze never picks it), now or once the Rust core runs Larder end to end.
2. **Weight**: whether the module face stays in the default build (+0.56 MB for every standalone user) or moves behind a feature only
   the `kafka-plugin` release turns on (the gates' test run would then need that feature).
3. **Behaviour changes** in the better-than-Go list that a Go user could notice: refusing what Go quietly defaulted (items 1 to 3, 7)
   and converging existing topics' partitions and configs (item 5).

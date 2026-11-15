# Data control, group c (2026-11-15)

`aws_wire::storage::Storage` for every remaining service that keeps files: Step Functions, CloudWatch, EventBridge, and the definition
services IAM, KMS, SSM, Secrets Manager, SNS, EventBridge Scheduler, CloudFormation and API Gateway. STS keeps nothing of its own (its
state is IAM's) and answers `None`. Every service follows SQS's shape (`crates/aws-sqs/src/storage.rs`): a `Data` variant on the
actor's request carrying a oneshot sender, `Core::data` answering inside the actor, `Keyed::keys_on_disk()` for the regions (IAM is
global: one file, reported as region `global`, and only when it exists), deletes flushed to the writer and then `Db::compact`. Nothing
runs unless asked; a usage wakes the regions that have files.

Every crate has `src/storage.rs` and a test `src/tests/data.rs` that works on a real temporary directory, checks `freed > 0` after an
empty or reset, checks that a dry run counts and removes nothing, and checks that what is left (or nothing, after a reset) is what a
restart loads. Wire behaviour is unchanged: no AWS operation was touched, only actor requests added (SSM, KMS and SNS had `Call` as
their actor request; it is now an `Ask` enum with `Call(Box<Call>)` and `Data`).

## Services that hold data

| service | a resource is | its bytes count | units | prunes | notes |
|---|---|---|---|---|---|
| Step Functions | a state machine (by name) | its executions' rows, their Map Runs, and their history-event rows (key + value) | executions | `before` (ended before) | ended executions dominate (prune: default); executions of a deleted machine (listed under its name); running executions |
| CloudWatch | a metric namespace | its data-point rows (one per metric, minute or second, unit; key + value) and its metrics' rows | data points | `before` (data-point time) | none |
| CloudWatch | `(alarm history)`, a resource of its own | the alarm history rows (each entry's share) | data points (history entries) | `before` (entry time) | none |
| EventBridge | an archive | its archived events' rows (key + value) | events | `before` (event time) | archive with no retention: kept forever (prune: default) |

- **Step Functions.** Emptying removes ended executions (Succeeded, Failed, TimedOut, Aborted, PendingRedrive), whole or those that
  ended before the time (`stop_ms`, or start when there is none), with their Map Runs and history rows. **A running execution is never
  removed**: it is left out of the counts (the dry run counts only what would go), and usage says so in a note ("in N running
  executions: emptying never removes them (stop them first)"). `Emptied` has no field for skipped units, so the note in usage is where a
  person sees it. Executions of a deleted machine stay (AWS keeps them 90 days; nothing here ages them out): they are listed under the
  machine's name with a note, and `empty <name>` finds them. The machine, its versions, aliases and tags stay. A reset removes machines,
  activities, executions, Map Runs and history, forgets the unclaimed Express answers, and a name is free at once.
- **CloudWatch.** Emptying a namespace removes its data points, whole or those whose bucket started before the time; a metric left with
  no data goes with them (on AWS a metric with no data stops being listed), one with newer data stays. Alarms, dashboards, anomaly
  detectors, metric streams, insight rules and mute rules are definitions: emptying never touches them. **Decision: alarm history is a
  pseudo-resource named `(alarm history)`**, not a note on a namespace: history belongs to alarms, a composite alarm has no namespace,
  and a person may want history gone while keeping data. No namespace can be called that (namespaces cannot hold parentheses). It is
  bounded (100 entries an alarm) and listed only when it holds something; emptying it keeps the alarms. A reset removes everything
  (series, samples, alarms, history, documents, the remembered account) and tells the bus again that no namespace is watched.
- **EventBridge.** Emptying an archive removes its events, whole or those before the time, and keeps the archive with its pattern and
  retention; its `EventCount` and `SizeBytes` drop by exactly what went (`SizeBytes` counts the events' text, as before). Buses, rules,
  targets, connections, API destinations and replays are definitions: kept by empty, removed by a reset. The default bus is made on a
  region's first call (as before), so after a reset it comes back as on a fresh data directory.

## Definition services

| service | resources listed | empty | reset also clears |
|---|---|---|---|
| IAM | none (files only, region `global`) | Unsupported: "IAM keeps definitions only (users, roles, policies, keys), nothing that grows: delete one through IAM, or reset IAM" | the account record is made afresh (a new session-signing key, so a session token from before no longer opens anything); AWS-managed policies filled as on open; the in-memory access log |
| KMS | none | Unsupported (keys and aliases only) | `alias/aws/*` managed keys (made again when SSM or Secrets Manager next asks); a new file sealing key |
| SSM | parameters with old versions: bytes of the row, items = versions | `--old-versions` removes old versions no label points at (labelled versions are names someone resolves, so they stay); anything else Unsupported with the sentence | expiry deadline |
| Secrets Manager | secrets with more than one version, or scheduled for deletion | `--old-versions` removes versions with no staging label; `AWSCURRENT`, `AWSPREVIOUS`, `AWSPENDING` and custom labels stay | rotation settings, purge deadline; a name held by a scheduled deletion is free at once |
| SNS | none: **SNS keeps no messages** (a publish is delivered as it is made; FIFO dedup ids are a 5-minute window inside the topic row) | Unsupported | SMS settings, platform applications and endpoints |
| Scheduler | none | Unsupported (what a schedule fires is not kept) | the `default` group (made again on the next call) |
| CloudFormation | each stack name: its stacks' rows (templates as given and processed), events and change sets; items = events | removes the name's **deleted** stacks (all, or deleted before the time) with their events and change sets; a live stack is never touched | outbox and pending jobs. It does not delete what stacks provisioned in other services (that is theirs to reset) |
| API Gateway | none; v1 answers for both APIs (one directory), v2's `storage()` is `None` | Unsupported | the authorizers' cached answers |

Notes on the calls:

- **SSM and Secrets Manager**, `--old-versions` is the only prune. Version numbers are unchanged after it (the next `PutParameter` is
  still `latest + 1`), `GetParameterHistory` / `ListSecretVersionIds` list fewer. It departs from nothing AWS-visible a program relies
  on: AWS itself drops the oldest unlabelled versions past 100.
- **CloudFormation**: AWS lists a `DELETE_COMPLETE` stack for 90 days; doze kept every one forever (Go weak: bettered by a note and a
  prune). Emptying a name makes its deleted stacks disappear from `ListStacks`, which is the point.
- **KMS** reset alone leaves what other services sealed with its keys unreadable (SSM `SecureString`s, secrets): the module doc says so;
  `doze-aws data reset` of everything is the way to start again.
- Every `reset` re-creates in memory exactly what opening an empty directory would, and the tests reopen the directory to prove disk
  agrees: IAM through `Core::fresh`, KMS by a new sealing key in `meta`, the lazily made defaults (EventBridge's bus, Scheduler's group,
  KMS's managed keys) by leaving them to the next call as a fresh start does.

## Calls on behaviour

- Pruning, measuring and resetting are not AWS operations: **doze's own** (local hook, `doze-aws data`). No AWS behaviour changed;
  conformance exact counts unchanged (no wire code was touched).
- Step Functions keeping executions of a deleted machine forever, CloudFormation keeping deleted stacks forever, EventBridge archives
  with no retention: Go weak (kept forever and invisible); **bettered**: measured, noted, and removable by name.

## Open

- `Emptied` cannot carry "N skipped" (Step Functions' running executions): the skip is said in usage's note, not in the empty's answer.
  If the CLI wants it in the dry run line, the contract would need a field (integrator's call).
- CloudWatch's sample rows are counted by scanning the table (no copy kept in memory); a namespace of millions of rows makes `usage`
  read them all once. Fine for a dev store; a per-series counter would avoid it if it ever matters.
- IAM, KMS, SNS, Scheduler and API Gateway list no resources: their rows are definitions of a few KB. If the console wants to show
  "what is in the file", a per-table count would be the next step.

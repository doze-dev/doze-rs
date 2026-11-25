# The AWS Lab: rehearsing what production does to a client (design)

The owner asked whether the AWS Lab is built well and pointed at the Kafka Lab for inspiration. It is not: today's Lab is a request builder (pick
any operation, edit its input, send it as the console) with a "Not built yet" box where the Lab should be. The Kafka Lab (`docs/port/phase9/lab.md`,
`docs/port/behaviours/lab.md` LAB-1 to LAB-22, `web/src/pages/lab/`) is the model: rehearsals that teach, fault rules with bounds, forced events
that go through the service's own machinery, Live tags, Stop all. This file is the contract for the three parts. The owner said "do all three".

Read the Kafka Lab's code for shape, **do not edit** `web/`, `crates/kafka-*` or the theme CSS. Do not copy Kafka-specific logic; write the AWS one.

## The three parts

1. **The page (console only).** The Lab page becomes tabs: *Rehearse* (the cards, the rules in force, what ended), *Send a call* (today's page,
   kept), and the history of calls sent from it (replayable, kept in this browser), saved inputs, and **recipes** that name the person's own queues,
   tables, buckets and functions and open Send a call with a real input for them. Every rehearsal has a lesson: what it stands for in production,
   what to expect, what a well-behaved client does, the SDK settings with their real defaults, where to see it.
2. **Fault rules (the engine).** A rule makes calls that clients send to the doze-aws port slow, fail the way AWS fails, or drop.
3. **Forced events (rehearsals that are not rules).** The AWS analogues of Kafka's "expire a member / force a rebalance / compact now": events that
   production causes and that wait on a timer here, forced through the service's own code path, never a new transition.

## 2. The engine

### Where it hooks
`aws-gateway`, one hook per protocol path, **after the request is decoded** (so the operation and the resource are known) and **before the service
is asked**: `pipeline.rs` (JSON, after `observe::decode`), `query.rs`, `cbor.rs` and `rest.rs` (after `observe::input`). The four paths already
note the service, caller, region, operation and resource on the request's `Seen` for the observer: the judge reads those notes.

- `Config::faults: Option<Arc<dyn Faults>>` next to `observer`. `None` (the default, and `--lab=false`) costs a request one `Option` test. With a
  table installed and no rule switched on, one relaxed atomic load.
- `trait Faults { fn judge(&self, ask: &Ask) -> Verdict }`. `Ask` borrows: signing name, SDK id, operation, resource, access key id, region, user
  agent. `Verdict`: `Pass`, `Slow(Duration)`, `Fail { status, code, message }`, `Drop`, each with the rule's number and name (for the tag).
- **Fail** is answered by the path's own error renderer (`fail` in each path), so it has the protocol's shape (JSON, Query XML, CBOR, REST XML/JSON)
  and carries the code in every place an SDK reads it (including `x-amzn-query-error` for SQS's JSON-with-query-compat). The service is never asked:
  **a fault never reaches storage** (nothing is written by a failed call). **Slow** waits, then the call proceeds (a stop of the server does not
  wait). **Drop** closes the connection with no answer (`respond` marks the response; `connection()` turns it into an error from the service fn).
- Not judged: `/_doze/health`, `/_doze/...` local hooks, function URLs, API Gateway stages (not AWS API calls: they are not told to the observer
  either), the console's own calls and calls between services (they do not go through the gateway: **the console is never bent**, and a Lab "Send a
  call" is not bent either, say so on the page).
- The observer is told which rule touched a request: `Finished` gains the rule's number, name and effect; the Recorder keeps them on the row
  (never folded), Live draws a tag, filters by `lab=<rule id>` and by Lab-only, and says "includes +N ms" under a slowed row's time.

### What a rule is (bounds are enforced, typed refusals, tests per bound)
- **Name** (≤80 chars, no control characters). **Matcher**: services (signing name or SDK id; ≤16), operations (exact; ≤16), regions (≤8), callers
  (access key ids; ≤8), resource patterns (glob `*` `?`; ≤16; against the decoded resource: queue, table, bucket/key, function, secret...). An empty
  field fits anything; a resource rule does not fit a request that names no resource. Linear-time glob (one backtrack point), tested hostile.
- **Effect**: `Slow{delay_ms 1..60000, jitter_ms 0..60000}`, `Fail{kind}` with kinds `Throttle`, `Unavailable`, `Internal`, `AccessDenied`,
  `Custom{status 400..599, code ≤64 chars [A-Za-z0-9.]}`, `Drop`.
  - Error codes come from **one table per service**, verified against the vendored models (`.audit-models`) and the SDKs' retry classification
    (botocore's throttling and transient error lists, the Java/JS v3 lists), not from memory. Examples to verify: Throttle is `ThrottlingException`
    (400) by default, `ProvisionedThroughputExceededException` for DynamoDB and Kinesis, `SlowDown` (503) for S3, `RequestThrottled`-class for SQS if
    its model says so; Unavailable is `ServiceUnavailable` 503; Internal is `InternalFailure` 500 (`InternalServerError` for DynamoDB, `InternalError`
    for S3). A rule is refused for a service whose protocol path cannot word the error (none expected).
- **Share**: every request, every Nth (2..1000), or a random 1..99 % from a seeded generator (same seed, same requests).
- **Lifetime**: 1 s to 24 h, and optionally a number of hits; ends by itself; a rule switched off does nothing and still runs out. Expiry is lazy (no
  timer): the table sweeps on every look and dates an ended rule when it was due.
- **Bounds**: 50 rules at once, 200 ended kept, table in memory (a restart clears it; no API, file or flag makes a rule except the console).
- **Two rules on the same requests: one wins**: same matcher and effect kind replaces; when several fit, drop > fail > slow, the longer delay wins,
  only the winner counts a hit.
- **A rule remembers little**: matched, hits, first request it fired on, last, ≤8 caller keys (rest counted).
- `--lab`: on by default; off with `--console` off; off by default when the console is served beyond loopback unless `--lab=true` (then start-up and
  the Access page say anyone who can reach the console can inject faults). Read how doze-aws's options and the Kafka LAB-21 do it.
  *Send a call* and the history do not depend on the flag (they are as dangerous as the console already is).

### Console API (`aws-console`, behind the same-origin proof like every write; shapes are `ts-rs` models so `cargo test -p aws-console api::types` writes the TS)
- `GET  /api/v1/lab/rules` → `{ enabled, off_why, in_force: [LabRule], ended: [LabRule], once: [LabOnce], limits }`
- `POST /api/v1/lab/rules/check` and `POST /api/v1/lab/rules` (same body: kind, name, matcher, effect, share, lifetime, max hits) → check returns the
  sentence ("Throttle 1 in 4 SQS SendMessage calls to orders for 10 minutes") and field problems in words; create returns the rule or the refusal.
- `GET  /api/v1/lab/rules/{id}` (the rule and the recent requests it touched, from the Recorder), `POST .../{id}/switch`, `POST .../{id}/remove`,
  `POST /api/v1/lab/stop-all`, `POST /api/v1/lab/history/clear`.
- `GET  /api/v1/lab/guide` → `{ intro, rehearsals: [LabTeach], recipes: [LabRecipe] }`: the words (one module, like `kafka-console/src/lab/teach.rs`)
  and recipes built from the person's own resources (read through the bus; recipes name only what exists, and are empty-state-safe).
- Forced events: see below. All words in sentences made once from the rule; never a live number inside a sentence.

## 3. Forced events

Each is a real step: it calls the function the service's own timer or request path calls, behind the service's own check, and is refused **in words**
when there is nothing to do. Survey first (read the services; see `Lazy`, `Keyed`, `Deadlines`, `Resumable` in `crates/doze-runtime`), then build
what is honest, in this order of value, and **write down in `lab.md` what you did not build and why**:

1. **Rest a service now**: close the lazy actor (the real idle close), so the next call opens it again; shows what wake-on-call costs. Per region.
2. **Fire a schedule now** (EventBridge Scheduler): the target is called by the same path the timer uses.
3. **Let an in-flight SQS message time out now**: the visibility timeout's own expiry step for one queue; the message is receivable again with the
   receive count it had.
4. **Run DynamoDB's expiry (TTL) sweep now** for a table.
5. **Make the next Lambda call cold** (drop the warm worker) if Lambda keeps one.
6. Others the survey finds that are as honest (Secrets rotation due, EventBridge archive retention, Logs retention sweep); skip any that would need a
   new state transition.
How the console reaches a service's private step is a design choice for the builder: least invasive wins (an optional method with a default on the
service trait, or the `/_doze/<service>/...` local hooks, or the peer bus). No console-only fake. The console checks a right first like Kafka's LAB-18
(the console is root). Each leaves a line in the `once` history ("Fired nightly-report now: it sent to email-outbox"), with a way to see the result.

### Forced events: survey

Read 2026-11-26 in `doze-runtime` (`Lazy`, `Keyed`, `Deadlines`, `Tasks`) and the services. The question for each candidate: what is the
function the service's own timer or request path calls, and can the console make it call that now without inventing a transition.

| Candidate | The service's own path | Verdict |
|---|---|---|
| Rest a service | `Lazy`'s actor loop (`run`): `timeout(idle, inbox.recv())` ends, the inbox closes, what is queued is answered, `Service::close` runs, the slot is released. Nothing outside the actor can end it early. | **Built.** `Lazy` gets a rest token (a child of the task set's stop token, so shutdown is unchanged); cancelling it ends the same loop. Every `Lazy` records itself, weakly, in its `Tasks`' `Rests` (name and region key). |
| Fire a schedule now | Scheduler: the region's `Deadlines` task sends `Request::Tick`, `tick()` -> `fire_one()` hands out an `Invocation`, `spawn_all` runs `deliver::attempt` and `retry::carry_on`. The local hook (`Request::Fire` -> `fire_now`) hands out the same `Invocation`, for any schedule, enabled or not. | **Built** as `Request::Force` -> `fire_checked` (refuses what the timer would not fire) -> `fire_now`, then the same `attempt` and `carry_on`. |
| Let an in-flight SQS message time out | **There is no step.** A message is in flight while `visible_at_ms > now`; every receive decides it by the clock, and the retention tick only reclaims space. The request path that ends the wait early is `ChangeMessageVisibility` with a timeout of 0, i.e. `Queue::change_visibility(handle, 0, now)`. | **Built** as exactly that function, for each in-flight message of the queue with the latest handle it was given. This is what a consumer does, not something a timer does, and the doc says so on the card. |
| DynamoDB TTL sweep | `Request::Tick` -> `housekeeping()` -> `sweep_expired()` (also run at the start of every call, at most once a second). | **Built** as `Request::Sweep`: counts what is expired in the table (the same index scan), then runs `housekeeping()` with the once-a-second limit lifted, as `Tick` does. doze deletes an item when it is due, so it seldom finds anything; see "Not built". |
| Make the next Lambda call cold | `Runtime::retire(region/name)`, called after `UpdateFunctionCode` and `UpdateFunctionConfiguration`: marks every warm process retired, kills the idle ones, the busy ones when they finish. | **Built** as `retire`, behind a check that the function exists, with `Runtime::alive` to say how many it ended. |
| Secrets rotation due | `Core::tick` -> `begin(name)` when the window opens; the request path is `RotateSecret` with no arguments, a real API call. | Not built: it is already an AWS call and the Resources page offers it. |
| Secrets recovery window, KMS pending deletion | `Core::sweep` removes what is past `purge_ms`, lazily on every call. | Not built: "the window has run out" would be a new value written to `purge_ms`, not a step. The real calls are `DeleteSecret` with `ForceDeleteWithoutRecovery` and (for KMS) none. |
| EventBridge archive retention | `archives.rs` removes events older than the retention, lazily, at most once an hour. | Not built: lifting a one-hour limit shows a client nothing it can act on. |
| Logs retention sweep | `Core::sweep(group)` runs on every write and read of the group, at most once a minute. | Not built, same reason. |
| SQS message retention | `Ask::Tick`; every call also counts expired messages out against the clock. | Not built: no client can see the difference. |

### Forced events: as built

- **How the console reaches a service's private step.** `aws_wire::forced::Forced` (`crates/aws-wire/src/forced.rs`): `force(caller, kind, input)
  -> Step::Done{sentence, subject} | Step::Nothing(why)`, reached through `Service::forced()`, a method with the default `None`, on the services the
  console already holds from the bus (`Peers::targets()`). No route on the gateway, no `/_doze/` hook, no network: only code in the process can ask.
  A service that implements it: SQS, Scheduler, DynamoDB, Lambda (`crates/aws-<service>/src/forced.rs`, each with its tests in `src/tests/forced.rs`).
  Each asks the authorizer first, as the matching call does: `sqs:ChangeMessageVisibility` on the queue, `scheduler:UpdateSchedule` on the schedule,
  `dynamodb:DeleteItem` on the table, `lambda:UpdateFunctionConfiguration` on the function. The console calls as itself, the account's root, which IAM
  lets do what the person running doze-aws can; a caller without the right is refused in the words its own call is refused in, and nothing is changed.
- **Resting** is not a service's own step: it is the host's register of lazy services. `doze_runtime::Rests` (one per `Tasks`, `crates/doze-runtime/src/rests.rs`)
  lists every `Lazy` by name and region key and rests one: `Lazy::keyed` registers a weak handle, `Rests::rest(name, key)` cancels that actor's rest token and
  waits until the actor has ended. `doze-aws` hands the register to the bus (`Peers::set_rests(tasks.rests())`, one line in `wire.rs`), which is how the console
  reads it. Per region: a region is a key. IAM is one key-less actor and is rested in every region at once (it is the same service).
- **The console** (`crates/aws-console/src/lab_once/`): `kinds` (five kinds and whether this run can carry each out), `form` (what there is to choose, read through the
  bus with the Resources space's own descriptors and calls), `act` (carrying out, as the console), `rest`, `history` (the last 200, in `State::once`), `teach`
  (`GET /api/v1/lab/once/guide`); `src/api/lab_once.rs` has the five handlers, `src/model/lab_once.rs` the models (ts-rs types in `console/apps/aws/src/api/types/LabOnce*.ts`).
  The page's other agent reads the history for `GET /api/v1/lab/rules` through `aws_console::lab_once::recent(&state)`.
- **A resting service is described as resting.** A form for a queue, schedule, table or function asks the register whether the service runs in the region; if not it
  answers `resting: true` with no choices and does not call it (a list is a call, and a call starts it). `?look=yes` reads it anyway.
- **Home** counts a service as running only if a call reached it after the last time the Lab rested it in that region (`History::rested_after`, one filter in `api::home::awake`).
- **Decisions made where the brief was silent.** The kinds are named `rest-service`, `fire-schedule`, `expire-visibility`, `sweep-ttl`, `cold-start`. A refusal in words is a
  200 with `ok: false` and is kept in the history like a success (a request that is not one, such as no choice or an unknown kind, is 400/404 and is not kept). Forms take
  `?region=`, else `X-Console-Region`, else `--region`; a POST takes `region` in its body the same way. "Rest" can time the wake: the first list call of the service after the rest and the same
  call again, both on Live as the console's. A schedule fired now invokes the target once and keeps its plan (the local hook's meaning), a one-time schedule's fire is its one fire.
  A schedule's first attempt is waited for at most ten seconds; after that the invocation carries on in the background and the sentence says so.

- **Weight** (`cargo xtask weight doze-aws`, macOS, release-aws, 2026-11-26): 12,087,968 B (the base commit `ce3fe9566`, measured the same way: 12,004,640 B, so +83,328 B), 305
  packages (unchanged), idle 7,760 KB (samples 7,664 to 7,792; the base 7,792 to 7,872). The register holds one weak handle per lazy actor made, which is a few bytes for each region a service was called in.

### Forced events: not built

- **Secrets rotation, Secrets and KMS deletion windows, EventBridge archive retention, Logs retention, SQS retention.** See the survey: each is lazy against the clock or is already a
  real API call; forcing it would write a new date or lift a limit no client can see. Not built on purpose.
- **A held-back TTL deletion.** DynamoDB's real behaviour that a client has to survive is deletion that comes up to two days late, with expired items still returned by reads.
  The sweep here cannot rehearse that: it would need a switch that delays the deletion, which is a new state, not a forced step. What was built is the sweep itself, which in doze
  finds something only when items have expired and not yet been swept (the timer sleeps to the first expiry, and the sweep is also run by every call), so it usually answers "nothing
  has expired" with when the next item does. The card says so in its lesson; the owner may prefer to drop it.
- **Which message, which handle.** The in-flight form brings back every in-flight message of a queue, not one chosen message: the Lab cannot see receipt handles and the console
  does not list messages in flight.
- **A forced event on a service left out of `--services`.** Refused in words (`available: false`, why): the service is on the bus but is not what the run serves.
- **Lambda's warm processes are not listed on the form** (the form lists functions with their runtime); the sentence after the event says how many ended.
- **The page.** Another agent builds `console/apps/aws/src/pages/lab`; these endpoints and types are its contract.

## 1. The page

Under `console/apps/aws/src/pages/lab/`, files under 300 lines. Use `@doze/ui` pieces and the app's `kit/`; Graphite look as the rest of the console;
the Kafka Lab page is the layout inspiration (cards, active table with switches and hit counts and "See it on Live", ended list, once list, Stop all,
recipes, "In production" text). The rule form is a dialog with a live sentence and the field problems from `/rules/check`. Everything has a
gallery-free e2e in `console/apps/aws/e2e/tests/lab*.spec.ts` against a real doze-aws and the real AWS CLI: a throttle rule makes `aws sqs send-message`
fail with the SDK's retry behaviour visible in Live; Stop all makes it work; the tag shows in Live.

## Safety and weight
The console is the only way to make a rule. Faults live in memory. The binary stays under 20 MB and 16 MB idle (`cargo xtask weight doze-aws`); the
Lab chunk is lazy (it already is). Fuzz: a new parser needs a `// fuzz:parser` marker and a target (the glob matcher and the rule body are such).

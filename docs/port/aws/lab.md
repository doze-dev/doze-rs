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
- `GET  /api/v1/lab/rules` → `{ enabled, off_why, now_ms, in_force: [LabRule], ended: [LabRule], limits }` (no `once`: the forced events are another part; section 4)
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

## 1. The page

Under `console/apps/aws/src/pages/lab/`, files under 300 lines. Use `@doze/ui` pieces and the app's `kit/`; Graphite look as the rest of the console;
the Kafka Lab page is the layout inspiration (cards, active table with switches and hit counts and "See it on Live", ended list, once list, Stop all,
recipes, "In production" text). The rule form is a dialog with a live sentence and the field problems from `/rules/check`. Everything has a
gallery-free e2e in `console/apps/aws/e2e/tests/lab*.spec.ts` against a real doze-aws and the real AWS CLI: a throttle rule makes `aws sqs send-message`
fail with the SDK's retry behaviour visible in Live; Stop all makes it work; the tag shows in Live.

## Safety and weight
The console is the only way to make a rule. Faults live in memory. The binary stays under 20 MB and 16 MB idle (`cargo xtask weight doze-aws`); the
Lab chunk is lazy (it already is). Fuzz: a new parser needs a `// fuzz:parser` marker and a target (the glob matcher and the rule body are such).

## 4. As built: the engine (branch `aws-lab-engine`)

Part 2 of the contract above, built; the rules and the tests that hold them are `docs/port/behaviours/aws-lab.md` (AWS-LAB-1 to 20). Not built
here: the page (part 1, another agent), the recipes (made by the page), the forced events (part 3, another agent's `lab_once`).

**The files.** `crates/aws-gateway/src/fault.rs` and `fault/apply.rs` (the trait `Faults { armed, judge }`, `Ask`, `Verdict`, `Done`, the hook
`apply`, the dropped-connection mark); the four paths call `fault::apply` after `observe::input`/`decode` (`pipeline.rs`, `query.rs`,
`cbor.rs`, `rest.rs`) and `server.rs` turns the mark into an error from the service function. `crates/aws-console/src/lab/` is the table:
`rule.rs` (every bound, `AddError`), `glob.rs`, `codes.rs` (the error table with its sources), `table.rs` (`LabTable`: bounds, lazy expiry,
precedence, seeded shares), `record.rs`, `words.rs` (the sentences), `teach.rs` and `teach/settings.rs` (the lessons). The API is
`api/lab_rules.rs` and `api/lab_build.rs`; the models are `model/lab_rules.rs`. `doze-aws/src/lab.rs` is the `--lab` decision and
`console.rs`/`run.rs` install the table. Live: `pages/live/lab-tag.tsx` and the filters. Fuzz: `crates/aws-console/fuzz` (`lab_glob`, `lab_body`).

**What the contract left open, and what was decided.**
- *Where Slow waits.* Before the service is asked (the doc said "then the call proceeds"), not after the service has made its answer as the Kafka
  Lab does: nothing the service does happens while the call waits, and a client that gives up while it waits is never served. Live's time includes
  the wait. The lesson says what that is not (in production the service may have done the work).
- *Where the hook sits.* After the input is decoded and noted, before the service, in all four paths. In `rest.rs` that is after the call is
  decoded and before it is handed over, so an S3 upload's payload has not been read (a failed `PutObject` is answered before its body, as
  AWS answers a refusal sent with `Expect: 100-continue`). A call the model refuses is not judged (AWS-LAB-5).
- *The judge costs.* The trait has `armed()`, one relaxed load, so a table with no rule switched on does not even have the call described to it;
  `Seen` is made when an observer **or** a table wants it, so the rules judge a call with no console observer too.
- *Drop's status.* A dropped call is told to the observer with status 0, and `Outcome::of(0, _)` is `error`. Live draws `NO ANSWER` and does not
  count it as a refusal (`isRefused`, `lookOf` and the map's counts now need a status above 0). `AuthorizationError` (SNS's own denial) is `denied`.
- *The rule's name on a Live row.* The ring keeps the rule's number and what was done (8 bytes a row: its worst-case total is still under a
  mebibyte, the bound its test holds). The name is read from the table when the row is sent, `Lab rule N` when the table has forgotten the rule
  (200 ended rules are kept). `Finished::lab` still carries the name.
- *The body of a rule* (`LabRuleBody`): `matcher {services, operations, regions, callers, resources}`, `effect {kind: slow|fail|drop, ...}`
  with `fail.error` one of `throttle|unavailable|internal|access-denied|custom`, `share {kind: every|nth|percent}`, `ends_in_secs`,
  `max_hits`. There is no `once` member and no seed in the body (the table is seeded from the clock; tests seed it).
- *Create's answer* is always 200 with `{outcome: "made", rule}` or `{outcome: "refused", check}` (the same `LabCheck` the form gets), so the
  page types both. `POST .../{id}/switch` takes `{enabled}` or nothing (it then turns the rule the other way): `LabSwitchBody`, a type added after the
  first commit of the models. `lab=<rule>|any` is a query of `GET /api/v1/calls`.
- *Services in a rule.* A word is a catalogue name (`stepfunctions`), a signing name (`states`) or an SDK id (`SFN`, `DynamoDB Streams`), any
  case; the first is read into the signing names of its APIs when the rule is made, so a typo is a problem in words and not a rule that never
  fires. Operations are checked against the models of the services named; resource patterns fit the whole text or the last name.
- *Replacement* is by matcher and kind of effect (a throttle replaces an unavailable), as the Kafka Lab does; the replaced rule is history.
- *Errors.* One table per API (`lab/codes.rs`), a modelled shape where the vendored model carries one, the Common Errors elsewhere. Messages say
  `(made by doze-aws Lab rule N)` so a log shows the fault was made.
- *`--lab`.* A text setting `lab` (`on` by default; bare `--lab` is on; `--lab=false` off; `DOZE_AWS_LAB`; `lab = "off"`), decided by
  `doze-aws/src/lab.rs` against where the console listens. Module mode (doze runs doze-aws) has the Lab on: the console is on loopback there too.
- *The Live tag.* A compact flask and the rule's number (hover or focus says what the rule did), the Lab-only chip that appears once a Lab call is
  held (`?lab=any`, or `?lab=3` from an address), the latency cell `403 ms +400`, `NO ANSWER`, and the call's drawer says it in a sentence. Rows
  a rule touched are never folded into a repeat (the retries are the point). The Result column grew (214 to 320, latency 84 to 120).

**Measured** (2026-11-26, darwin/arm64). `cargo run --release -p aws-console --example lab_bench`: a request through the gateway to SQS costs
about 24.5 to 25.3 microseconds with the recorder only, with the table and no rule, with one rule that fits nothing and with 50, all within
about 1 microsecond of each other; `armed` is 0.2 ns, `judge` 2 ns with no rule, 25 ns with one that fits nothing, 123 ns with fifty.
`cargo xtask weight doze-aws` (with the Live work and the rebuilt bundle): 12,088,352 bytes against 20,000,000, 305 packages against 310,
7,872 KB idle against 16,384 (darwin/arm64, `release-aws`).

**SDK defaults the lessons name** (read on 2026-11-25 from the official docs and sources; the sources are in `lab/teach/settings.rs`): botocore
standard mode by default with 3 attempts (4 for DynamoDB), legacy 5; connect and read timeouts 60 s; JavaScript v3 3 attempts, standard, a request
timeout of 0 (none); Java 2.x standard from 2.58.0 with 3 attempts (4 for DynamoDB), legacy 4 (9) before it; Go v2 `retry.Standard` with 3 attempts
and a 20 s backoff cap, timeouts from the call's context. The retry classification lists are botocore's, read from its source.

## 5. Not built

- **The page, the recipes and the forced events** belong to other parts of this work (section 1 and 3 above); nothing here touches
  `console/apps/aws/src/pages/lab` or `GET /api/v1/lab/once*`.
- **A recording of real AWS.** The codes are from the vendored models and the API References' Common Errors pages (IAM, STS, Step Functions,
  CloudFormation, CloudWatch, SNS and SQS read on 2026-11-25), not captured from AWS (no account yet). Where a service's own page was not read
  (SSM, Secrets Manager, KMS, Kinesis, Logs, EventBridge, Scheduler, Lambda, DynamoDB, API Gateway) the model's shape is used if it has one and the
  common error otherwise. Not carried: Lambda's `Reason`/`Retry-After`, DynamoDB's per-table throttling reasons, S3's `x-amz-request-id`
  variants beyond what the renderer already makes.
- **Numbers that could not be confirmed are not given:** Java's connection and socket timeouts (the SDK docs state none), the JavaScript
  connection timeout (none unless set, from the handler's source, but no documented number), Go's dial timeouts.
- **Faults inside an answer.** A rule bends a call before the service is asked: no half-sent body, no event stream cut in the middle, no
  slow download, no answer that is right but late in pieces. A drop is before the answer, never after the service has written.
- **"Drop once" per connection** (Kafka LAB-13): AWS clients pool connections, but a rule that closes "the connections open now" has no
  meaning for a call-per-request protocol; use a rule with `max_hits`.
- **Access page text.** There is none for the Lab and the console is loopback-only (`--console` refuses another address), so nothing says
  "anyone who can reach the console can inject faults" in the console; `LabFacts.open` carries it for the day the console is served wider, and
  the start-up line says it today if `--lab=true` is given with such a console (tested as a function, not reachable through the binary).
- **Browsers.** The Live spec ran in Chromium only (one browser at a time on a shared machine; Firefox and WebKit are in the config); the dark
  theme was not photographed (the tag uses the theme's `primary` and `current` colours).
- **A banner or a count of "rules in force" on other pages** (Live's header, the sidebar): Live shows the tag and the chip only once a Lab call is
  held; a rule that has not fired is only on the Lab page.

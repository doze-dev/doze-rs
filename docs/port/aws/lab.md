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

## 1. The page

Under `console/apps/aws/src/pages/lab/`, files under 300 lines. Use `@doze/ui` pieces and the app's `kit/`; Graphite look as the rest of the console;
the Kafka Lab page is the layout inspiration (cards, active table with switches and hit counts and "See it on Live", ended list, once list, Stop all,
recipes, "In production" text). The rule form is a dialog with a live sentence and the field problems from `/rules/check`. Everything has a
gallery-free e2e in `console/apps/aws/e2e/tests/lab*.spec.ts` against a real doze-aws and the real AWS CLI: a throttle rule makes `aws sqs send-message`
fail with the SDK's retry behaviour visible in Live; Stop all makes it work; the tag shows in Live.

## Safety and weight
The console is the only way to make a rule. Faults live in memory. The binary stays under 20 MB and 16 MB idle (`cargo xtask weight doze-aws`); the
Lab chunk is lazy (it already is). Fuzz: a new parser needs a `// fuzz:parser` marker and a target (the glob matcher and the rule body are such).

## Not built (page, part 1)
- **S3 PutObject from Send a call.** The Lab sends over the peer bus, which carries no object body (`aws-s3/src/put.rs`: "the object body was not
  received"), so PutObject and UploadPart are answered with an error. The page says so under the editor; the bucket's recipe is *Look inside*
  (ListObjectsV2) and its next step is the bucket's page, where upload exists. Sending a body needs the bus to carry one.
- **Recipe for a Lambda function needs no code to be present, but a call to one that has no runtime** is the service's own answer, not the Lab's.
- **Rehearse's rule and forced-event cards** are drawn off ("Needs a newer doze-aws") until the engine and forced-event endpoints are merged
  (`LAB_ENGINE` in `pages/lab/rehearsals.ts`); their words are static until the guide endpoint supplies them.

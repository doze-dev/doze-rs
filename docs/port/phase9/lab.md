# The Lab: fault injection for the console (design, as built)

The owner: "Labs needs to still be done." D14 of `docs/port/phase9.md` had deferred it as a feature of its own (fault rules in the request path).
This is that feature: the engine and the console-side API are built and tested; **the pages that draw its twelve boards come with the React
rebuild of the console** (the Rust server becomes a JSON API over the view-models named below). Rules: `docs/port/behaviours/lab.md` (LAB-1 to
LAB-21) and CON-930 to CON-939. Debt: J491 to J497.

## What the Go edition's Lab was

A rule with a matcher, an effect and a lifetime, with hit counts, made from the console and held in memory; the boards keep that shape: a rule
slows or fails Produce or Fetch for one topic, 1 in N, for a few minutes; and four rehearsals that happen once (drop a connection, expire a
member, force a rebalance, run the cleaner).

## The engine

`kafka-server::fault` is a bounded in-memory table of rules.

- **A rule** is a name, a matcher (client id exact or glob, principal, APIs, topics and partitions, a group, a limit on connection numbers), an
  effect (`Slow{delay, jitter}`, `Fail{code}`, `Drop`), a share (every, every Nth, a random per cent, seeded), a lifetime (1 s to 24 h, and
  optionally a number of hits), an enabled flag, and what it has done (matched, hits, first and last hit, at most 8 client names).
- **The hook** is `FaultPlan::judge(&Ask)`, called in `connection/exchange.rs` before the broker handles a request, modelled on the observer:
  `Config::fault` is `None` with `--lab=false` and a request then costs one `Option` test; with the table installed and no rule switched on it is
  one relaxed atomic load. A fail answer is built by `fail_response(request, version, code)`, one typed function per API (12 APIs, one `match`
  over every request with no wildcard), and the broker is never asked. A delay waits after the broker has answered (the stop of the server cuts
  it short). A drop closes the connection (`End::Faulted`); at ApiVersions too, so "keep dropping" works on new connections. SASL requests are not
  judged at all.
- **Tagging.** `Observed::fault` carries the rule's number, name and what it did to the recorder, which keeps it on the row (`Row::lab`),
  never folds such a row, and says "includes +N ms".
- **Never the console.** Its calls do not go through the connection loop; a connection whose principal is the console's is left alone by any rule
  that does not name that principal. **Never storage:** faults are made of the request and the answer alone.
- **Expiry is lazy.** There is no timer: the table sweeps on every look (a request while a rule is on, any read of the rules), and an ended rule is
  dated when it was due.
- **Bounds:** 50 rules; 200 ended kept; 24 h; delay 60 s; 16 topics; 64 partitions; 8 client names a rule; 200 rehearsals kept by the console.

## The three rehearsals that are not rules

`kafka_broker::ConsoleLab` (feature `inspect`), a separate surface from the read-only `ConsoleView`:

- **Expire a member:** `Event::Force(Expire(member))` runs the function the session timer runs (`session_expired` for a classic group, the
  silent-member removal for the new protocol).
- **Force a rebalance:** `Event::Force(Rebalance)` is `rebalance_after_change` with the cause `Forced` (classic) or `bump_and_reassign` (new
  protocol). A new event, no new transition: the property tests, the differential test and the oracle replay are unchanged.
- **Compact now:** the cleaner's own `clean_partition` on one partition, after the planner's verdict is read.
- Each checks a right first (DELETE on the group, ALTER on the topic).

## The console side

`kafka-console::lab` (words, forms, views, actions, the history of rehearsals) and `gather::lab` (what needs the broker): plain `Debug + Clone +
PartialEq` view-models in `model::lab` (`LabHome`, `LabRule`, `LabRuleView`, `LabForm`, `LabOnceForm`, `LabCompact`) and actions that return typed
results (`LabMade`, `LabOnceDone`, `Result<(), LabCompact>`) in the words of the engine or of Kafka. No serde yet (the foundation agent adds it).

| For the JSON layer | Call |
|---|---|
| the Lab page | `gather::lab::home(&lab, &backend)` |
| one rule, with the rows it touched | `gather::lab::rule(&lab, recorder, id)` |
| the create form (choices, validation, preview) | `gather::lab::rule_form(&backend, recorder, kind, fields, check)` |
| create / remove / switch / forget | `create`, `remove`, `set_enabled`, `clear_history` |
| drop, expire, rebalance | `drop_form` / `drop_connections`, `expire_form` / `expire`, `rebalance_form` / `rebalance` |
| Compact now | `compact_form`, `compact` |

`Live::with_lab(table)` turns it on; `Live::lab` is `None` when `--lab=false`.

## Safety

The console is unauthenticated: rules can be made only through it (loopback by default), by POSTs with the same-origin proof, and nowhere else (no
Kafka API, no file). `--lab=false` installs nothing. The Lab is off with the console; off by default where the console is served beyond loopback in
front of SASL or TLS (that start is refused without `--console-unprotected`; `--lab=true` switches it on); start-up says when it is on beyond
loopback, and the Access page says anyone who can reach the console can inject faults.

## Numbers

`produce_bench` (one connection, 20,000 one-record Produce requests a round, 7 rounds, in memory), median requests per second, quiet machine:
no observer 35,008; Lab on with no rule 35,382 (+1.1 %); Lab on with one rule that fits nothing 35,602 (+1.7 %); the noise between two runs of the
same configuration is +0.4 %. A first run on a busier machine had a 39 % outlier in one median (best of the rounds -2.4 %): the second run is the
fair one.

## What is not built

The twelve boards as pages (React), the tag on the Live tape and the Lab-only filter, the lab banner on Live (J491, J492); fields for principal,
group and partition in the forms; a registry that would close a connection at once instead of at its next request (J493).

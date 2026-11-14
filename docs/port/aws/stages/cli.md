# Stream `cli`: the first minute of the doze-aws binary

Branch `aws-cli` (from `70ce8e3d`). Crate `crates/doze-aws` (the binary) and two small additions to `crates/aws-gateway`. Written
2026-11-14. PLAN, STATUS, scoreboard and the weight line are left to the integrator.

## What was built

The Go edition had a command layer (`cmd/doze-aws`: dispatch table, ready block, env, doctor, apply/export, override prompt, config
file). The Rust binary had flags only. It now has:

| Command | What it does |
|---|---|
| `doze-aws [flags]` | serves (no command is "serve"; a bare flag is a start) |
| `doze-aws version`, `--version`, `-V` | the version and the 17 services this build serves |
| `doze-aws help`, `--help` | commands, every setting grouped (Server, Access, Services), examples |
| `doze-aws env [--shell sh\|fish\|powershell] [--unset]` | the shell lines that point SDKs and the AWS CLI at the running instance |
| `doze-aws config print` | the effective configuration as commented TOML, each value with where it came from |
| `doze-aws apply [TEMPLATE] [--stack S] [--param K=V]...` | CreateStack, or a change set + ExecuteChangeSet, on the running server; events followed live |
| `doze-aws export [--stack S] [--processed]` | GetTemplate of a stack |
| `doze-aws doctor` | one verdict a line (ok / !! / no) and the command that fixes each; exits 1 on a failing check |

Files (all under 300 lines): `main.rs` (dispatch), `cli.rs` (clap: derive for the commands, builder for the settings), `keys.rs` (the
one table of settings), `config.rs` (precedence and checks), `toml.rs` (the TOML subset), `show.rs` (version, config print), `run.rs`
(lifecycle), `wire.rs` (services, as before plus `--services`), `services.rs` (catalogue, `Off`), `ready.rs`, `instance.rs`, `locate.rs`,
`env.rs`, `client.rs` (blocking HTTP/1.1 + SigV4 with aws-auth's primitives), `cfn.rs`, `apply.rs`, `events.rs`, `export.rs`,
`doctor.rs`, `probe.rs` (`lsof`, `df`), `startfail.rs`, `style.rs`. Gateway: `health.rs`, two `Config` fields.

### Dispatch

clap subcommands with an optional command: no command serves, an unknown word is clap's `unrecognized subcommand 'aply'` with
`tip: a similar subcommand exists: 'apply'` and exit 2, and nothing is bound or created (tested on the binary). A mistyped flag gets
clap's suggestion too. **Go weak (bettered):** Go hand-rolled the table after `doze-aws help` booted a server; clap gives the same
safety plus suggestions for flags and commands.

### Ready block (stdout)

```
  doze-aws is up in 8 ms

    endpoint   http://127.0.0.1:4566
    serving    17 services in us-east-1, account 000000000000
    data       ./doze-aws-data
    config     ./doze-aws.toml
    overrides  --data-dir over the file's data-dir = state

    eval "$(doze-aws env)"  point this shell at it
    doze-aws doctor         when something looks wrong
```

Printed once listening; the log line `doze-aws listening on <addr>` on stderr is unchanged (the harness waits for it). With a subset:
`s3 and sqs in us-east-1, account ... (15 more with --services all)`. Colour (bold endpoint, dim labels, cyan commands) only when stdout
is a terminal, `NO_COLOR` is unset or empty and `TERM` is not `dumb`. The log's own ANSI colour now follows the same rule on stderr
(it was always on before, even into a file). The hint follows `$SHELL` (`doze-aws env | source` for fish). **Go weak (bettered):** the
start time, the data directory, the config file and any flag overruling it are shown; Go showed none of them.

### Settings: flag > `DOZE_AWS_*` > `doze-aws.toml` > default

One table (`keys.rs`) makes the flags, the variables, the file keys and `config print`, so they cannot drift. Every server flag is
settable in all three places (20 settings); `--config FILE` / `DOZE_AWS_CONFIG` names the file, else `./doze-aws.toml` is read when
present. Values are checked wherever they came from, and the error says where: `doze-aws.toml, line 2 (region): 'Mars' is not a region
name`, `--services: 'dynamdb' is not a service doze-aws serves (did you mean 'dynamodb'?)`. An unknown file key is refused with the
nearest real one. Paths in the file are relative to the file, on the command line relative to the shell (Go's rule; AWS-irrelevant).
The file format is flat `key = value` (strings, integers, booleans, arrays): a hand-written subset reader (`toml.rs`, ~200 lines with
tests) refuses tables, dotted keys and inline tables by line number rather than half-reading them. Chosen over `toml`/`toml_edit` for
weight (several packages and a few hundred KB for a flat file). `config print` output read back gives the same configuration (tested).

**`--yes` / the override prompt: dropped. Go weak (bettered).** Go stopped at boot to ask `Continue? [y/N]` when a flag overruled
`--data-dir`, `--services`, `--name` or `--region` in the file, and needed three escape hatches (no terminal, EOF, `--yes`) to avoid
hanging under supervisors and `&`. A flag is an explicit instruction, every one of these is undone by restarting without it (nothing
is deleted), and a prompt at boot is the wrong place for a warning. Instead the ready block lists each overruling flag or variable
and what the file said, at the moment it matters, and `config print` marks it (`# from --region; the file says "eu-west-1"`).

New: `--region` (default `us-east-1`): what `env` exports, the region `apply` signs for (its stack lives there), and the region the
gateway gives requests that name none (an unsigned S3 request, a SigV2 presigned URL; both were hard-coded `us-east-1`). AWS sends
both to us-east-1, which stays the default: **AWS-correct by default**, a documented local setting otherwise. Signed requests are
routed by their credential scope as before. `--account` also accepts Go's `--account-id`.

### `--services`

Names, aliases (`events`, `states`, `sfn`, `monitoring`, `secrets`, `cfn`, `apigatewayv2`, `dynamodbstreams`) or `all`. A service left
out is still made (lazily: nothing opens until a call), stays on the peer bus, and is not resumed at start (no wake-up). The gateway
puts it behind `Off`, which keeps the service's model (so a call is understood and answered in its own protocol: JSON, Query XML,
REST XML) and answers every call:

`403 OptInRequired: doze-aws is not serving EventBridge: it was started with --services s3,sqs. To serve it, add eventbridge to
--services (or to 'services' in doze-aws.toml) and start doze-aws again.`

`OptInRequired` is `(memory)`: what AWS says to an account not opted into a service; not recorded, chosen as the nearest AWS answer
to "this endpoint exists but does not serve you"; 403 so SDKs do not retry. Its function URLs and the like (`Web`) are not served.
**Go wrong (fixed):** Go built only the enabled services, so a served one that leans on another (Lambda's logs into CloudWatch Logs,
a template naming a topic) broke; here they keep working through the bus, and only outside calls are refused.

### `instance.json` and finding the running server

Written to the data directory once listening (pid, endpoint, region, account, auth, services, version, start time, data dir; the
verify keys only under `--auth verify`, file mode 0600), atomically (temp file + rename), removed at a clean stop. Liveness is the data
directory's lock, not the pid: a file left by a crash or a reused pid is never mistaken for a live server (`instance::held` takes a
shared lock attempt). Commands find the server by that file, else by `GET /_doze/health` at the configured address (a server started
from another directory or in a container). **Go weak (bettered):** Go asked its names registry or dialled guessed addresses.

### `env`

`export AWS_ENDPOINT_URL`, `AWS_REGION`, `AWS_DEFAULT_REGION`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` (`test`/`test` under trust,
the configured keys under verify), plus `unset` for what is set in this shell and would interfere: any `AWS_ENDPOINT_URL_<SERVICE>`
(beats `AWS_ENDPOINT_URL`), `AWS_IGNORE_CONFIGURED_ENDPOINT_URLS` (makes SDKs ignore both), `AWS_SESSION_TOKEN` (a real session's token
sent with the local keys). sh/bash/zsh, fish (`set -gx`), PowerShell (`$Env:X = '...'`), chosen from `$SHELL` or `--shell`; `--unset`
undoes it. With nothing running it prints the configured address and says so on stderr (so it works in a shell profile), unless the
port is 0. **Go weak (bettered):** Go printed sh only and left the interfering variables in place. No per-service hostnames (those
need `.doze` names, which belong to the doze core, not this binary).

### `apply` and `export`

`apply` calls the RUNNING server's CloudFormation over its wire (awsQuery form posts, SigV4-signed, XML read with aws-wire's reader):
`CreateStack` for a new stack; for an existing one `GetTemplateSummary` (which parameters the new template declares), `CreateChangeSet`
(`UPDATE`; parameters not given and still declared keep their values with `UsePreviousValue`), the planned changes printed (`+ ~ -`,
`(replaced)`), then `ExecuteChangeSet`. A change set that "didn't contain changes" prints `orders is up to date with template.yaml:
nothing to change.`, deletes the change set and exits 0. A stack left in `ROLLBACK_COMPLETE` or `REVIEW_IN_PROGRESS` holds nothing and
cannot be updated (AWS makes you delete it); `apply` deletes it, says so, and creates it again. Capabilities `CAPABILITY_IAM`,
`_NAMED_IAM` and `_AUTO_EXPAND` are acknowledged by `apply` (a local account: the acknowledgement guards nothing; the CloudFormation
service itself still requires them from other callers, as AWS does).

Events are followed by polling `DescribeStackEvents` every 200 ms from the client (the server keeps no timer), printed oldest first as
a table: time since the command started, logical id, type, status (green complete, yellow rollback, red failed), reason. It stops when
the stack is no longer `*_IN_PROGRESS`, prints `✓ orders CREATE_COMPLETE in 0.2s` and the outputs, or `✗ broken ROLLBACK_COMPLETE` with
`cause Bad (AWS::SQS::Queue): <the resource's reason>` (the first failed resource event, not the stack's summary) and exits 1. Default
template `./template.yaml`, `.yml`, `.json`, `cloudformation.yaml`; stack name from the directory, made a valid CloudFormation name.

`--template FILE` (or `template =` in the file) at start applies it the same way once listening, on its own thread (the server's one
thread keeps serving the calls it makes); a failed apply is reported and the server stays up. **Go weak (bettered):** Go applied
`./template.yaml` at boot whenever one was present (surprising in a SAM project) through its own provisioning engine and exited on a
failure; here only a named template is applied, through the same CloudFormation a developer's tools use, with the same events.

`export` prints `GetTemplate` (`Original`; `--processed` for after SAM and other transforms). **Go did differently, deliberately not
followed:** Go synthesised a template from whatever was running. A template rebuilt from live resources loses what only the author
wrote (parameters, conditions, intrinsics, SAM), invents logical ids and cannot tell which resources belong together; the stack's own
template is exact and is what `apply` sent.

### `doctor`

Checks (unprivileged; the one write is a probe file, removed): **running** (found, and answering `/_doze/health`; a holder that does
not answer is a failure with `kill <pid>`), **port** (free; this doze-aws; another doze-aws, with its pid and data dir; or another
program named by `lsof`, with `doze-aws --listen 127.0.0.1:0`), **data** (writable or will be made; held by the running server; held
by another process), **disk** (free space by `df`; warn under 2 GB, fail under 200 MB), **shell** (`AWS_ENDPOINT_URL` unset: SDKs call
real AWS; pointing elsewhere; `AWS_ENDPOINT_URL_<X>`, `AWS_IGNORE_CONFIGURED_ENDPOINT_URLS`, `AWS_SESSION_TOKEN` set; `AWS_REGION`
differing from the instance's), each with `eval "$(doze-aws env)"` as the fix. Clock skew is not checked: client and server share a
clock. **Go wrong (fixed) / out of scope:** Go's doctor was about `.doze` names only; names belong to the doze core and are not here.

### `GET /_doze/health`

On the gateway, before any routing: `{"name":"doze-aws","pid","region","account","auth","services":[...],"version","started",
"data_dir","uptime_secs"}`, `Cache-Control: no-store`. Unsigned, answered from facts handed to the gateway at start (it adds the
uptime); wakes no service. A local hook, not an AWS address (no bucket name starts with `_`). **AWS-correct** (no AWS equivalent; Go
had it).

### Start failures

Printed plainly on stderr (not as a log line), exit 1:

```
doze-aws is already running on ./doze-aws-data (pid 97247, http://127.0.0.1:4599).

  Use it:            eval "$(doze-aws env)"
  Or stop it:        kill 97247
  Or run a second:   doze-aws --data-dir <another directory> --listen 127.0.0.1:0
```

```
doze-aws cannot listen on 127.0.0.1:4599: it is taken by another doze-aws (pid 97247, data /path/doze-aws-data).

  Any free port:     doze-aws --listen 127.0.0.1:0
  Another port:      doze-aws --listen 127.0.0.1:4600
```

The holder of a port is asked `/_doze/health` first, then `lsof` (`postgres (pid 812)`), else "another program".

## Tests

Unit (28): dispatch (no command and a bare flag serve, a typo errors with the suggestion, `-V`/`--version`/`version`, command arguments
and settings from both levels, `debug_assert` of the command tree), config precedence and sources, paths anchored to their source,
refusals naming where a bad value came from, unknown keys, `config print` round trip, the TOML subset, env output in three shells and
the interfering variables, `--services` parsing, the ready block, `instance.json` liveness by the lock, the HTTP reader (whole and
chunked), doctor's shell checks, `lsof`/`df` parsing, event timestamps, stack names. Gateway (2): health answers its facts and uptime;
without facts the path is not the gateway's.

Integration (`crates/doze-aws/tests/first_run.rs`, the real binary): start in an empty directory with a `doze-aws.toml`, read the ready
block, a signed SQS call, `env` finds the endpoint by `instance.json`, `apply` of a template with an SQS queue reaches
`CREATE_COMPLETE` and the queue is listed, a second `apply` has nothing to change, `export` returns the template, `doctor` says running,
a second start on the same data directory fails naming the first, a clean stop removes `instance.json`. And: `doze-aws aply` exits 2
and creates nothing.

## First run, timed

Release (`release-aws`) binary on darwin/arm64, fresh data directory each time, five starts (bun script in the session scratchpad):
the ready block is printed **8 ms** (median) after the process is spawned, and the first SQS call is answered **52 ms** (median) after
spawn (the first call opens SQS's store). The first of the five, cold, took 406 ms. The debug build in the integration test: ~1.3 s.

## Open

- `OptInRequired` for a service not served is unrecorded wording `(memory)`; no boto3 scenario (a server flag, not an SDK behaviour).
- `apply` polls events at 200 ms; a stack that never settles is followed until Ctrl-C (no client timeout).
- `doctor`'s port owner and disk space use `lsof` and `df` (macOS, Linux); on Windows those lines are absent.
- Settings for Lambda interpreters and Step Functions mocks remain their own variables (`DOZE_LAMBDA_PYTHON`, `SFN_MOCK_CONFIG`, ...),
  not in `doze-aws.toml`.

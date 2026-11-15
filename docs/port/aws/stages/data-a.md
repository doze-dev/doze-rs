# Data control, group a: S3 and Lambda (2026-11-15)

Both services implement `aws_wire::storage::Storage` (`Service::storage` returns `Some(self)`), in SQS's shape: an
`Ask::Data(Box<Asked>)` carrying a oneshot sender, answered inside the region's actor; regions from `Keyed::keys_on_disk`.
No AWS-visible behaviour changed. Nothing runs unless asked (usage wakes a region's actor; it idles out as usual).

## S3 (`crates/aws-s3/src/{storage,data,usage}.rs`, test `tests/data.rs`)

**A resource is a bucket.** `units` = objects.

**Its bytes** are what a delete gives back: every version row (key + value) and the blobs its segments point at, plus each
unfinished multipart upload's row, its lock/ACL rows, its part rows and the parts' blobs. `items` = current objects. The bucket's
own row (its settings) is not counted: an empty keeps it (a reset counts it). The database file is reported with what a compaction
would give back; blobs are not database files and are counted in the buckets.

**Notes** (counted apart):
- old versions and delete markers: `in 2 old versions and 1 delete marker: listed only by ListObjectVersions`, prune `old-versions`;
- unfinished multipart uploads: `in 1 unfinished multipart upload (1 part): no object uses them until they are completed or
  aborted`, prune `unused`;
- versions Object Lock holds now (retention not yet expired, or a legal hold): `... emptying here removes them anyway (AWS would
  refuse; this is your disk)`, no prune;
- versioning Enabled with no enabled lifecycle rule expiring noncurrent versions: a growth remark, 0 bytes, no prune.

**Prunes** (`before`, `prefix`, `old-versions`, `unused`), combined with AND:
- default: every version, delete marker and unfinished upload;
- `old-versions`: noncurrent versions and every delete marker (a key whose newest version was a marker goes for good);
- `unused`: unfinished multipart uploads only (aborted, parts and all). The contract has no "uploads" field; `unused` ("only what
  nothing uses") fits: no object uses a part until the upload completes. `old-versions` + `unused` takes both. **For the integrator
  to confirm**: the CLI's help for `--unused` should say it aborts unfinished uploads on S3.
- `prefix`: keys (and upload keys) under it; `before`: versions last modified (uploads initiated) before the time.

**Empty** keeps the bucket and all its configuration (versioning, policy, CORS, lifecycle, notifications, tags, ACLs, Object
Lock configuration, website, ownership): only version rows, upload/part rows and blobs are touched. Crash-safe in the API's own
order: the rows are committed and acknowledged on disk, then the blobs removed (reusing `Blobs`; `remove_counted` added, so
`freed` is what the disk got back), then `Db::compact`. A crash between leaves unreferenced blobs, swept by the existing unclean-start
sweep. No event notification is sent and no lifecycle rule is consulted (this is not an AWS call). The bucket's `next_seq` stays,
so a new version id never repeats one.

**Object Lock decision: a local empty overrides retention and legal holds** (COMPLIANCE included). Reason: it is the owner's own
disk, and a COMPLIANCE retention can never be lifted through the API, so without this the only way out would be deleting files by
hand, which is what this feature exists to spare people. It is never silent: usage carries the note above before anything is
removed, and a dry run counts those versions with the rest. The S3 API still refuses exactly as AWS does.

**Reset** removes every bucket, version, upload and part, then every blob in the region's `blobs/` (including ones a crash left with
nothing pointing at them), removes the lifecycle marker, compacts. A name is free at once. The janitor, if armed, wakes at the next
midnight, finds no rule and stops (no new timer is armed).

Tests (real temp dir, real socket): measured by kind; dry run removes nothing; old-versions, unused, before, prefix and whole each
remove exactly their part with `freed > 0` and blob files really gone; policy, CORS, versioning and the Object Lock configuration
survive; a restart loads what the live service held; `NotFound` for an absent bucket and an absent region; reset with dry run,
blobs dir empty after, restart, name re-used.

Calls: not an AWS API and new in this port (owner's request, 2026-11-15), so there is no AWS or Go behaviour to match; the S3 API's
answers are unchanged.

## Lambda (`crates/aws-lambda/src/{storage,data,usage}.rs`, test `src/tests/data.rs`)

**A resource is a function.** `units` = code packages.

**Its bytes**: its row, the rows of event source mappings whose `FunctionArn` names it, and the code packages its `$LATEST` and
published versions point at (each `code/<id>`: `code.zip` and the unpacked `root/`, measured on disk, so files a function writes
into its working directory, its task root, are counted too). `items` = distinct packages. Code run in place (`_local_`) is the
person's own directory and is not counted. Note: `in code only published versions run (N of them): DeleteFunction with a Qualifier
removes one`, no prune.

**Layers** are reported too, as `layer:<name>` (row + their versions' code), since they hold code a person may not know about.

**Leftovers** (pseudo-resources whose note carries `unused`):
- `(unused code)`: `code/<id>` directories no function, version or layer points at. They do happen: code is unpacked before the
  actor is asked to keep it, so a refused `CreateFunction` (409) or a failed `UpdateFunctionCode`/`PublishLayerVersion` leaves its
  package behind until the next delete sweeps.
- `(runtime clients)`, home region only: `<data>/lambda/shims`, the Python/Node/Ruby Runtime API clients, written again at the next
  process start. This is the only thing Lambda's runtime writes on its own; no other scratch directory exists (processes inherit the
  host's `TMPDIR`, which is the person's, not ours).

**Prunes**: only `unused` (name ignored), which removes the unused code and the runtime clients. Emptying either pseudo-resource by
name with no prune does the same. A function or a layer emptied any other way is refused with `Unsupported`: "a function's code is
what it runs, so it is not emptied: DeleteFunction removes a function and its code (with a Qualifier, one published version),
DeleteLayerVersion a layer version; --unused removes the code nothing uses". `before`/`prefix`/`old-versions` are refused the same.
`unused` leaves a package changed in the last 60 s (an operation may be between unpacking it and the actor keeping it; tests set the
grace to 0).

**Reset** removes every function (with versions, aliases, URL configs, policies), layer, event source mapping, code signing
configuration and poller checkpoint; awaits the rows on disk, then removes `code/` and the runtime clients, then compacts; then ends
every function's processes (`Runtime::retire`) and re-syncs the pollers so none keeps polling. A name is free at once.

Tests: a function with a published version and new code, a layer, a refused create's orphan, and (when `python3` is installed) an
invocation writing the clients: measured, refusals by name, `NotFound`, dry run, unused removes exactly the orphan and the clients
with `freed > 0`, the function and version 1 still answer and run (clients re-written); reset with a mapping and a URL config, dry
run first, all lists empty and `code/` gone after, the name re-used, and a fresh `Core` on the same directory loads exactly that.

Calls: not an AWS API and new in this port; the Lambda API's answers are unchanged.

## What could not be made to shrink, and what is open
- Lambda: `empty` on a function shrinks nothing by design (see above). `unused` changes no rows, so it does not compact the file.
- S3 compaction gives back little for small row counts; the blobs are where the bytes are, and they are counted in `freed`.
- Lambda reset/`unused` race: code unpacked for an operation that arrives during a reset is removed with the rest (the 60 s grace
  guards `unused` only). Rare, and the operation then answers with code that is gone; noted, not fixed.
- Removing the runtime clients while a process is between `shims::ensure` and exec could fail that one start; it is re-written on
  the next.
- S3 `unused` meaning "unfinished uploads" needs the integrator's CLI wording.

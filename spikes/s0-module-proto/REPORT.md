# S0: module protocol spike (docs/port/modules/PLAN.md, D3)

Date 2026-11-12. Mac: M5 Max, macOS 27. Linux: the `doze-ubuntu` Lima VM (Ubuntu 24.04, arm64, vz), static
musl build cross-compiled with rust-lld. Everything below was measured; commands at the end.

## Verdict

**The design holds. Use it: control socket as the module's stdin, 12-byte frame header, multiplexed calls in
both directions, streams with cancel, SCM_RIGHTS on the same socket, minicbor bodies with numbered fields.**
No `unsafe`, no go-plugin, no TLS, no pidfiles for modules.

## Results

| check | macOS | Linux |
|---|---|---|
| launch → hello (20 launches) | p50 1.6 ms, p99 2.0 ms | p50 0.11 to 0.28 ms |
| plan call (890 B request, 338 B reply), 10 k sequential | p50 26 µs, p99 35 µs | p50 27 µs, p99 41 to 44 µs |
| small call | p50 22 µs | p50 16 µs |
| 5,000 plan calls, 100 in flight | 13.7 ms | 29 ms |
| module call that calls the host back twice (resolve → 2 × fetch) | p50 41 µs, p99 54 µs | p50 47 µs, p99 71 µs |
| stream: 12 rows | 12/12 | 12/12 |
| stream cancel seen by the module | 0.05 ms | 0.03 to 0.05 ms |
| wire handoff (fd on the control socket, module calls host `boot`, connects backend, splices, `release`) | p50 100 µs, p99 117 µs | p50 161 µs, p99 194 µs |
| 200 concurrent handoffs, each client a unique token | 3.8 ms, 0 crossed, acquire/release balance 0 | same |
| module crash: the crashing call and an in-flight call | both fail "connection closed" in 0.8 ms | 0.2 ms |
| **host SIGKILLed** while the module is streaming and has a call in flight | module gone in 1.5 to 2.5 ms (10/10) | 1.3 to 1.4 ms (10/10) |
| RSS idle (host / module) | 3.3 / 2.7 MB | 1.1 / 1.0 MB |
| RSS after the load above | 8.4 / 8.8 MB (allocator keeps it) | 1.4 / 1.3 MB |
| whole spike binary (tokio, transport, three codecs, fixtures) | 1.0 MB | |

Against the go-plugin host spike (Rust host, real postgres module over gRPC): launch 7 to 10 ms → 1.6 ms;
p50/p99 39/56 µs → 26/35 µs; 5,000 concurrent 91 ms → 14 ms; host RSS 7.0 MB → 3.3 MB idle.
(Different payloads; same machine; the point is that nothing got worse.)

## Encoding

Same data (a postgres-sized PlanReq with an evaluated spec tree and spans, a SpawnPlan, a Hello with a
24-argument schema):

| | minicbor | postcard | prost |
|---|---|---|---|
| PlanReq bytes | 890 | 749 | 960 |
| PlanReq encode+decode (mac / linux) | 2.2 / 2.7 µs | 1.5 / 3.3 µs | 2.8 / 4.7 µs |
| SpawnPlan encode+decode | 0.48 µs | 0.40 µs | 0.39 µs |
| old build reads a message with new fields | **ok** | error | **ok** |
| new build reads a message without them | **ok** | error | **ok** |
| new enum variant in an `Option` field reaches an old build | **ok** (`None`) | error | n/a |
| code size per message type (opt-level s) | ~4 KB | ~1 KB | ~2.7 KB |
| needs | derive + `#[n(i)]` | serde derive | hand `#[prost(...)]` or .proto + build step |

**minicbor.** postcard cannot evolve (any skew is a hard error, so every protocol change would be a flag day
between core and every published module). prost evolves equally well and is a bit smaller, but its types are
protobuf-shaped (every message field `Option`, oneof wrappers, no plain Rust enums), which would leak into the
SDK's API or need a conversion layer. minicbor keeps ordinary Rust types. Rules that make evolution work,
proven here: append fields with a new index; new non-`Option` fields carry `#[cbor(default)]`; enums that may
grow are carried as `Option<Enum>` (an unknown variant decodes as `None`). Cost: about 200 KB for a ~50-type
protocol; acceptable, and to be re-measured on the real message set.

## Design findings (go into the SDK)

1. **stdin is the control socket, no `unsafe`.** The host passes its socketpair end as the child's stdin
   (`Stdio::from(OwnedFd)`); the module takes a CLOEXEC duplicate (`as_fd().try_clone_to_owned()`).
2. **The module must then point fd 0 at /dev/null** (`rustix::stdio::dup2_stdin`). Without it, every program the
   module runs (psql, mariadb, temporal) inherits the control socket: measured, a grandchild's stdin was the
   socket. With it: not a socket.
3. **macOS has no `MSG_CMSG_CLOEXEC`.** Received fds get CLOEXEC set right after `recvmsg`. There is a tiny
   window in which a concurrent fork elsewhere in the module could inherit a client fd; the SDK's runner
   spawns from the runtime thread, so keep it that way (Linux sets the flag atomically).
4. **An fd frame goes alone.** The writer sends a frame carrying an fd in its own `sendmsg` so the fd rides
   with that frame's first byte. Under 200 concurrent handoffs the receive queue never held more than one fd
   (instrumented); unique per-client tokens proved no fd was paired with the wrong call.
5. **Host death needs nothing but EOF**, as long as the module's reader task can run. The SDK must keep
   blocking work off the runtime thread (`spawn_blocking`), or a module stuck in sync work would outlive its
   host until it returns.
6. **Calls in both directions on one connection work without deadlock**, because every incoming call is
   served on its own task.
7. **Memory after bursts is the allocator's, not the protocol's** on macOS (8.8 MB retained after 200
   concurrent splices with 64 KB buffers; 1.3 MB on Linux musl). Size the read buffer and splice buffers in
   the postgres step and measure there.

## Not covered (carry into step 1)

- **Backpressure.** Channels here are unbounded. A `Tail` stream to a slow consumer would grow without
  limit. Production: bounded per-stream credit (the host grants N items, the module waits).
- Protocol negotiation beyond "hello says protocol 2"; frame size limit behaviour (16 MiB cap untested).
- x86-64 Linux and glibc builds (arm64 musl only); Linux run in a VM, not on metal.
- Realistic message set and its code size; the `Value`/schema derive (step 2).

## Reproduce

```
cargo build --release
target/release/s0-module-proto bench     # codecs, sizes, version skew
target/release/s0-module-proto host      # transport scenarios
target/release/s0-module-proto orphan    # module exits when the host is SIGKILLed
# size probes: target/release/probe_{none,cbor,pc,pbnone,pb}; opt-level s via CARGO_PROFILE_RELEASE_OPT_LEVEL=s
# Linux: rustup target add aarch64-unknown-linux-musl
#        CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld cargo build --release --target aarch64-unknown-linux-musl
#        limactl copy … doze-ubuntu:/home/srini.linux/s0 ; limactl shell doze-ubuntu -- ./s0 host
```

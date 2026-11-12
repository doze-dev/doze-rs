# Names: `.doze` in Rust

Owner, 2026-11-13: "Have we built the whole DNS system yet? We had to work pretty hard on the golang code to get the
right combination there." No: nothing in doze-rs claims, serves or resolves a `.doze` name. The Rust doze-aws and
doze-kafka, run standalone, have no `aws.doze` / `kafka.doze`, which the Go ones do. This is the plan to port it.

## The rule for this port

The Go package `doze-names` (v0.4.0: 5,611 lines in 31 files, 4,144 lines of tests, 97 tests) is not inspiration
here, it is the specification. Its comments record measured decisions (below); each one is kept, cited, and held by
a Rust test ported from the Go test that proves it. The Rust code is written Rust's way (rust-port-write-it-in-rust);
the behaviour is Go's, except where this plan says why not.

During the move the two live on one machine: Go doze beside Rust doze-aws, or the reverse. So the Rust crate reads
and writes **the same files the same way**: `names.json` (format 2) under the shared home, `flock` on the sidecar
`names.json.lock` (std `File::lock` is `flock` on Unix: compatible), atomic rename, the ports file, the routes
record, the CA folder. `format_test.go` and `zone_switch_test.go` become cross-language tests: the Go library and the
Rust crate read and write one file in turn.

## The decisions it keeps (from the Go comments)

| Decision | Why (measured, in Go's comments) |
|---|---|
| No binary owns the zone: first to bind the resolver answers for all; the next takes over | install order must not matter (doze, doze-aws, doze-kafka) |
| Two tiers: apex (`aws.doze`, first-come, the loser is told who holds it) and qualified (`svc.stack.doze`, never contested) | a standalone engine needs a name with no stack |
| Every name gets its own address; apex addresses fixed at offsets 2 to 9, the rest hashed from 10 | every Kafka on 9092, every AWS on 80; Linux writes apex names into /etc/hosts once |
| Base 127.0 on Linux and Windows; 198.19 (lower half) on macOS | macOS needs an alias per loopback address; 250 aliases took mDNSResponder to a full core and lookups from 10 ms to over 1 s, 4,000 to 150 s, configd killed |
| macOS: one daemon (`netd`) owns a tunnel with the whole block routed to it and translates each packet to the service's private port | the alternative above; nothing kept per connection |
| Resolver not on port 53; on Linux at 192.0.2.53 (TEST-NET-1) on interface `doze0` | systemd-resolved holds :53 on the wildcard and refuses loopback DNS servers |
| macOS routes the zone with `/etc/resolver/doze` (unicast) | getaddrinfo drops non-127.0.0.1 loopback answers learned over mDNS |
| Linux: who owns resolv.conf is read from its marker comment, not the process table | Tailscale's lesson; plain resolv.conf cannot split DNS, so those machines get apex names from /etc/hosts only, said plainly |
| The /etc/hosts block is bounded by markers and replaced idempotently | DDEV once grew users' hosts files without end |
| One HTTP front door on the IPv4 wildcard :80/:443, routing by Host/SNI, first-come like the resolver | macOS binds a privileged port only on the wildcard; `:80` takes IPv6 and silently loses IPv4 |
| Names outside the zone that a route sent here are forwarded to the machine's own upstreams; zone typos are NXDOMAIN | a routed `dev.acme.com` must work with doze down; netd answers when no program does |
| The ports file beside the registry, and heal | an older program dropped `Entry.Ports` when it rewrote the file |
| The daemon's table re-read on each connection when the file changed (inode + time) | ext4's clock ticks let two writes look like none |
| One machine CA under `<home>/ca`, leaf certificates minted per domain on first TLS handshake, rotation | mkcert's and OrbStack's trade |
| Everything privileged in one script under one prompt; each step reported separately by Check | a half-finished install says which half |
| Routes record which project asked for each domain; only listed routes are ever removed | a machine-wide route must know who still wants it |

## Where Rust can do better (D16)

- **Windows.** Go has stubs (24 + 17 lines): no names there. Windows sends a domain to a server through the NRPT
  (`Add-DnsClientNrptRule -Namespace .doze -NameServers <ip>`), one privileged command, and 127.0.0.0/8 is all local
  as on Linux. Microsoft documents `-NameServers` as addresses only (the `host:port` form is documented for the
  DirectAccess proxy, not for it), so on Windows the resolver listens on port 53 at an address of its own
  (127.0.0.53), unlike the other systems. A spike in the elevated Windows CI runners proves it before N5 builds on
  it (learn.microsoft.com/powershell/module/dnsclient/add-dnsclientnrptrule).
- **One copy of every rule.** Go's setup scripts are strings assembled per OS; Rust builds each step as data
  (`Step { check, apply, undo, says }`) so Check, the prompt text and the script come from the same list.
- **Parsers fuzzed**: the DNS wire, the packet translation, resolv.conf, scutil output, the hosts block.

## Libraries (asked: tiny ones)

| Need | Choice | Why |
|---|---|---|
| DNS messages | hand-written, as Go's (crate `wire.rs`, about 100 lines); `simple-dns` 0.13 (MIT, no dependencies) is the fallback if more record types are ever needed | the zone answers one question with A, empty NOERROR or NXDOMAIN, and caps relayed TTLs: no dependency earns its place for that. `hickory-proto` brings rand, url, ipnet, tracing; `domain` is larger. Fuzzed (`fuzz_wire`). |
| CA and leaf certificates | `rcgen` 0.14 on `ring` | doze-rs already uses rustls with ring (doze-kafka TLS): no second crypto stack |
| TLS at the front door | `tokio-rustls` (already in the workspace) | SNI picks the leaf |
| macOS tunnel | written in `doze-os` (utun: a `PF_SYSTEM` socket, `CTLIOCGINFO`, connect; ~100 lines) | the only `unsafe` stays in doze-os (D11); `tun-rs` would add a dependency for a few calls |
| Packet translation | hand-written IPv4/TCP rewrite with checksum update | a dozen lines of arithmetic the Go version also writes by hand; fuzzed |
| HTTP routing at the front door | hyper 1 (already in the workspace: aws-gateway) | Host header, streaming |

## Who uses names, and how modules plug in (owner, 2026-11-13)

"doze-aws plugs into the DNS for all of its services, which means other modules should be able to do it too."

- **Subtree ownership is how.** A registered name owns every name beneath it (most specific wins; the walk stops
  before `.doze`). doze-aws holds `aws.harbour.doze` and so answers `sqs.ap-south-1.aws.harbour.doze` and API Gateway
  ids minted at runtime, with no claim per service. The rule is the registry's, not doze-aws's: every name has it
  (ported and tested in N1, `a_registered_name_owns_its_subtree`).
- **Modules (protocol 2).** A module declares listeners with routes (`@`, a label, `prefix-@`); the core claims a
  qualified name per granted host (`Granted.hosts`, already in the protocol), routes it at the listener's backend for
  the front door, and twins the extra hosts (Go's `Lease.Twin`, e.g. `sync-aws.shop.doze`, and declared domains).
  So any module gets what doze-aws has: its names, everything under them, and `http://<name>` through the front
  door. A module never touches the registry itself: the core does, so the zone has one writer per stack.
- **Standalone engines** (doze-aws, doze-kafka run on their own) claim apex names (`aws.doze`, `kafka.doze`) through
  this crate directly, as the Go ones do (N6).
- **The core** (doze CLI and app daemon), as Go doze does today: a lease per instance and its twins, routes, the
  stack claim (`claim_stack`, refusing a stack name another project holds), its own live resolver consulted before
  the registry (Go `ServeResolveIn`: a name answers the moment a service is added), the front door, recorded domains
  for `trust`, the CA, and `pool_usable` to decide addresses or 127.0.0.1.

## Root: what needs it, and when (owner, 2026-11-13)

The networking needs root once per machine, never per run, as in Go:

- **One prompt for setup**: `doze dns-setup` (also `doze doctor --fix`, and the app's Repair) runs one privileged
  script whose steps are reported one by one (`Check`): on macOS the network daemon (launchd, root only to open the
  tunnel and add the route, then it drops to the user for good) and `/etc/resolver/doze`; on Linux the `doze0`
  interface with 192.0.2.53, the resolved or dnsmasq route, the daemon's systemd unit, the marked `/etc/hosts` block;
  on Windows the NRPT rule. The app shows what the script will do before the password (Go's `RepairPlan`).
- **One prompt for trust**: `doze trust` routes a project's own domains to the resolver and trusts the doze CA
  (keychain, ca-certificates, certutil), and removes stale routes in the same prompt.
- **Nothing else**: the resolver is on a high port, services listen on their own addresses (Linux, Windows) or
  behind the daemon (macOS), and the front door's :80/:443 needs no root on macOS (the wildcard) and needs the
  privileged-port sysctl on Linux, which setup sets. Every program runs as the user afterwards.
- **Tests**: N1 to N3 need none. N4/N5 run with root on the owner's Mac (the netd end-to-end tests, as Go's
  `e2e_darwin_test.go`), in the Lima VM (Linux setup), and on the elevated Windows runners.

## TLS and proxying (owner, 2026-11-13)

- **Here**: the front door's TLS. One CA per machine under `<home>/ca` (made under a lock, key readable by its owner
  only, rotation), a short-lived leaf minted per domain on the first handshake that asks and kept beside it, a
  person's own certificate served instead when they bring one (`Lease::certificate`), plain http to a declared
  domain redirected to https, and no certificate for a peer that is not this machine. N3, on rustls (already in the
  workspace) with rcgen.
- **Not here, recorded so it is not lost**: the core's client-facing proxy (Go `internal/proxy`: one listener per
  instance, boot on first connection, splice) and its TLS termination for engines that need it (`tls.go`: a cached
  self-signed certificate, enough for postgres `sslmode=require`). In protocol 2 that is the core's proxy plus the
  module's wire filter (`Wire` capability; postgres in step 11). See docs/port/modules/PLAN.md.

## The crate

`crates/doze-names` (library; used by the doze core, doze-aws and doze-kafka), platform code in `doze-os`:

```
crates/doze-names/src/
  lib.rs        Name, Tier, InZone, the zone constants and their reasons
  pool.rs       addresses: base per OS, apex offsets, hashed offsets
  registry.rs   names.json: open, claim, release, leases, liveness, format 2, the lock
  ports.rs      the ports file; heal.rs
  routes.rs     which project asked for each domain
  domains.rs    declared domains, wildcards
  resolver.rs   UDP+TCP server: first-come bind, take-over, answer from the registry
  upstream.rs   the machine's upstreams (scutil, resolv.conf); forward
  ingress.rs    the front door: first-come :80/:443, Host and SNI routing
  ca.rs         the CA: load or create under a lock, rotate, mint leaves
  listen.rs     Lease::listen / bind_addr / forward: hides bind-vs-translate
  setup/        steps as data: mac.rs (netd, /etc/resolver), linux.rs (resolved, dnsmasq, doze0, hosts),
                windows.rs (NRPT), check, install, uninstall; trust.rs (keychain, ca-certificates, certutil)
  netd/         the daemon: tunnel loop, translate (nat.rs), table cache, resolver when no program holds it
crates/doze-os/src/tun/       utun on macOS
```

## Steps

| # | What | Tests (ported Go tests) | Days |
|---|---|---|---|
| N0 | This plan; cross-language format test harness (Go library vs Rust crate, one home) | format, zone_switch | 1 |
| N1 | Names, pool, registry, leases, liveness, ports file, routes, domains | registry, allocator, domains, subtree, stack, peers, private_zone, heal, format | 4 |
| N2 | Resolver and upstream: first-come bind and take-over between processes, zone answers, forwarding | resolver, peers, private_zone | 3 |
| N3 | CA and front door: Host/SNI routing, leaf minting, first-come :80/:443 | ca, ingress | 3 |
| N4 | Listen/forward; macOS netd: tunnel, translation, table cache (privileged parts tested on the owner's Mac) | listen, nat, netd, e2e_darwin | 5 |
| N5 | Setup and trust per OS as data: macOS, Linux (in the Lima VM with sudo), Windows (NRPT, in CI) | setup, setup_linux, setup_netd_linux, setup_darwin, resolvconf, trust | 5 |
| N6 | Wire into Rust doze-aws and doze-kafka (`aws.doze`, `kafka.doze` again), then the core | their own e2e | 2 |

About 23 days. N0 to N3 need no privilege and run unattended; N4 and N5 need root (the owner's Mac, the Lima VM, the
elevated Windows runners). Tests never touch the real zone: a private home with `DOZE_ZONE_RESOLVER` and
`DOZE_ZONE_INGRESS` pointed at test ports, as the Go tests do.

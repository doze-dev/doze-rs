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
| DNS messages | `simple-dns` 0.13 (MIT, no dependencies) | parse and build packets; a resolver for one zone needs nothing more. `hickory-proto` brings rand, url, ipnet, tracing; `domain` is larger too. Fuzzed at our boundary. |
| CA and leaf certificates | `rcgen` 0.14 on `ring` | doze-rs already uses rustls with ring (doze-kafka TLS): no second crypto stack |
| TLS at the front door | `tokio-rustls` (already in the workspace) | SNI picks the leaf |
| macOS tunnel | written in `doze-os` (utun: a `PF_SYSTEM` socket, `CTLIOCGINFO`, connect; ~100 lines) | the only `unsafe` stays in doze-os (D11); `tun-rs` would add a dependency for a few calls |
| Packet translation | hand-written IPv4/TCP rewrite with checksum update | a dozen lines of arithmetic the Go version also writes by hand; fuzzed |
| HTTP routing at the front door | hyper 1 (already in the workspace: aws-gateway) | Host header, streaming |

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

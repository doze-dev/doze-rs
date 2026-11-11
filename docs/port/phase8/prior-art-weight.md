> Research record, 2026-11-12, kept as evidence for [`../phase8.md`](../phase8.md). Sizes are darwin/arm64 from a scratch echo server, not the broker; re-measure on the Linux CI job before fixing a limit.

# Phase 8 (security): prior art and weight

Date 2026-11-12. Measured on darwin/arm64 (Apple silicon), rustc/cargo 1.99.1, release profile copied from the repo
(`lto = "thin"`, `codegen-units = 1`, `strip = true`, `panic = "abort"`; opt-level is the default 3, as in the repo).
Scratch projects (kept, small): `/private/tmp/claude-501/-Users-srini-Code-doze-dev/014d8981-fa59-41b3-a494-fc4a4035c0bb/scratchpad/phase8/weight/{echo,scram,bench.sh}`.
Their target dirs, a repo copy used for `cargo tree` and the certs were deleted.

Two things to know before reading the numbers:

1. **The CI size gate is not 4.2 MB on Linux.** `.github/workflows/ci.yml` enforces `size <= 5,150,000` (Linux x86_64,
   measured 4,899,328 B), `<= 115` packages and `<= 8192 KB` idle RSS. The 4.2 MB figure in `weight.md` is the darwin/arm64
   number (4,029,504 B). Headroom: darwin 4.2 MB limit leaves 170 KB; Linux leaves 250 KB. I give both below.
2. **CI counts packages with `cargo tree -p doze-kafka --prefix none --edges normal | sort -u | wc -l`**, which counts a
   package twice when it appears once plain and once with the ` (*)` dedup marker. Today that gives 107 (92 distinct
   name+version pairs). I computed every package delta both ways, against a copy of the real workspace (cargo tree only, no build).

---

## Part 1: prior art

### What each comparable system does

| System | Mechanisms | ACLs | How users are created | Default posture | TLS |
|---|---|---|---|---|---|
| **Apache Kafka (KRaft)** | PLAIN, SCRAM-SHA-256/512, GSSAPI, OAUTHBEARER | `StandardAuthorizer`, ACLs in the metadata log, `super.users`, `kafka-acls.sh` | PLAIN: `user_<name>="<pw>"` lines in a JAAS file (clear text on disk). SCRAM: `kafka-storage format --add-scram` at format time, then `kafka-configs --alter --add-config 'SCRAM-SHA-256=[iterations=8192,password=...]'` (the AlterUserScramCredentials API). Default iterations 4096. | Open (PLAINTEXT listener). With an authorizer, a resource with no ACL is super-users-only; `allow.everyone.if.no.acl.found=true` reverses that and the docs call it a dev/test/migration aid. | Keystore/truststore (JKS/PKCS12/PEM) per listener; docs say SCRAM and PLAIN "should be used only with" TLS. |
| **Redpanda** | SCRAM-SHA-256 (default), SCRAM-SHA-512, PLAIN opt-in via `sasl_mechanisms` | Kafka-compatible ACLs via `rpk security acl create`; new users have no rights; `superusers` cluster property | `rpk security user create <u> -p <pw> --mechanism SCRAM-SHA-256` (goes through the Admin API on :9644, same credential store as the Kafka API). `RP_BOOTSTRAP_USER=user:pass` (compose examples) creates the first SCRAM superuser on first start only; it does not turn auth on. | Open until `rpk cluster config set enable_sasl true` (no restart) or per-listener `authentication_method: sasl`. Docs warn: enabling without a superuser locks you out. | `kafka_api_tls` with `cert_file`, `key_file`, `truststore_file`, `require_client_auth`; docs show hand-made openssl self-signed certs. |
| **Nisshi (formerly Tansu)**, Rust, pluggable storage | SCRAM-SHA-256 and -512 only | **None**: "Nisshi does not have an authorizer", so any logged-in user can call every API | `nisshi user create alice alice-secret --mechanism scram256`, a CLI subcommand that writes into the storage engine (sqlite/postgres only; `memory://` and `s3://` refuse to start with auth). Procedure: start open on 127.0.0.1, create the user, restart with `--authentication`. The docs admit the password lands in shell history. | Open; `--authentication` is off by default. Before login only ApiVersions/SaslHandshake/SaslAuthenticate are answered. | `--cert` and `--key` PEM (PKCS#8, SEC1 or RSA, optional `--key-passphrase-file`). Listener becomes TLS-only; any load error stops startup and the broker "never falls back to plaintext". |
| **Bufstream** (Buf, JVM-free Go) | SASL page lists OAUTHBEARER; mTLS as the other principal source (`principalSource`: SUBJECT_COMMON_NAME, SAN_DNS, SAN_URI, ANONYMOUS); no SASLprep | Kafka ACLs via `kafka-acls.sh` once an `authorization:` section names super users | Identity comes from the IdP/cert, not from a user store. | Open unless configured (Helm values). Its conformance page still lists SASL/ACLs as unimplemented; the pages disagree, probably stale. | mTLS |
| **WarpStream agent** | SASL/PLAIN and SCRAM-SHA-512 | ACLs from the console | Console "Create Credentials" (shown once). Local mode: in-memory, `-requireSASLAuthentication` prints a generated username and password at startup; a `-saslUser user:pass` flag exists for `local` mode (from the CLI reference summary). | Plaintext and open; enforcement is opt-in (`WARPSTREAM_REQUIRE_SASL_AUTHENTICATION=true`). Docs tell you to add TLS whenever SASL is on. | Agent-side TLS config |
| **franz-go `kfake`** (Go in-process fake cluster) | PLAIN, SCRAM-SHA-256, SCRAM-SHA-512 | `EnableACLs()`: deny by default, superusers bypass; `User(method,user,pass,acls...)` seeds a user with ACLs | `EnableSASL()`; `Superuser(method,u,p)`; `User(...)`. **If no user is configured the default superuser is `admin`/`admin` (SCRAM-SHA-256).** "If you delete all SASL users, the kfake cluster will be unusable." | Open; both SASL and ACLs are explicit options. | `TLS(*tls.Config)`: the caller supplies the cert |

I did not find anything for "MSK local"; there is no such artifact that I could locate. Kafka-in-Docker images
(Bitnami, Confluent, Apache's own `docker/examples`) are the de facto local mode and are configured purely through
`server.properties` or env, which is where most of the complaints come from (below).

### Local-dev TLS practice

- **mkcert** (https://github.com/FiloSottile/mkcert): creates a private CA once, `mkcert -install` puts it in the system,
  Firefox/Chrome and Java trust stores, then `mkcert localhost 127.0.0.1` signs a leaf. It warns that `rootCA-key.pem` "gives
  complete power to intercept secure requests from your machine". Node needs `NODE_EXTRA_CA_CERTS`, other devices need the CA copied.
- **Caddy** (https://caddyserver.com/docs/automatic-https): generates its own local CA, tries to install the root into the OS
  trust store at first use (prompts for a password; documented to fail in containers and unprivileged services).
- **The Kafka ecosystem mostly skips this.** Typical compose files use `SASL_PLAINTEXT` locally (the usual local mode is
  plaintext + SASL, or fully open), and TLS only where the test is about TLS. Brokers that do TLS take PEM files and leave
  cert creation to the user (Nisshi, Redpanda docs, kfake). None of the ones I read generate certs themselves.
- Kafka clients do not read the OS trust store: Java uses its own truststore, librdkafka takes a CA file. So a
  Caddy-style "install into the system store" buys nothing for Kafka clients; what helps is **printing the CA path**
  and accepting PEM everywhere.

### What developers complain about (evidence is thin; this is what the sources show)

- Listener plumbing: `advertised.listeners` must match what the client resolves, listener-name-to-protocol maps, and the
  Apache Docker examples note that `advertised.listeners` has to travel with the SSL properties in a file, not as a separate
  env var (https://apache.googlesource.com/kafka/+/refs/heads/3.7/docker/examples).
- Keystore/truststore paths and passwords duplicated on broker and client, and dated ZooKeeper-era compose files copied into
  KRaft setups (https://forum.confluent.io/t/setting-up-ssl-woes/13407,
  https://forum.confluent.io/t/self-hosted-kafka-with-kraft-ssl-and-sasl-scram-sha-256/8863).
- SCRAM under KRaft: credentials must be present on every controller when set with `--add-scram`; a mismatch between the
  formatted credential and the JAAS login gives "Invalid credentials" between brokers.
- Lock-out: enabling auth with no superuser (Redpanda docs say this outright); kfake documents the same trap.
- Plaintext secrets: Kafka's default PLAIN keeps passwords clear in the JAAS file; Nisshi's CLI takes the password as an argument (history leak).

### Ranked design options for doze-kafka

**(a) How users and credentials are provisioned in a single-binary local broker**

1. **All three, layered (recommended).** (i) A bootstrap source: `--sasl-user name:password` (repeatable) and a
   `DOZE_KAFKA_SASL_USERS` env, or a `[[users]]` block in the config file, with passwords plain or already hashed
   (`SCRAM-SHA-256$4096:salt$stored:server`); these are re-applied at every start and are the superusers. (ii) The Kafka
   `AlterUserScramCredentials` / `DescribeUserScramCredentials` APIs for runtime changes from `kafka-configs.sh`, franz-go,
   etc., stored in the metadata (redb). Reason to have both: the API alone has the lock-out and chicken-and-egg problem that
   Redpanda and kfake each solve with a bootstrap user, and the oracle/real tooling expects the API. Note the API client sends
   the *salted password* already, so the broker does no PBKDF2 for it; PBKDF2 is only needed to turn a plaintext bootstrap password into stored credentials,
   once at start.
2. **Bootstrap flags/file only.** Smallest and enough for local dev (Nisshi-like, WarpStream `-saslUser`), but fails any
   conformance scenario that calls the SCRAM APIs and cannot add a user to a running dev broker.
3. **API only** (Kafka KRaft, Redpanda without bootstrap). Most faithful, worst local experience; needs the lock-out guard.
   Not recommended unless paired with 1(i).
- Avoid a Nisshi-style offline `user create` that writes the store directly: it races the running broker and the data-dir lock.
- If SASL is enabled with no user defined anywhere: generate one superuser and print it at startup (the WarpStream
  pattern) rather than a fixed `admin/admin` (kfake) or a dead cluster.

**(b) Default posture**

1. **Open by default (plaintext, no authentication, no authorizer), everything opt-in (recommended).** This is what Kafka,
   Redpanda, WarpStream, Nisshi and kfake all do, and it keeps `kcat`/`kafka-console-producer` zero-config. Add a loud
   startup warning if it listens on a non-loopback address without auth (ties into the planned allowed-hosts guard).
   Once SASL is on, unauthenticated clients get nothing beyond ApiVersions/handshake (Nisshi's rule, matches Kafka).
2. Authorization separate from authentication: ACL CRUD APIs always work and are stored (needed for conformance), but
   enforcement is a switch (`--acl-enforce`). When enforcing, follow Kafka's `StandardAuthorizer`: no ACL means super users only,
   with `allow.everyone.if.no.acl.found` as the dev escape hatch. Nisshi shows "authenticated means all-powerful" is acceptable for a dev tool but it fails the ACL conformance scenarios.
3. Secure-by-default with generated credentials for every start: safest, but breaks zero-config clients and the existing
   suite. Rank last for a local tool.

**(c) TLS provisioning**

1. **PEM files, never fall back (recommended baseline):** `--tls-cert`, `--tls-key` (PKCS#8/SEC1/RSA, parsed with
   `rustls-pki-types` `PemObject`); any error aborts startup (Nisshi's rule). Document the two-line `mkcert` recipe. Costs nothing beyond rustls.
2. **Optional `--tls-dev` that creates a local CA + leaf in the data dir on first start** and prints the CA path for
   `ssl.ca.location` / a PEM truststore. In-process generation with `rcgen` costs +68 KB and +9 packages (measured, below);
   shelling out to `openssl` is free in weight but is absent on Windows and on many minimal containers. Do not copy Caddy/mkcert's
   install-into-trust-store step; Kafka clients do not use the OS store. Defer until asked.
3. **Plaintext + SASL (SASL_PLAINTEXT) as the documented local mode, TLS only on request.** Matches what compose files do, and
   Kafka itself warns that PLAIN/SCRAM want TLS, so the startup log should note it when SASL is on without TLS on a non-loopback bind.
   mTLS principal mapping can wait; Bufstream shows it is a separate feature.

Sources: Kafka SASL https://kafka.apache.org/42/security/authentication-using-sasl/ ; Kafka authorization
https://kafka.apache.org/42/security/authorization-and-acls/ ; KIP-900 (`--add-scram`)
https://cwiki.apache.org/confluence/spaces/KAFKA/pages/240881535 ; Redpanda authentication
https://docs.redpanda.com/streaming/25.2/manage/security/authentication/ ; Redpanda TLS
https://docs.redpanda.com/streaming/24.2/manage/security/encryption/ ; Redpanda bootstrap user (compose samples)
https://docs.redpanda.com/23.2/reference/docker-compose ; Nisshi/Tansu
https://raw.githubusercontent.com/nisshi-io/nisshi/main/README.md and `.../docs/security.md` ; Bufstream
https://buf.build/docs/bufstream/authorization/acls , https://buf.build/docs/bufstream/auth/mtls , https://buf.build/docs/bufstream/auth/sasl ;
WarpStream https://docs.warpstream.com/warpstream/kafka/manage-security/sasl-authentication ,
https://docs.warpstream.com/warpstream/reference/cli-reference/warpstream-local ; kfake
https://pkg.go.dev/github.com/twmb/franz-go/pkg/kfake ; mkcert https://github.com/FiloSottile/mkcert ; Caddy
https://caddyserver.com/docs/automatic-https .
Not verified from primary pages: WarpStream `-saslUser` (search summary only), Redpanda `RP_BOOTSTRAP_USER` semantics
(compose samples via search summary), Bufstream SASL mechanisms beyond OAUTHBEARER. SCRAM iteration bounds (Kafka
accepts 4096 to 16384, from memory) not checked.

---

## Part 2: crates and weight

### Method

Package `echo`: tokio echo server (`rt`, `rt-multi-thread`, `net`, `time`, `io-util`, `macros`, same as the repo's tokio use),
with feature-selected rustls providers; a matching client for handshake timing. TLS 1.3 + 1.2 unless stated
(`tls12` costs about 52 KB). Server cert: ECDSA P-256 self-signed (openssl); also tried RSA-2048. Idle RSS = `ps -o rss` one second after start
(three worker threads on this machine are the default). "Packages" are two numbers: **dist** = distinct name+version pairs
in `cargo tree --edges normal`; **CI** = the repo's `sort -u | wc -l` method. "Marginal" columns come from adding the
dependency lines to a *copy* of `crates/kafka-server/Cargo.toml` in a scratch copy of the workspace and running the CI
`cargo tree -p doze-kafka` (baseline 107 CI / 92 dist). Nothing was built in the repo.

### Table A: TLS stacks (stripped release server binary)

| Configuration | Binary | Delta vs no-TLS | Packages added (standalone dist) | Marginal on the real tree (dist / CI) | Idle RSS: compiled in, unconfigured / TLS configured, no conn / 1 open conn | 100 full handshakes (client+server, P-256 / RSA-2048) | Build needs | Notes |
|---|---|---|---|---|---|---|---|---|
| (1) no TLS, echo | 464,928 B | baseline | 12 total | n/a | 2,656 KB | n/a | Rust only | |
| (2) rustls 0.23 + tokio-rustls 0.26, **ring 0.17** | 1,337,536 B | **+872,608 B** | +11 | **+9 / +12 (107 to 119)** | 2,752 / 3,312 / 3,600 KB | 27-30 ms / 61 ms | C compiler + assembler (ring's build script) | cargo deny: licenses, advisories, bans OK. Pulls `windows-sys 0.52` on Windows, a duplicate of tokio's 0.61 (warn only). TLS 1.3 only: 1,285,744 B (+820,816) |
| (3) rustls + **aws-lc-rs** (aws-lc-sys 0.47) | 2,779,872 B | **+2,314,944 B** | +10 | +9 / +12 (119) | 2,896 / 3,728 / 4,368 KB | 46 ms / 77 ms | C compiler; built here without cmake (cc builder) in 18 s. Windows/other targets not measured. | cargo deny OK. 2.65x ring's size for no benefit here |
| (4) rustls + **rustls-rustcrypto 0.1.0-pre.0** (pure Rust) | 1,495,840 B | **+1,030,912 B** | +65 | +62 / +90 (154 dist / 197 CI) | 2,784 / 3,264 / 3,536 KB | 63 ms / 137 ms | Rust only, no C | **Fails cargo deny advisories**: `paste` RUSTSEC-2024-0436 (unmaintained) needs an `ignore` entry; pre-release crate; pulls `rsa`, `p384`, `ed25519-dalek` etc. even though a dev cert is P-256. TLS 1.3 only: 1,459,984 B |
| (5) native-tls/openssl | excluded by brief | | | | | | | |

Handshake figures are ms for 100 full handshakes on loopback with session resumption disabled, steady state (the first run of each
series is about 2x slower from warm-up and is excluded). Every stack is under 1.5 ms per handshake, so speed is irrelevant for a dev broker.
Not measured: `rustls-pure` 0.15.4 (TLS 1.3 only, single author, new), Linux and Windows builds, x86_64 sizes.

PEM and certificate options (all measured on the ring build unless noted):

| Option | Cost | Verdict |
|---|---|---|
| `rustls-pki-types` built-in `PemObject` (`CertificateDer::pem_slice_iter`, `PrivateKeyDer::from_pem_slice`) | 0 extra packages (pki-types is already a rustls dependency); the ring row above already includes it | **Use this.** Handles PKCS#8, SEC1 and RSA keys |
| `rustls-pemfile` 2.x | +1 package | **Fails cargo deny**: unmaintained, RUSTSEC-2025-0134. Not needed. |
| `rcgen` 0.14 (`crypto`, `pem`, `ring`; default features off), in-process self-signed generation | **+68,240 B**, +9 dist / +10 CI packages (`rcgen`, `pem`, `base64`, `yasna`, `time`, `deranged`, `num-conv`, `powerfmt`, `time-core`); CI 119 to 129. Generated cert verified against `openssl s_client` (TLS 1.3). | cargo deny OK. Worth it only if `--tls-dev` is wanted; it also brings `base64` |
| `openssl` / `mkcert` out of process | 0 bytes | LibreSSL on macOS needed `-addext` and a separate `ecparam` step to get a key ring accepts (its `req -newkey ec` produced a PKCS#8 key without the public part, which rustls rejected), so shelling out is fiddly; absent on Windows |
| Not generating at all (PEM files only) | 0 | Baseline |

### Table B: SCRAM and PLAIN crypto

Standalone `scram` package (baseline: hello-world std binary 339,728 B), and the same code linked into the ring-TLS server to get the true marginal cost.
Each variant implements the full server side for SCRAM-SHA-256 and -512 (credential derivation, client-proof verification with a
constant-time compare, server signature), plus a client proof to check it, and the tampered-message check passes in every variant.

| Configuration | Standalone delta | Marginal on ring-TLS server | Marginal on the real tree (dist / CI) | Notes |
|---|---|---|---|---|
| **ring primitives** (`hmac`, `digest`, `pbkdf2`, `constant_time` all from ring) + 30-line hand-rolled base64 | **+34,400 B**, 6 pkgs total | **+34,256 B, +0 packages** | **+3 / +3 (no TLS, 107 to 110)** | cargo deny OK. SHA-256 and SHA-512 both supported |
| ring + `base64` crate | +51,072 B, 7 pkgs | | | the crate adds 17 KB over hand-rolled |
| **RustCrypto** `hmac` 0.13 + `sha2` 0.11 + `pbkdf2` 0.13 + `subtle` 2.6 + `base64` 0.23 | +34,432 B, 17 pkgs | +51,168 B, +14 pkgs (37 total) | **+14 / +17 (107 to 124)**; without `pbkdf2` and `base64` it is still +12 / +14 (121) | pure Rust, no C compiler; busts the 115 package gate by itself |
| `scram` 0.6 crate | +34,256 B, 13 pkgs | | | SHA-256 only (Kafka needs 512); pulls **ring 0.16**, `rand 0.8`, `base64 0.13`. **Fails cargo deny**: ring 0.16.20 has RUSTSEC-2025-0009 and -0010 and no licence field. Rejected |
| `rsasl` 2.3 (`plain`, `scram-sha-1`, `scram-sha-2`; client-side probe) | **+302,560 B**, 38 pkgs | | | cargo deny OK; far too heavy, plus `stringprep`/unicode tables and SHA-1 it does not need |

PBKDF2-HMAC, 4096 iterations (Kafka's default), release mode, average of 50 on this Apple-silicon machine:

| | SHA-256 | SHA-512 |
|---|---|---|
| ring | 0.59-0.61 ms | 0.92-0.93 ms |
| RustCrypto (`sha2` uses the ARM SHA2 instructions via cpufeatures) | 0.44-0.49 ms | 0.77-0.80 ms |

So SCRAM cost is negligible either way. Where it is paid: **the server never runs PBKDF2 to authenticate a user.** It stores
`StoredKey` and `ServerKey` and verifies with two HMACs and one hash (microseconds). PBKDF2 runs on the client for each login, and on the server only
when it turns a plaintext bootstrap password into stored credentials (once at start). The `AlterUserScramCredentials` request
already carries the salted password. PLAIN needs only a constant-time compare (ring's `constant_time`, or `subtle`, which rustls already pulls in). Kafka applies
NFKC-style normalization to SCRAM names/passwords; restricting bootstrap passwords to ASCII or normalizing in the user-file loader avoids a unicode-table dependency (rsasl's `stringprep`).
Not measured: x86_64 PBKDF2 times (SHA-NI vs not).

### Supply chain (`cargo deny` run with the repo's `deny.toml`, `all-features` off, per configuration)

| Candidate | Result |
|---|---|
| ring 0.17.14 (`Apache-2.0 AND ISC`) | licenses, bans, advisories, sources all OK |
| aws-lc-rs / aws-lc-sys 0.47 | OK |
| rustls, tokio-rustls, rustls-webpki, rustls-pki-types | OK |
| rcgen 0.14 and its dependencies (`time`, `yasna`, `pem`, `base64`) | OK |
| hmac, sha2, pbkdf2, subtle, base64 (latest) | OK |
| rsasl 2.3 | OK |
| rustls-rustcrypto 0.1.0-pre.0 | **advisories FAILED** (`paste`, RUSTSEC-2024-0436); a `duplicate syn` warning too |
| rustls-pemfile 2.2 | **advisories FAILED** (RUSTSEC-2025-0134) |
| scram 0.6 | **advisories and licence FAILED** (ring 0.16.20) |

### Projection against the real binary

| Build | darwin/arm64 size | vs 4.2 MB doc limit | Linux x86_64 vs 5.15 MB CI gate | Packages (CI) | Idle RSS |
|---|---|---|---|---|---|
| Today | 4,029,504 B | fits (170 KB headroom) | 4.90 MB measured, fits | 107 | 4.8 MB |
| + SCRAM/PLAIN on ring, always on | 4,063,760 B (+34 KB) | fits (136 KB headroom) | about 4.93 MB, fits | 110 | unchanged (nothing configured) |
| + SCRAM on RustCrypto instead | 4,080,672 B (+51 KB) | fits | fits | **124, fails 115** | unchanged |
| + TLS (ring), always on | about 4,936,000 B (+906,656 B = +22.5%) | **fails** | about 5.8 MB (my estimate; x86_64 not measured), **fails** | **119, fails** | about 4.9 MB unconfigured (+0.1), about 5.5 MB with TLS active (+0.66), one connection +0.3 |
| + TLS with rcgen | about 5,004,000 B | fails | fails | 129 | as above |
| + TLS (aws-lc-rs) | about 6.3 MB | fails | fails | 119 | worse |

### Recommendation

**Stack:** `rustls` 0.23 (`default-features = false`, `std`, `tls12`, `ring`) + `tokio-rustls` 0.26 (`default-features = false`, `tls12`),
PEM via `rustls-pki-types`' built-in `PemObject`, and SCRAM-SHA-256/-512 plus PLAIN written by hand on **ring's** `hmac`, `digest`,
`pbkdf2` and `constant_time`, with a small hand-rolled base64 (about 30 lines, pinned by RFC 7677 and RFC 5802 test vectors).
This is one crypto library for both jobs: when `tls` is on, SCRAM adds no packages and +34 KB.

**Weight cost:** TLS is +872,608 B and +9 distinct packages (+12 by the CI count, 107 to 119) with about +0.7 MB idle RSS when active and about +0.1 MB when compiled in but unused; SCRAM/PLAIN on ring is +34 KB and +3 packages (107 to 110) with no RSS change.

**Does TLS fit?**
- **Always on: no.** It takes the darwin binary from 4.03 to about 4.94 MB (limit 4.2 MB), the Linux build to roughly 5.8 MB (gate 5.15 MB), and packages to 119 (gate 115).
- **As a default-off cargo feature: yes**, and this matches PLAN.md ("Features gate weight: ... `tls`"). The default build then pays only SCRAM/PLAIN: 4,063,760 B, 110 packages.
  CI's package count uses `cargo tree -p doze-kafka` with default features, so the optional dependencies are not counted and the existing gate stays valid without a written exception for TLS.
  The SCRAM +34 KB and +3 packages do need a written reason (ring is the first C-compiled dependency; the build then needs a C compiler where it needed none).
- **If the project wants TLS in the default binary** the limits would have to become 5.2 MB darwin (4,936,160 B + 5%), 6.2 MB Linux x86_64 (estimate; measure first),
  121 packages, 8 MB RSS unchanged. I would not do this: SCRAM-over-plaintext is the normal local mode and TLS users can pass `--features tls`.
- Add a second CI measurement for `cargo build --release -p doze-kafka --features tls` with its own gates (suggested: size at measured + 5%, 121 packages, RSS 8 MB).

**If a C compiler in the default build is unacceptable**, the fallback is RustCrypto `hmac` + `sha2` + `subtle` for SCRAM in the default build (+14 packages, 121, over the gate, so the gate would need to rise to about 122 for SCRAM alone), keeping ring only behind `tls`. I rank that worse than ring-everywhere.
A fully hand-rolled SHA-2/HMAC/PBKDF2 (about 150 lines, zero dependencies, pinned by NIST and RFC vectors) is the only zero-package SCRAM; I have not built it, and I would only choose it if "+3 packages and ring in the default build" is rejected.

**Not recommended:** aws-lc-rs (+2.3 MB), rustls-rustcrypto (pre-release, fails deny on `paste`, 65 extra packages, slowest, +1.03 MB), the `scram` crate (ring 0.16, SHA-256 only, fails deny), `rsasl` (+302 KB), `rustls-pemfile` (unmaintained).
`rcgen` (+68 KB, +9 packages) only if a `--tls-dev` option is built; it pushes the tls build to 129 packages.

### Caveats

- All sizes are darwin/arm64, one toolchain; Linux x86_64 will differ (the repo's own two numbers differ by 0.87 MB). Re-measure the tls feature on the Linux CI job before fixing its limit.
- Handshake and PBKDF2 times are single-machine loopback figures; RSS is `ps` RSS from one run per configuration.
- The scratch echo server is not the broker; marginal RSS in the broker should be smaller or similar since tokio's runtime is already there.

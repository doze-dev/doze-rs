# Fuzzing kafka-auth

One target per parser: `plain` (the SASL/PLAIN token), `scram_first` and `scram_final` (the two messages a SCRAM client sends;
the second also runs the machine and must never log in without the password), `acl_filter` (ACL filters, creations and the decision
over whatever was stored) and `hostguard` (the allowed-hosts guard over any `Host` header, with no list, one listed name and `*`).

```
cd crates/kafka-auth/fuzz
cargo +nightly fuzz run plain corpus/plain seeds/plain -- -max_len=65536
cargo +nightly fuzz run acl_filter corpus/acl_filter seeds/acl_filter -- -max_len=4096
cargo +nightly fuzz run scram_first corpus/scram_first seeds/scram_first -- -max_len=65536
cargo +nightly fuzz run scram_final corpus/scram_final seeds/scram_final -- -max_len=65536
cargo +nightly fuzz run hostguard corpus/hostguard seeds/hostguard -- -max_len=4096
```

The first directory is the fuzzer's own corpus (ignored by git); `seeds/` is read-only input and must come second.
The largest token a stranger can send is the pre-login frame cap, 128 KiB. An ACL request is only read after the
login, so its target is capped at a few KiB: the strings it builds are four characters at most. A `Host` header is a
few hundred bytes in practice (the guard refuses any name over 253), so its target is capped at 4 KiB.

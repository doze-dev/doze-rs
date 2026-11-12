# Fuzzing kafka-auth

One target per parser: `plain` (the SASL/PLAIN token) and `acl_filter` (ACL filters, creations and the decision
over whatever was stored).

```
cd crates/kafka-auth/fuzz
cargo +nightly fuzz run plain corpus/plain seeds/plain -- -max_len=524288
cargo +nightly fuzz run acl_filter corpus/acl_filter seeds/acl_filter -- -max_len=4096
```

The first directory is the fuzzer's own corpus (ignored by git); `seeds/` is read-only input and must come second.
The largest token a stranger can send is the pre-login frame cap, 512 KiB. An ACL request is only read after the
login, so its target is capped at a few KiB: the strings it builds are four characters at most.

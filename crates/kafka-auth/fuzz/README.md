# Fuzzing kafka-auth

One target per parser: `plain` (the SASL/PLAIN token).

```
cd crates/kafka-auth/fuzz
cargo +nightly fuzz run plain corpus/plain seeds/plain -- -max_len=524288
```

The first directory is the fuzzer's own corpus (ignored by git); `seeds/` is read-only input and must come second.
The largest token a stranger can send is the pre-login frame cap, 512 KiB.

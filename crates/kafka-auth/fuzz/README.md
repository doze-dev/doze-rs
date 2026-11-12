# Fuzzing kafka-auth

One target per parser: `plain` (the SASL/PLAIN token), `scram_first` and `scram_final` (the two messages a SCRAM client sends;
the second also runs the machine and must never log in without the password).

```
cd crates/kafka-auth/fuzz
cargo +nightly fuzz run plain corpus/plain seeds/plain -- -max_len=524288
cargo +nightly fuzz run scram_first corpus/scram_first seeds/scram_first -- -max_len=524288
cargo +nightly fuzz run scram_final corpus/scram_final seeds/scram_final -- -max_len=524288
```

The first directory is the fuzzer's own corpus (ignored by git); `seeds/` is read-only input and must come second.
The largest token a stranger can send is the pre-login frame cap, 512 KiB.

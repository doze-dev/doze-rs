# Fuzzing kafka-wire

Three targets, one per parser: `fuzz_frame`, `fuzz_request`, `fuzz_batch`.

```
cd crates/kafka-wire/fuzz
cargo +nightly fuzz run fuzz_request corpus/fuzz_request seeds/fuzz_request -- -max_len=1048576
```

The first directory is the fuzzer's own corpus: libFuzzer writes what it finds there (ignored by git). The second
is `seeds/`, which is read only if it comes after the first. Never pass `seeds/` first, or the fuzzer fills it with
thousands of generated inputs. `seeds/` is written by `cargo test -p kafka-wire --test every_offered_version --
--ignored write_the_seed_corpus`. A large `-max_len` matters: the allocation defects only show on big frames.

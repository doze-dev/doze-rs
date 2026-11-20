# Fuzzing aws-console

One target: `origin`, the proof a write must carry (method, `Origin`, `Sec-Fetch-Site`, `Host`), over any text.

```
cd crates/aws-console/fuzz
cargo +nightly fuzz run origin corpus/origin seeds/origin -- -max_len=4096
```

The first directory is the fuzzer's own corpus (ignored by git); `seeds/` is read-only input and must come second. Input is four
lines: method, Origin, Sec-Fetch-Site, Host; an empty line is a header that was not sent.

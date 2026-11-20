# Fuzzing aws-console

Two targets:

- `origin`, the proof a write must carry (method, `Origin`, `Sec-Fetch-Site`, `Host`), over any text. Input is four lines:
  method, Origin, Sec-Fetch-Site, Host; an empty line is a header that was not sent.
- `user_agent`, the label kept from a request's `User-Agent` (`agent::label`): never a panic, never more than `MOST` bytes,
  only printable ASCII.

```
cd crates/aws-console/fuzz
cargo +nightly fuzz run origin corpus/origin seeds/origin -- -max_len=4096
cargo +nightly fuzz run user_agent corpus/user_agent seeds/user_agent -- -max_len=4096
```

The first directory is the fuzzer's own corpus (ignored by git); `seeds/` is read-only input and must come second.

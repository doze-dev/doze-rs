# Fuzzing aws-console

Four targets:

- `origin`, the proof a write must carry (method, `Origin`, `Sec-Fetch-Site`, `Host`), over any text. Input is four lines:
  method, Origin, Sec-Fetch-Site, Host; an empty line is a header that was not sent.
- `user_agent`, the label kept from a request's `User-Agent` (`agent::label`): never a panic, never more than `MOST` bytes,
  only printable ASCII.
- `lab_glob`, the pattern language of a Lab rule's resource list (`lab::glob::fits`): any pattern against any name (the first line
  is the pattern, the rest the name) must agree with a plain table-driven match, and never panic.
- `lab_body`, the body of a Lab rule (`model::LabRuleBody`) read as JSON and its words made into a matcher (`lab::Matcher::build`):
  never a panic, and no list is kept longer than it was given.

```
cd crates/aws-console/fuzz
cargo +nightly fuzz run origin corpus/origin seeds/origin -- -max_len=4096
cargo +nightly fuzz run user_agent corpus/user_agent seeds/user_agent -- -max_len=4096
cargo +nightly fuzz run lab_glob corpus/lab_glob seeds/lab_glob -- -max_len=512
cargo +nightly fuzz run lab_body corpus/lab_body seeds/lab_body -- -max_len=4096
```

The first directory is the fuzzer's own corpus (ignored by git); `seeds/` is read-only input and must come second.

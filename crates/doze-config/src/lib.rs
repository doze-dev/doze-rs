//! doze.hcl (core plan phase H4): doze's schema over doze-hcl. It reads a stack's files the way
//! Go's `internal/config` does and gives the engine its typed `Stack`, or the diagnostics Go gives,
//! at Go's positions. Nothing here is built yet beyond the oracle corpus in `testdata/oracle`
//! (see `testdata/tools/README.md`).

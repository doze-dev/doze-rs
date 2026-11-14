# Step 2: `doze-module`, the SDK modules are written against

Status: built 2026-11-12: `doze-module` + `doze-module-derive`; the echo module passes 5 tests as a real process (CI: five targets). Plan: PLAN.md D1, D2, D4, step 2. Protocol: `protocol.md`.

## What a module looks like

```rust
#[derive(Spec)]
struct ValkeySpec {
    #[spec(doc = "Require clients to authenticate with this password.")]
    password: Option<String>,
    #[spec(doc = "Evict keys over this size, e.g. \"256mb\".")]
    maxmemory: Option<String>,
    #[spec(default = "false", doc = "Append-only persistence.")]
    appendonly: bool,
    #[spec(doc = "Any other valkey setting, passed through.")]
    settings: BTreeMap<String, String>,
}

struct Valkey;

impl Module for Valkey {
    type Spec = ValkeySpec;
    fn about(&self) -> About { … }                                   // static: engine, title, versions, port …
    async fn resolve(&self, cx: &Cx<'_, ValkeySpec>, fetch: &Fetch) -> Result<Resolved> { … }
    async fn provision(&self, cx: &Cx<'_, ValkeySpec>, tc: &Toolchain) -> Result<Provisioned> { … }
    async fn plan(&self, cx: &Cx<'_, ValkeySpec>, tc: &Toolchain) -> Result<Plan> { … }
    fn connection(&self, cx: &Cx<'_, ValkeySpec>) -> Result<Connection> { … }
}

fn main() -> std::process::ExitCode {
    doze_module::serve(Valkey).run()                                 // .converge() .admin() … for capabilities
}
```

## Decisions

- **`#[derive(Spec)]` is the single source** of the schema (sent in the manifest, validated by the core, rendered by
  the registry) and of decoding (typed fields from the core's `Value` tree, with a diagnostic at the field's path for
  anything wrong). They cannot drift. Supported: `String`, `bool`, integers, `f64`, `Option<T>`, `Vec<T>`,
  `BTreeMap<String, T>`, nested `#[spec(block)]` structs (repeated: `Vec<…>`, labelled: a `#[spec(label)]` field).
  Field attributes: `doc`, `default`, `since`, `until`, `rename`. `Module::check` adds semantic rules a schema cannot
  state, as diagnostics.
- **Block encoding in the `Value` tree** (the core's side of the contract): an argument is a map entry; a nested block
  type is a map entry holding a list of maps, one per block; a block's labels are entries of its map named after the
  schema's label names.
- **Capabilities are traits** (`Converge`, `Objects`, `Prune`, `Admin`, `Console`, `Template`, `Attributes`,
  `BackendUrl`, `Health`, `Restart`, `Wire`). Registering one on the server requires the trait, and the manifest's
  capability bits come from what was registered: a module cannot advertise what it does not implement.
- **Handlers get a context, `Cx`**, holding the decoded spec, the instance, the call's cancel signal and id, and the
  module's log and progress. It owns the protocol call while the handler runs (protocol.md: a handler holding only the
  cancel signal dropped its responder). Calls are stateless (D2): the spec is decoded from the instance on every call.
- **`Fetch` is the host's download service**, usable only during `resolve` (it carries the call's id).
- **Helpers every Go module repeated**: `resolve_via` (lock pin, exact version, major through the host), exactness by
  dots, settings pass-through with locked keys, a tool runner (no shell, null stdin, error = trimmed stderr), stale
  socket and pid file clearing, template ensure and clone (`doze-os::fs::clone_dir`).
- `serve().run()` takes the channel from `doze-os`, says hello, serves until the host goes, and exits.

## Files

```
crates/doze-module-derive/   proc macro: #[derive(Spec)]
crates/doze-module/
  src/lib.rs        re-exports, serve()
  src/spec.rs       Spec trait, decoding primitives with paths, Diagnostics
  src/module.rs     Module trait, About, Cx
  src/caps.rs       capability traits
  src/server.rs     the builder, dispatch of calls to traits, hello
  src/fetch.rs      Fetch
  src/help/         resolve_via, settings, run (tools), stale files, template
  tests/echo.rs     the echo module (tests/echo/main.rs as a test binary) driven by a host peer
```

## Done when

The echo module, started as a real process through `doze-os`, passes: hello and manifest (schema from the derive),
`decode` with a diagnostic at the right path, `network`, `connection`, `resolve` calling the host's `fetch.ensure`,
`provision`, `plan`; green on five targets.

## The module contract

Owner rule (2026-11-14): **nothing module-specific is hard-coded in doze**, in the app or the CLI. What doze knows
about an engine comes from its module, through the SDK and protocol 2 (`Manifest` in
`doze-module-proto/src/msg/hello.rs`), and reaches the core and the app through the registry (`meta.yaml`, written
by `doze-module-release`, read by `doze_cli::registry::Cache::hints`). Where a module declares nothing, a generic
default applies; doze never fills the gap with its own table.

The fields, and the default when a module leaves one out:

| Field | What it is for | Default |
|---|---|---|
| `client` (`ClientCommand`) | the shell on an instance: program, argv template over `{host}` `{port}` `{url}` `{password}`, env template (secrets go here, never argv), fallbacks on `PATH`. Feeds `doze connect` and the app's "open shell" | no shell is offered (`connect` exits 1 and points at `doze url`) |
| `detect` (`Detect`) | how `doze init`/`add` and the app's empty folder recognise a project that uses the engine: packages by ecosystem (`go`, `npm`, …), container images, env variables | the module is never guessed |
| `default_version` | the `version` `doze add` writes | the newest engine the module's index entry supports |
| `example_label` | the label a new block gets (`module valkey "cache"`) | the module's name |

Only modules customise their page in the app, through their declared console spec, its extensions and the manifest
fields above. Apps, tasks and outside services (`app`, `task`, `service` blocks) always get the fixed standard pages:
there is no declaration mechanism for them in the protocol, the CLI or doze.hcl, and none is to be added.

Keep it lean: doze is for debugging and performance. A new module-facing field is added only when several modules
genuinely need it; the change that adds one carries one line saying which modules and why, for debugging or
performance. A field that fits one tool belongs in that module (its console, its actions), not in the contract.

Declared today: valkey and kvrocks (`client`, `detect`), temporal (`detect`; it has no shell, its CLI is not a
session). kafka and aws declare theirs when their module faces are built (core plan M3); postgres, mariadb and ferret
when they are ported (M1). The app's empty-folder table (`doze-desk` `act/detect`) takes the union of the modules'
`detect`, with nothing built in.

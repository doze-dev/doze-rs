//! The examples each command's `--help` ends with (as gh, cargo and uv do): the common uses first, each a line a person
//! can copy. Kept here so the command list in `cli.rs` stays a list.

pub(crate) const STATUS: &str = "Examples:
  doze status                every service, its state and address
  doze status -w             redraw as things change (Ctrl-C to stop)
  doze status --graph        what each service needs
  doze status --json | jq '.data.services[].name'";

pub(crate) const WAKE: &str = "Examples:
  doze wake                  everything doze.hcl does not turn off
  doze wake api              api, with what it needs first
  doze wake api --dry-run    what would start, starting nothing";

pub(crate) const SLEEP: &str = "Examples:
  doze sleep                 everything awake (doze keeps running)
  doze sleep db cache        these two; their data stays";

pub(crate) const RESTART: &str = "Examples:
  doze restart api           stop api and start it again
  doze restart api worker --dry-run";

pub(crate) const TASK: &str = "Examples:
  doze task                  the tasks and how each last ran
  doze task migrate          run migrate now (its output follows)";

pub(crate) const PIN: &str = "Examples:
  doze pin db                keep db awake past its idle time
  doze pin db --off          let it sleep again when idle
  doze pin db --toggle       flip it, as the app's switch does";

pub(crate) const LOGS: &str = "Examples:
  doze logs api              api's newest run, its last 200 lines
  doze logs -f               every service, merged, as lines come
  doze logs api --run -1     the run before (after a crash)
  doze logs api -n 50 --since 10m";

pub(crate) const WHY: &str = "Examples:
  doze why db                what woke db, what keeps it, when it sleeps";

pub(crate) const DOCTOR: &str = "Examples:
  doze doctor                what would stop doze here, and the fix for each
  doze doctor --fix          set up what is missing, one password
  doze doctor --fix --dry-run    the script it would run, running nothing
  doze --json doctor         the checks for a script or the app";

pub(crate) const DNS_SETUP: &str = "Examples:
  doze dns-setup --check     what is set up, without sudo
  doze dns-setup --dry-run   the script, to read or run yourself
  doze dns-setup             set it up (one password)
  doze dns-setup --uninstall";

pub(crate) const TRUST: &str = "Examples:
  doze trust --check         the stack's domains: routed? https trusted?
  doze trust                 route them here and trust https (one password)
  doze trust --remove        undo it for this stack
  doze trust --rotate        replace the certificate authority";

pub(crate) const LINT: &str = "Examples:
  doze lint                  both passes; exit 11 if a block went unchecked
  doze lint --offline        only what needs no module (CI without a cache)
  doze --json lint | jq '.data.unchecked'";

pub(crate) const FMT: &str = "Examples:
  doze fmt                   rewrite the files in HCL's layout
  doze fmt --check           list what would change; exit 1 if anything would (CI)
  doze fmt --dry-run         the change itself, as a diff";

pub(crate) const COMPLETION: &str = "Examples:
  source <(doze completion bash)                     in ~/.bashrc
  source <(doze completion zsh)                      in ~/.zshrc
  doze completion fish | source                      in ~/.config/fish/config.fish
  doze completion powershell | Out-String | Invoke-Expression
Service names complete from the stack's doze.hcl as you type.";

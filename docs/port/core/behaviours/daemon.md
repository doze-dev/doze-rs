# D1: the daemon (doze-daemon) — behaviours and protocol notes

What the Rust daemon does against Go's `internal/daemon`, `internal/daemonctl` and `internal/control/server.go`.

## Starting and stopping (`ctl`, Go's `daemonctl`)

| | Go | Rust | Why |
|---|---|---|---|
| Start | `doze __daemon --config <abs>`, `Setsid`, stdout and stderr appended to `run/doze.log`, `--var`s as `DOZE_VAR_<name>`; waits up to 10 s for the socket, quoting the log's last 15 lines on failure | the same (`ctl::start`) | |
| Zombie | the started process is released; a long-lived starter (the Go app) kept a zombie until it exited | a thread of the starter reaps it (`doze_os::detach::spawn_detached`) | the app starts stacks and lives for days |
| Pid file | `run/doze.pid`: the pid and a newline, written by the daemon | the same, written once the socket is up | |
| Stop | SIGTERM to the pid, wait 20 s | the same; on Windows, where another console's process cannot be sent CTRL_BREAK, it is ended (its Job objects end its engines) | |
| Stack lock | none | `run/doze.ctl.lock`, an advisory lock (`ctl::StackLock`) `up` and `down` hold while they start or stop the daemon; the OS drops it when its holder dies | two `up`s must not start two daemons for one stack |
| A second daemon | a second bind fails | `__daemon` refuses when a daemon already answers on the socket | |

## `__daemon` (`entry`)

doze.hcl through doze-config (this process's environment, `DOZE_VAR_*` included; doze-names' domain checks), `to_stack` (apps, services and tasks, an `after` on a task a need), the engine, the server on `<data_dir>/run/doze.admin.sock`; SIGTERM or SIGINT stops the server, then the engine stops everything it started, then the pid file goes.

Interim, until the module catalog over the pool lands (D1's next step): every module type is accepted with what every module has, and a module instance says when woken that this daemon does not run modules yet. Certificates are checked for presence only.

## Protocol 2: additive fields (Rust daemon only)

Go's clients ignore fields they do not know, so these leave every field Go defines byte-identical.

- `InstanceView.env` (object, omitted when empty): the variables a client sets to reach the instance (`DATABASE_URL` and the rest), as the instance's module answers them (its attributes through doze-module-host). Never computed by module-specific code in the daemon.
- `InstanceView.client` (string, omitted when empty): the client program the module declares, for `doze connect`.

Both are filled from the module's own answer once the catalog lands; apps, services and tasks carry none.

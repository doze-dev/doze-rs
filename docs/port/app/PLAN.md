# The doze desktop app in Rust (GPUI): build plan

Status: proposed 2026-11-13, not approved. Built so far (branch `app-shell`): M0 steps 0.1 to 0.7 (0.2–0.5 on their
own branches, 0.1, 0.6 and 0.7 here) and the Map (step 1.3).

M2 steps 2.1, 2.2, 2.5 and 2.6 (branch `app-m2-act`): `doze-desk::act` (dialogs held to Go's goldens, notices, stack
actions through `doze --json`, the checks before Up and before a wake, the start sheet, the empty folder with a reduced
detector) and `apps/doze-app/src/act` with its overlays. Differences: the overview's ⋯ opens ⌘K (its stack commands are
palette rows) rather than a menu of its own; Remove does not forget the stack's domain routes (that is the names daemon's,
waiting for its Rust port); the domain check reads `doze --json doctor` and its fix copies `doze trust` (the password
prompt is M3's). In the combined branch a failed start is the page's banner and Needs you's alert, as Go's (its line is
kept for Recent, with no toast of its own).

M2 combined (branch `app-m2`, superseding `app-m2-act` and `app-m2-attention` for review): steps 2.3, 2.4 and 2.7
(⌘K, Needs you, alerts, the away strip, the keys and the menu bar) wired to the verbs. ⌘K's and Needs you's `Verb` and
`StackVerb` go to acting the way the pages' buttons do (dialogs where Go asks, the checks before Up, down and remove ask,
`holder:PID` looks again that the pid still holds the port the crash names, then restarts the service); a failed stack
action is a Needs you item and a stack mid-action carries its items (one `Failure` type for both). One action table
feeds the help sheet, the chips, the menu bar and ⌘K, each command listed once. Beyond Go: ⌘K's ⌘⏎ on a service in
trouble restarts it (Needs you's first fix). Still M3: Settings, Activity, Check for Updates, Settings › Machine's
repair and clean-up (their rows say "soon").

M3 steps 3.1, 3.2 and 3.5 (branch `app-m3-tray`): `doze-desk::tray` (Go's `Build`, `Split`, `NeedsOf` with their
tests, and the whole menu as data with a signature and a text form) and `doze-desk::notify` (Go's D8 rules with their
tests, the fix pressed outside the window, the notification's words); the app's `around` keeps one Needs you list for
the menu bar and notifications (the window's while it is open, its own while it is closed) and opens the window when
something pressed from the menu bar asks; `cargo xtask app-bundle` makes Doze.app; `doze-app open` hands over to a
running app or starts one on its own. Differences and beyond Go:
- each running stack's submenu also lists its services (wake, sleep, turn on, restart or run, as fits; their page and
  logs) and All logs; the stack line leaves the restart countdown out, so a crash loop does not rebuild the menu every
  second; menu lines carry the menu's generation, so a pick that arrives after a rebuild is dropped;
- a note waits up to 2 s for Needs you to list its item, so the notification carries the fix button (Go posted at
  once and often had none); its button presses the fix only while the item still offers it, and for a port's holder
  only while the same pid holds the port (as Go); without a bundle a note is a toast and a Recent line, and stderr;
- the Dock badge is Needs you's red count; Settings… and Check for Updates… go to `doze://settings` (step 3.4 builds
  that page); the machine's items in the menu while the window is closed are the window's last (doctor runs only
  with a window);
- Q4 as built: the status item, its submenus and the Dock badge are objc2 in `doze-os::ui`; notifications need no
  `unsafe` of ours (gpui's own `UNUserNotificationCenter` path). Q2 as built: the app is its own binary in the bundle;
  Go's `doze open` handing off to the Rust bundle is a Go-side change not made here (`doze-app open` does the same
  hand-off, and `$DOZE_APP_BUNDLE` names the bundle it starts).

Where the M0 build differs from this plan:

- The app lives at `apps/doze-app`, not `crates/doze-app`, so `default-members = ["crates/*", …]` leaves it out by
  glob. `--workspace` ignores default members, so the check, doc and Windows jobs and `xtask gates` pass
  `--exclude doze-app`; the app has its own macOS CI job (`app`). `lint-size` scans `apps/` too.
- `doze://` URL events come from GPUI (`Application::on_open_urls`), so `doze-os::ui` holds only the status item and
  the Dock badge. The link parser is in the app (`src/link.rs`) until `doze-desk` grows `link`; it moves there with a
  fuzz target.
- The tray follows the hub while the window is hidden through doze-desk's `Policy::hidden` (one batch a second); the
  window is dropped on close, so it hears nothing then.
- The theme's xtask reads a copy of the Go app's `tokens.css` kept in `apps/doze-app/themes/` (CI has no Go checkout);
  `cargo xtask themes --check` holds the JSON to it.
- doze-map gained `Layout::overview` (regions and weighted bundles) for the whole-map view.
- Measurements against §6 are in `docs/port/weight.md`, "The desktop app".
- M1 steps 1.1 (Library), 1.2 (overview: head, strip, Tiles, List and pane, density, the stopped page) and 1.4 (Grid)
  are built on branch `app-m1-overview`, read-only (a switch or a verb says it comes with M2). Their view models are
  in `doze-desk` (`library`, `overview`), held field for field to Go's view JSON (`crates/doze-desk/tests/pages`):
  Go's own goldens, and what Go's app served for the shop and larder fixtures on a stopped clock.
  - A stopped stack's shape is `doze --json status` filled in from the file itself (`overview::refs`, a reader of
    doze.hcl's blocks and references, fuzzed), so the rows, the Map and the Grid of a stopped or offline stack show
    even when the CLI cannot load the file. The Grid's quotes come from the same reader (Go: `RefsIn`).
  - The machine line is `doze --json doctor` run in a stack's folder (doctor reads the names setting from a config);
    other programs' names are read from `names.json`, never written.
  - A dozing row counts down ("sleeps in 3:12") where Go said "quiet; sleeps soon": Go's view snapshot dropped the
    idle time.
  - Not yet: the running stack's ConfigBanner (needs `doze lint --json`), the domain set-up banner and the Library's
    "unready" https pill (need the trust check), the busy and failed banners (M2's actions), the Grid's sticky first
    column.

The owner's brief: "make the GUI as best as you can make it! Leave no stone
unturned! It's got to squeeze the best out of GPUI and its components. Check
what the Go doze GUI offers today and just make it the best god damn doze GUI
you can think of."

This plan covers four things:

- what the Go app does today, every feature, with the file it lives in;
- how each feature is rebuilt in GPUI and gpui-component;
- what GPUI makes possible beyond the Go app;
- the crates, the budgets, the order of work with day estimates, and the open questions.

## Sources read

- **Go app.** Read from `~/Code/doze-dev/doze` at `origin/main` `23b73d6`:
  - Go side: `internal/desk/{app,autostart,devserve,hub,link,notify,prefs,serve,tray,view,web}`.
  - React side: `internal/desk/frontend/src/**`.
  - Supporting code: `cmd/doze/{open,face}.go`, `internal/control`.
  - Design docs: `docs/design/desktop/**` (README D1–D19, `parity.md`, d18, d19, the audits, 36 boards) and `docs/design/next/daemon-api.md`.
- **GPUI spike.** `~/Code/doze-dev/doze-gpui-spike`: about 2,700 lines of Rust, using `gpui-pre` 0.3.12 and `gpui-component` 0.9.1.
- **Component library sources.** The registry sources of gpui-component 0.9.1, gpui-base 0.9.1 and gpui-pre 0.3.12.
- **doze-rs.** Read at `origin/main` `996c30b5`:
  - `README.md`, `docs/port/**`, `modules/{PLAN,compat,protocol}.md`, `names/PLAN.md`, `weight.md`;
  - the `cli-design` branch's `docs/port/cli/proposal.md`.
- **Memory notes.** The desktop app and GPUI spike notes, and the owner's bar for the console experience: overlays, not pages; gentle motion; "stunning". That last note also says the boards are the floor, not the ceiling.

## 1. What exists today

### 1.1 The Go app (Wails v3 + React)

This is one binary that is the CLI, the daemon and the app, and it has one window.

**Shell and data flow**
- The window is a webview over an embedded React build.
- Go owns every fact (D16): `internal/desk/view` has 22 files, about 7,000 lines, and computes typed JSON view models.
- `serve.Service` exposes 48 bound methods and pushes 10 kinds of event, paced at 250 ms and hash-deduplicated.

**Data sources**
- `hub` keeps one control-socket subscription per running stack.
- It finds stacks by globbing `$DOZE_HOME/projects/*/run/doze.admin.sock` every 2 s.
- Stopped stacks come from the project records in `projects/*/project.json`.

**Daemon protocol**
- Control protocol 2 (`internal/control`): NDJSON over a unix socket.
- Each side sends a hello first: `{"hello":{"proto":2,"client":…}}`.
- Requests are multiplexed by `id`. A request is cancelled with `{op:"cancel",id}`.

**What the hub uses**

| Use | Op |
|---|---|
| Snapshot | `status` |
| Live state | one `subscribe` per stack on `instances` + `journal`, plus `stats` and `logs` (backlog 400) while a page watches them |
| Module console | a separate `subscribe` on `console` per watched instance, with `panel` and `selector` |
| Verbs | up, wake, sleep, restart, reset, task, pin, off, on, apply |
| Module actions | `admin` |

- Stack up, down and restart do not go over the socket. They run the binary itself as a subprocess: `doze -y up --last|--all|<names>`, `doze -y down`.
- Ring sizes: 240 stat samples (4 minutes), 400 log lines per instance, 800 merged, 300 journal events, 300 console stream rows.

**Measured** (deskbench, macOS)
- Menu bar only: 77–81 ms ready, 23 MB.
- Window: 145 ms to paint home, 111–121 MB.
- After close: 44–49 MB.
- CPU at rest: 0.08–0.13 %, over a budget of 0.10 %.
- Budgets (`test/deskbench/budget.json`): tray 150 ms / 25 MB, window 200 ms / 240 MB, after close 50 MB, CPU 0.1 %.

### 1.2 The GPUI spike (`~/Code/doze-dev/doze-gpui-spike`)

It is a separate crate and was never committed. The map code is being changed by another agent; this plan does not touch it.

**What the spike has**

| File | Lines | What it does |
|---|---|---|
| `app.rs` | 243 | Root view, actions `TogglePalette ClosePalette NextRow PrevRow NextCol PrevCol`, key bindings, theme boot |
| `page.rs` | 632 | Title bar with tabs, head with the segmented view switch, banner, vitals strip, the List view with rows and switches, the filter, the details pane, the keys bar |
| `palette.rs` | 193 | The ⌘K overlay over a dimmed backdrop: fuzzy-marked labels, sections, footer keys, step navigation |
| `map.rs` | 518 | The Map painted from Go's `MapView` (SVG paths parsed into PathBuilder, dashes, arrowheads, fit-zoom), the detail card, arrow-key `next_card` (matches flow.tsx on 96 moves) |
| `widgets.rs` | 154 | tracked text (letter-spacing emulation), dot, ring, switch, kbd chip, buttons, copy link, tag, segmented control |
| `theme.rs` | 84 | violet-dark tokens only |
| `model.rs` | 316 | serde view models matching the Go JSON |
| `daemon.rs` | 285 | Control socket client: hello, then polls `status` once a second on a thread and sends over `async_channel`. Maps status to a small subset of `internal/display` (look, because, vitals) |
| `tray.rs` | 88 | objc2 `NSStatusItem`, with "Open doze" and "Quit (stacks keep running)" |
| `shot.rs` | 50 | Offscreen `Window::render_to_image()` to PNG (feature `capture` = gpui `test-support`) |
| `tools/*.ts` | — | compare, overlay and rules diff against the React page's PNGs |

**Fidelity against the React page** (mean diff out of 255)
- list 2.86, palette 2.20, larder map 2.72, shop map 2.51.
- Most elements are within 1–3 device px.

**Measured** (M5 Max)
- Menu bar only: 11 MB bare, 16 MB with gpui-component, fonts and theme.
- Window idle: about 73 MB at 0 % CPU.
- Redrawing at 10/s: 216 MB peak. This is the OS compositor floor: a bare Metal view costs 168 MB.
- After close: 33 MB.
- Binary: 12 MB with gpui-component and 8 font faces.

**Not in the spike**
- Tiles, Grid, Library, the light theme, every other page and overlay.
- Subscriptions (it only polls).
- Verbs, notifications, Linux.

### 1.3 doze-rs today

doze-rs has no CLI, daemon, control socket, display port, project registry or app (README: "planned, not in this repository yet").

**What exists**
- The kafka and aws engines.
- The module SDK and protocol 2: CBOR over fd 3, which is not the control protocol.
- `doze-os`, the only crate allowed `unsafe`.
- `doze-names`: N0–N3 done, N4 partial, N5 setup/trust not started.
- The CLI proposal on `cli-design`, which says: "the terminal is a client of the daemon, as the app is. Same actions, same words".

**Workspace rules that bind the app**
- `unsafe_code = forbid`.
- No `unwrap`, `expect`, indexing or `panic` outside tests.
- Files at most 450 lines (soft limit 300).
- Functions at most 70 lines.
- Weight limits in `xtask/weight-limits.txt`, raised only with a written reason.

**The `map` branch** exists only locally, in `~/Code/doze-dev/doze-rs-map`. As of this writing it adds nothing over `origin/main`; another agent is starting the map crate there.

### 1.4 What gpui-component 0.9.1 gives us

All components are always compiled in; the crate has no `default` feature. The only optional pieces are highlighting (`tree-sitter`, `syntect`), `speech` and `decimal`. gpui-component is now a styled layer over `gpui-base`, which holds the behaviour.

| Component | Use in doze |
|---|---|
| `table::DataTable` + `TableDelegate` (virtualised, sort, resize, column move, context menu, `load_more`) | the Library stack table, the List view, the Runs table, console `table` nodes, the Leftovers table, the Machine doctor table |
| `list::List` + `ListDelegate`; `searchable_list` | sidebar lists, Recent, Needs-you items, the StartSheet ticks |
| `virtual_list` (variable height) and gpui `uniform_list` / `list` | **the log viewer** (uniform rows when wrapping is off, variable rows when it is on) and console streams |
| `command` (`Command`, `CommandState`, groups, key hints) | a candidate for ⌘K. The spike's own palette matches the board; build on `command` only where it helps (§4.3) |
| `kbd` (`Kbd::binding_for_action`) | every shortcut chip is drawn from the live keymap, so a chip never disagrees with a binding |
| `dialog` (+ `DialogEntrance`), `sheet` (any side), `popover`, `hover_card`, `tooltip` | the confirm dialogs, the inspector drawer, the StartSheet and PreflightSheet, the Map detail card, hover-for-why |
| `notification` (`NotificationList`, `InAppAndSystem`) | toasts that act, and system notifications from inside the bundle |
| `menu` (`PopupMenu`, `ContextMenuExt`, `DropdownMenu`), `native_menu` (OS-drawn, can extend past the window) | the ⋯ stack menu, right-click on every row, card and tile, and the app menus |
| `chart` (Line, Area, Bar, Pie, Candlestick, Radar, Sankey) and `plot` (scales, axis, `CrossLine` hover, `PlotAppear`) | vitals charts with hover, the memory share bar, Activity timelines, run-duration bars. A Sankey of who keeps what awake is a "beyond" candidate |
| `resizable` (`h_resizable`, `v_resizable`) | the List and pane split, the logs/console split, the sidebar width |
| `dock` (DockArea, TabGroup, PaneTree, Panel trait) | not the shell; it is too IDE-like for doze. Possibly for the pop-out log/console window (§3) |
| `tab`, `sidebar`, `title_bar`, `status_bar`, `toolbar`, `breadcrumb` | the stack tabs, the instance/logs sidebar, the client-drawn title bar, the keys bar, the palette crumbs |
| `setting` (Settings, SettingPage, SettingGroup, typed `SettingField<T>`) | the Settings page (General / Machine / About) |
| `input` (`Input`, `InputState`, `Textarea`); `input::Editor` (rope, gutter, diagnostics); `highlighter` (with `syntect` or `tree-sitter`) | filters, the palette box, the dialog input and typed name; a **read-only doze.hcl viewer** with the error line marked |
| `diff` (virtualised patch viewer) | "doze.hcl changed": show what changed before the restart |
| `text::TextView` (Markdown) | module notes, the Help sheet glossary, release notes in About |
| `switch`, `checkbox`, `radio`, `select`, `combobox`, `button`/`ButtonGroup`/`Toggle`, `slider` | the row switch, the Show list, density, the segmented views |
| `badge`, `tag`, `progress`, `skeleton`, `spinner`, `shimmer`, `empty`, `alert`, `clipboard` | the bell count, chips, the dozing countdown, loading states, empty states, banners, copy buttons |
| `theme` (`ThemeRegistry` JSON themes, `watch_dir` hot reload, `MotionTokens` fast 120 / normal 180 / slow 280 ms, `apply_system_reduce_motion`) | the 5 palettes × light/dark, user themes, and motion tokens that match the bar |
| `gpui_base::motion` (`Presence`, `MotionReveal`, `Stagger`, `NavStack`/`NavMotion`) | gentle page transitions, overlay enter/exit, staggered tile reveal on first paint |

Built into gpui-pre itself:
- `set_menus` and `set_dock_menu`, keymaps with contexts, multiple windows.
- `on_drop::<ExternalPaths>` for files dropped from Finder.
- AccessKit accessibility.
- `show_system_notification` with actions (UNUserNotificationCenter; it only works inside a `.app`).
- `reduce_motion()`, springs, `with_animation`.
- `render_to_image` behind `test-support`.

**Not available anywhere:** a tray/status item, a WebView, letter-spacing, baseline alignment.

## 2. Feature parity table

Every row is a feature of the Go app on `origin/main`. Abbreviations:
- `fe/` = `internal/desk/frontend/src/`
- `v/` = `internal/desk/view/`
- `s/` = `internal/desk/serve/`
- `a/` = `internal/desk/app/`

Spike status values:
- **built**: rendered and compared with the reference page.
- **partial**: some of the feature, or fixture data only.
- **—**: not started.

The "Needs" column says what the feature depends on:
- **G**: the Go daemon's control protocol (works now).
- **C**: a Go CLI subprocess with a machine contract (`--json`) (works now).
- **R**: waits for the Rust core.
- **L**: local files and OS only.

### 2.1 Shell, windows, menus

| # | Go feature (where) | Rust/GPUI approach | Component | Needs | Spike |
|---|---|---|---|---|---|
| 1 | Menu-bar item, bat icon, alert icon while red > 0 (`a/tray.go`, `a/icons/`) | `NSStatusItem` through objc2 in `doze-os::ui` (the one `unsafe` crate); template images; a dot variant per appearance | objc2 | L | partial (title "z", 2 items) |
| 2 | Per-stack lines: ● running / ○ stopped / ⚠︎ trouble, Sub words, order rules, 8 running + 5 stopped then "More stacks · N" (`tray/tray.go`) | the pure `TrayModel` in `doze-desk` (ported with tests); `NSMenu` rebuilt only when the signature changes | — | G+L | — |
| 3 | Stack submenu: Open in doze, Open <url>, Sleep everything, Restart, Down; stopped: Up as last time, Up everything with preflight (`a/tray.go`) | the same, sent over a channel to the app | — | G+C | — |
| 4 | Needs-you section: header, up to 3 red items each with Detail, fix and Show, and the "N worth doing" line (`tray/needs.go`) | the same, from the `Attention` model | — | G+L | — |
| 5 | Open doze ⌘⇧D, Settings… ⌘,, Check for Updates…, Quit "stacks keep running" (`a/tray.go`) | the same | — | L | partial (Open, Quit) |
| 6 | Linux: a primary click opens the window | a StatusNotifierItem via `ksni` on Linux (that crate's own D-Bus code, no `unsafe` of ours) | ksni | L | — |
| 7 | Native app menu: doze/File/Edit/Go/View/Window/Help with 10 accelerators (`a/menu.go`) | `cx.set_menus` generated from the same action table as ⌘K and the keymap | gpui menus | L | — |
| 8 | Activation policy Accessory ↔ Regular when the window opens and closes (`a/activate_darwin.go`) | `cx.set_activation_policy` (built into gpui) | gpui | L | built |
| 9 | One window, created on demand and **destroyed on close**; streams stop (`a/window.go`) | the same: drop the root view and the subscriptions on close (`QuitMode::Explicit`) | gpui | L | built |
| 10 | First open at the screen's work area; frame remembered (`a/window.go`, `prefs`) | `WindowBounds` from the display's visible bounds; frame saved on close | gpui | L | — |
| 11 | Single instance; second launch forwards folder, `--show` and `doze://` (`a/app.go`) | lock + `~/.doze/desk/app.sock`; the second launch sends its args and exits | std | L | — |
| 12 | `doze://` links: stack, stack/service, open?dir&show, library, settings (`link/link.go`) | `doze-desk::link` parser; macOS URL events via an `application:openURLs:` handler in `doze-os::ui` | objc2 | L | — |
| 13 | File drop on the window opens that folder (`a/window.go`) | `on_drop::<ExternalPaths>` on the root; also on the Dock icon and the status item (beyond) | gpui | L | built (M2) |
| 14 | Open at login: LaunchAgent plist / XDG autostart (`autostart/`) | port as `doze-desk::autostart` (file writes only) | — | L | — |
| 15 | Title bar: Home tab ⌘0, stack tabs with dot, badge and close ×, pending tab, bell, `?`, ⌘K button, double-click to zoom (`fe/components/titlebar.tsx`) | client-drawn title bar with traffic lights inset; tabs from a tab model | `title_bar`, `tab` | G | built (visual) |
| 16 | Sidebar on instance/logs pages: ‹ Overview, grouped instances, Show checkboxes on All logs (`fe/components/sidebar.tsx`) | `sidebar` with groups; checkboxes | `sidebar`, `checkbox` | G | — |
| 17 | Keys bar per page (`fe/components/keys.tsx`) | `status_bar`; chips from `Kbd::binding_for_action` | `status_bar`, `kbd` | — | built |
| 18 | Global banners: daemon not answering, call failed (`fe/app.tsx`) | `alert` at the top, page faded under it | `alert` | G | built (banner) |
| 19 | Back/forward history, 50 entries, ⌘[ ⌘] (`fe/store.ts`) | `Nav` stack in the app model; `NavStack` motion | `gpui_base::motion` | — | — |
| 20 | Reopen the page I had (`doze.reopen`, `doze.last`) | prefs | — | L | — |

### 2.2 Library (front door)

| # | Go feature | Approach | Component | Needs | Spike |
|---|---|---|---|---|---|
| 21 | Brand head, tagline, counts; Open folder ⌘O, Settings ⌘, (`fe/pages/library.tsx`) | page | — | L | — |
| 22 | Getting-started steps strip, "Got it ×" (`v/library.go` steps, prefs `hints`) | the same | — | L | — |
| 23 | Empty library: drop card, start-empty card with an example doze.hcl, "New stack… ⌘N" (Go has no handler for ⌘N) | the same, with ⌘N **actually bound** (folder picker, then the empty-folder flow) | `empty` | C | built (M2) |
| 24 | Stack table: dot, name/dir/declared, Running figures, address pills (+N), Last, switch, ⋯ (`v/library.go`) | `DataTable` with a custom row; switch per row | `table`, `switch` | G+L | — |
| 25 | Row states: busy, missing (Remove…), broken, need, "run by an older doze" | the same | `badge` | G+L | — |
| 26 | Switch: up as last time / down (asks) | preflight, then the `doze -y up --last` subprocess | — | C | built (M2) |
| 27 | StackMenu: Restart / Start everything / Choose what starts… / Edit doze.hcl / Show in Finder / Remove from doze… (`library.tsx:285`) | `PopupMenu` (the ⋯ button) + the same items on right-click | `menu` | C+L | built (M2) |
| 28 | "Also on this machine": other doze programs (owner, pid, Open) | port `others` (ps scan) | — | L | — |
| 29 | Machine line: doctor dots, "N left behind · Clean up…", "Set up (one password)", Machine… | from `doze doctor --json` (C) until names N5 is in Rust | — | C | — |

### 2.3 Stack overview

| # | Go feature | Approach | Component | Needs | Spike |
|---|---|---|---|---|---|
| 30 | Head: name, dir, up X / stopped · when; view switch Tiles·List·Map·Grid (remembered per stack); Activity A; All logs L; stack switch; ⋯ (`fe/pages/overview.tsx`) | the same | `ButtonGroup`, `switch` | G | built (List/Map) |
| 31 | Vitals strip: memory with a share bar per service; cores; state counts; connections; faded when stale | `stat` cards; share bar as `plot` Stack | `plot` | G | built |
| 32 | Busy banner (Starting/Stopping…) | `alert` + `spinner` | `alert` | C | built (M2) |
| 33 | FailedBanner: Edit file / Try again / Copy / Dismiss, "What doze printed", SourceCard (`v/home.go` FailureOf) | the same; SourceCard via a read-only `Editor` with the line marked | `alert`, `input::Editor` | C | built (M2) |
| 34 | ConfigBanner: stack keeps running, Edit doze.hcl ⌘E, excerpt + hint (`v/configerror.go`) | the same; excerpt from `doze lint --json` | `Editor` | C | — |
| 35 | doze.hcl changed → Restart the stack (`v/configerror.go` ConfigChanged) | the same, **plus a diff** (§3) | `diff` | C+L | — |
| 36 | Domain not set up → Set up the machine / Copy command | the same | — | C | — |
| 37 | **Tiles** view: groups, tile with dot/name/pin/switch/Run/⊘, word·because, mem + address with open/copy, filter | grid of tiles; responsive 3/2 columns | — | G | — |
| 38 | **List** view + **details pane**: Addresses, Run (command, awake for, pid, sleeps with Pin), Needs/held-by, last error; Logs l / Restart r / Open ⏎; DownPane for stopped rows | `h_resizable` list + pane (pane width remembered) | `table`, `resizable` | G | built (List + pane, fixture) |
| 39 | **Map** view: Go-placed cards, shared band, bezier edges, fit zoom, lit card fades unrelated cards, detail card, legend, arrow nav (`v/flow.go` 1,022 lines + `geometry.go` 234) | layout from the **`doze-map` crate** (§5.4); painted with PathBuilder (spike code) | gpui canvas | G+L | built (painting, fixture layout) |
| 40 | **Grid** view: services × apps/shared/tasks, crosshair, positioned tooltip with ref "in doze.hcl" (`v/grid.go`, `refs.go`) | `DataTable` with frozen column + hover; refs from `doze status --json` blocks | `table`, `tooltip` | C | — |
| 41 | Stopped stack: DownStrip, rows drawn `down`, a row's switch starts the stack with that row (`v/stopped.go`) | the same; shape from `doze status --json` (works with a broken file) | — | C | — |
| 42 | StartSheet: tick apps, "needs …", "last time", "also starts" (transitive), Cancel / Everything / Start these | `sheet` from the bottom or a centred dialog (board decides) | `dialog`, `checkbox` | C | built (M2) |
| 43 | Density Compact/Cosy/Roomy (List) | theme row height token | `radio` | L | — |
| 44 | sr-only live region announcing crashing/failed | AccessKit live region | gpui a11y | G | — |

### 2.4 Inside a service (instance page)

| # | Go feature | Approach | Component | Needs | Spike |
|---|---|---|---|---|---|
| 45 | Head: glyph, name, path; word + switch / ⊘ paused; crashing: Stop trying s / Try now r; Pin p, Restart r, Sleep s, Wake w, Open o; task: Run ⏎ (`fe/pages/instance.tsx`) | the same | `button`, `switch` | G | — |
| 46 | Tabs from Go: Overview, Logs/Runs, Last run, Console (web), `console:*` module tabs | `tab` bar; tab state per instance | `tab` | G | — |
| 47 | Crash banner (retrying vs gave up, tries pill) | `alert` | `alert` | G | — |
| 48 | PortClash card: holder, Stop <cmd>, Home ⌘0 when the holder is another stack, Use another port (`s/portclash.go`) | port `lsof`/`ss` holder lookup to `doze-desk::ports` | — | L+G | — |
| 49 | "What it said" (last run lines + error, Copy) | log lines component | — | G | — |
| 50 | Connect / Reach it KV with Copy | `description_list` + `clipboard` | — | G | built (pane) |
| 51 | Snippets: Node/Python/Go/Shell/.env (+psql, redis-cli), Copy | `tab` + code view (highlighted) | `Editor` read-only | G | — |
| 52 | Module Extend blocks | console node renderer (#67) | — | G | — |
| 53 | State card: holders pills, dozing countdown "sleeps in X", sleeps text, needs chips | `progress` countdown animated per second (only while visible) | `progress` | G | — |
| 54 | Recent · this run | list | — | G | — |
| 55 | Vitals charts: Memory, CPU, Connections, last ~4 min (`fe/components/chart.tsx`) | `AreaChart` / `plot` with CrossLine hover; longer history (§3) | `chart`, `plot` | G | — |
| 56 | Asleep module: "Wake and open the console" / "Just wake" / Connect | the same | — | G | — |
| 57 | Task never run: steps, command, empty output | the same | — | G | — |
| 58 | Last run: output, Runs table (When, Took bar, Exit, Started by, Shown) | `DataTable`; duration bars | `table` | G | — |
| 59 | Web console tab: iframe of the module's web console, "waking it…", Copy address y, Open in browser o | **no WebView in GPUI**: see open question Q1. Default: wake, then open in the browser; tab shows state + address | — | G | — |

### 2.5 Logs

| # | Go feature | Approach | Component | Needs | Spike |
|---|---|---|---|---|---|
| 60 | All logs (merged, colours per instance, doze's own lines from the journal), instance Logs tab, task output (`fe/pages/logs.tsx`, `v/logs.go`) | one `LogView` entity used in three places | `uniform_list` / `virtual_list` | G | — |
| 61 | Filter with "N of total", level segment Debug/Info+/Warn+/Errors, run segment (This run / All N / run k), Relative/Clock, error/warn counts, Copy c | toolbar; filtering off the UI thread | `input`, `ButtonGroup` | G | — |
| 62 | Follow / pause (scroll up pauses), "Follow again" | list scroll handle; follow when at the bottom | — | G | — |
| 63 | Show checkboxes per service + "doze itself" | sidebar checkboxes | `checkbox` | G | — |
| 64 | Virtualised list, 24 px rows, windows of 200 (`fe/components/vlist.tsx`) | `uniform_list`, every line in a compact ring (§3) | gpui | G | — |

### 2.6 Module consoles

| # | Go feature | Approach | Component | Needs | Spike |
|---|---|---|---|---|---|
| 65 | Console tab from a ConsoleSpec node tree: row/col/table/stat/detail/stream/actions/note/chart (`fe/components/console.tsx`, `v/console.go`) | `ConsoleView` renders the Node tree into gpui elements | `table`, `chart`, `TextView` | G | — |
| 66 | Selectable table rows → `focus` → the stream follows the selector; rates computed (`hub/console.go`) | console subscribe with `panel`/`selector`; rates in `doze-desk` | — | G | — |
| 67 | Action buttons keyed by letter; destructive or input actions ask; `admin` op | `dialog` with input | `dialog` | G | — |
| 68 | `[` `]` move between tables, ↑↓ rows, uppercase letter = action | keymap context `ConsoleTab` | gpui keymap | G | — |
| 69 | Asleep note "the console never wakes it"; reading / err / missing states | `skeleton`, `alert` | — | G | — |

### 2.7 Overlays, confirmations, notices

| # | Go feature | Approach | Component | Needs | Spike |
|---|---|---|---|---|---|
| 70 | Dialog: sleep (dependants / only), restart, reset (type the name, default moves when it matches, "From a script:" foot), ask, action-with-input (`v/dialog.go`, `fe/components/dialog.tsx`) | `dialog` over a dimmed scrim, `DialogEntrance` 180 ms; ←→ between buttons, y/n on restart | `dialog`, `input` | G | built (M2) |
| 71 | ⌘K palette: nothing typed = broken / page / recent / stacks; verb+name with CLI form; scopes (Tab in, ⌫/⇧Tab/Esc out), crumbs; did-you-mean; copy rows "Copied ✓"; recent commands (`v/palette.go` 744 + `commands.go` 347) | port of the palette model to `doze-desk::palette`; spike's overlay (matched to the board) | spike palette, `kbd` | G | built (M2) |
| 72 | Help sheet `?`: keys by scope + "The words" glossary + version (`fe/components/help.tsx`) | generated from the keymap + glossary text | `dialog`, `kbd` | — | built (M2) |
| 73 | Toasts from `notice`: up to 5, success 4 s, failure 12 s with "All" → Recent | `notification` (in-app) | `notification` | G | built (M2) |
| 74 | Recent page: last 20 notices (`fe/pages/recent.tsx`) | becomes a **popover from the toast stack** rather than a page (overlay rule) | `popover` | — | built (M2) |
| 75 | Bell + Needs-you panel: Broken now / Worth doing, fixes, Show, Not now, age, "fixed ✓", N put aside, calm state (`fe/components/bell.tsx`, `v/attention.go`, `s/attention.go`) | `popover` anchored to the bell; list with keys ↑↓ ⏎ ⌘⏎ n | `popover`, `list` | G+C+L | built (M2) |
| 76 | Alerts: toasts for new red items (≤ 3) with first fix + Show | `notification` with actions | `notification` | G | built (M2) |
| 77 | Away strip: "While the window was closed · for", events, Open Needs you, Got it (`v/away.go`) | banner with `Presence` enter | — | G+L | built (M2) |
| 78 | PreflightSheet: checks before Up (folder, doze.hcl loads, ports free via lsof + bind probe, holders own/other user/other stack, domains trusted); fixes stop pid / up without X / anyway / edit / repair (`s/preflight.go` 453) | port to `doze-desk::preflight`; `sheet` | `sheet` | C+L | built (M2) |
| 79 | Instance preflight before wake (`PreflightInstance*`) | same module | — | G+L | built (M2) |

### 2.8 Activity, empty folder, settings

| # | Go feature | Approach | Component | Needs | Spike |
|---|---|---|---|---|---|
| 80 | Activity: last hour / 24 h / 7 d, summary (asleep while up, memory freed, wakes, crashes), per-service timeline with spans, wake-to-ready, crash marks, asleep %, woken by; What happened list; filter by service (`v/activity.go` 426, journal on disk) | read the Go journal files (`internal/journal` format, read-only port) + journal topic; timeline drawn on `canvas` | `plot`, gpui canvas | G+L | — |
| 81 | Empty folder: found table (Found / Where / Becomes), 3 choice tiles (init / add one thing / empty), busy state (`v/empty.go`, `fe/pages/empty.tsx`) | `doze init`/`add` via the CLI subprocess | — | C | built (M2) |
| 82 | Settings › General: Mode System/Light/Dark, density, 5 colour themes drawn in the mode, Open at login, Reopen the page I had, prefs path | `setting` page | `setting` | L | — |
| 83 | Settings › Machine: Repair banner, doctor table by names/domains/app with Copy fix; Left behind (daemons, engines, data, routes, CA) with Look again / Clean up N / per-item / danger asks inline / show all; Copy report (`s/leftovers.go` 427, `s/repair.go` 255) | `doze doctor --json` (C); leftovers ported (ps + globs, L); repair through `doze-os` privilege prompt running `doze dns-setup` / `doze trust` | `setting`, `table` | C+L | — |
| 84 | Settings › About: version, update check (GitHub latest, cached 6 h), What's new, Copy `brew upgrade doze` (`s/update.go`) | the same (`ureq` or std + rustls is extra weight: see Q6) | — | L | — |

### 2.9 Notifications, prefs, links, dev server

| # | Go feature | Approach | Component | Needs | Spike |
|---|---|---|---|---|---|
| 85 | Notification rules D8: priming, silent while on screen, episodes; crash loop, gave up, not ready, stuck waking (5 min + 2 min quiet), failed task (`notify/rules.go`) | port to `doze-desk::notify` with the same tests | — | G | — |
| 86 | Native notification with a fix action (holder stop with pid re-check; restart; task), click → instance (`a/notify.go`) | `cx.show_system_notification` with actions; bundle only | gpui | G | — |
| 87 | Prefs file `$DOZE_HOME/desk/prefs.json`: window, lastDir, hints, theme, mode, views, recent, notNow, commands, windowClosed (`prefs/prefs.go`) | the same file and keys (an app switch keeps settings) + new keys (§3) | serde | L | — |
| 88 | Theme: violet/teal/burgundy/amber/slate × dark/light, system mode live (`fe/tokens.css`) | 10 gpui-component JSON themes generated from tokens.css; `watch_dir` for user themes | `theme` | L | partial (violet dark) |
| 89 | `doze open [dir|doze://] [--show]`, detach from terminal, "✓ doze is open" (`cmd/doze/open.go`) | the Rust app binary takes the same args; until the Rust CLI exists, Go's `doze open` can launch the Rust bundle (Q2) | — | R for the single binary | — |
| 90 | `doze open --serve --fixture …` dev server + Playwright | replaced by the Rust **fixture daemon** + offscreen shots (§5.6) | — | — | partial (ref JSON) |
| 91 | Deskbench budgets in CI (`test/deskbench/budget.json`, `.github/workflows/deskbench.yml`) | `xtask app-bench` (§6) | — | — | partial (measure.ts) |

### 2.10 Keyboard (every Go binding)

All of these become gpui actions with key contexts, so the help sheet, the chips, the menus and ⌘K read one table.

| Scope | Go bindings |
|---|---|
| Global | ⌘0 Home; ⌘1–9 tab n; ⌘O open folder; ⌘L all logs; ⌘, settings; ⌘⇧N bell; ⌘K palette; `?` help; ⌘[ ⌘] back/forward; Esc up; ⌘W close window; ⌘/ help; ⌘⇧D open (tray) |
| Library | ↑↓ j k move; space up/down; ⏎ open; ⌫ remove |
| Overview (running) | ↑↓ j k, ←→ (tiles); space switch/run; ⏎ open; p pin; w wake/run; s sleep; r restart/run; l logs; L all logs; A or T activity; y copy address; / filter; 1–4 views |
| Overview (stopped) | space start with row; u up as last time; c start sheet |
| Map | arrows to the nearest card; ⏎ select/open; Esc clear; space toggle |
| Grid | arrows; ⏎ open tip target; o open row; space; l; p; / |
| StartSheet | ↑↓ space ⏎ Esc |
| Instance | r s w p o y l ⏎; letters go to the console on a console tab |
| Console tab | ↑↓ rows; `[` `]` tables; uppercase letter = action |
| Logs | / filter; 1–4 level; c copy; t relative/clock; space pause/follow; `[` `]` runs |
| Empty page | arrows; ⏎; 1–3 |
| Palette | Esc (query → scope → close); ⌫ on empty; ↑↓; ⏎; Tab; ⇧Tab |
| Dialog | Esc; ←→; ⏎; y/n (restart); Tab trapped |
| Bell | ↑↓; ⏎ first fix; ⌘⏎ show; n not now; Esc |
| StackMenu | ↑↓ Tab ⇧Tab Esc |
| Settings | c copy report |
| Config | ⌘E edit doze.hcl (Go binds it only while the ConfigBanner is mounted: fix it globally) |

**Counts**

| Go app | Count |
|---|---|
| Feature rows above | 91 |
| Pages | 8 (about 30 screens with sub-views and states) |
| Overlays | 13 |
| Bound methods | 48 |
| Push events | 10 |
| Distinct key bindings, across 16 scopes | about 75 |
| Tray items | 7 fixed + 5/3 per stack + the Needs-you section |
| Notification rules | 5 |
| Prefs fields | 10 |
| Boards in the repo (more on the canvas) | 36 |

Where the spike stands on those 91 rows:

| Spike status | Rows |
|---|---|
| built | 7 (visual against fixtures) |
| partial | 7 |
| — | 77 |

Known Go defects that the port fixes rather than copies:
- ⌘N has no handler.
- ⌘E is bound only while a banner is mounted.
- `Dock` and `Sparkline` are dead code.
- `RepairPlan` is bound but unused.
- The Help board's `o` and `⏎` are not bound.
- CPU at rest is marginal: 0.12 % against a budget of 0.10 %.

## 3. Beyond Go: what GPUI makes possible

Each item states the test that proves it. Items marked ★ are the top ten.

### Speed and weight

1. ★ **A native window, a fraction of the memory.**
   - Targets: window idle ≤ 85 MB (Go: 111–121), after close ≤ 35 MB (Go: 44–49), menu bar only ≤ 18 MB (Go: 23).
   - Test: `xtask app-bench` on a quiet machine, in CI on a macOS runner.
2. **Instant window.**
   - Targets: tray click → first frame of the last page ≤ 100 ms (Go: 145 ms); Library → stack overview ≤ 16 ms (one frame).
   - Test: bench timestamps from `on_next_frame`.
3. **Zero idle work.**
   - Redraw only on model change, and only while the window is visible.
   - CPU with the window open and nothing changing ≤ 0.3 %. With the window closed ≤ 0.05 % (Go: 0.10–0.13 % with no window).
   - Test: bench quiet minute, plus a frame counter that must stay 0 over 60 s on a static fixture.

### Logs

4. ★ **A log viewer that holds the whole run.**
   - A compact ring (one `Arc<str>` arena + offsets) keeps **100,000 lines per stack** (Go: 400 per instance, 800 merged).
   - Older history is paged in on demand with the existing `logs` op (`tail`, `since_time`).
   - Tests: memory ≤ 25 MB for 100k typical lines; scroll from top to bottom with no frame > 16 ms (frame timer); follow at 2,000 lines/s with no dropped lines (fixture daemon flood).
5. ★ **Search that navigates.**
   - Regex or plain text, case toggle.
   - Matches highlighted, n/N jump between them, a count of "k of N".
   - Searching runs on the background executor, so typing never blocks.
   - Test: 100k lines filtered in < 50 ms; keystroke-to-frame < 16 ms while a search runs.
6. **Select and copy lines.** Click, shift-click or drag to select line ranges; ⌘C copies them with or without times. Go had only "copy shown" (parity.md lists "drag to select log lines" as not built). Test: a selection test in `doze-app` (simulated input on `TestAppContext`).
7. **Structured lines.** A JSON log line expands in place into a key/value tree (`tree`); the level is read from the JSON when present. Test: fixture line → expanded view snapshot.
8. **Pop-out log window.**
   - ⌥⏎ (or a context menu) opens a stack's logs, or one service's logs, in their own window, which follows independently.
   - The main window stays one window per D11; see Q3.
   - Test: two windows, each following, with memory delta ≤ 20 MB per extra window.

### Keyboard-first

9. ★ **One action table drives everything.**
   - Every command is a gpui `Action`, and the same table generates:
     - the app menus (`set_menus`);
     - the ⌘K rows;
     - the Help sheet;
     - the key chips (`Kbd::binding_for_action`);
     - the tray accelerators.
   - So no chip can lie, and no command exists only in one place. Go got this wrong: ⌘N had no handler, and the Help board listed `o` and `⏎`, which nothing bound.
   - Test: for every action, there is a menu item or a palette row, and every chip resolves to a binding.
10. **A user keymap.** `~/.doze/desk/keymap.json`, Zed-style (context + keystroke → action), hot-reloaded. Test: rebinding `s` to `z` changes the chip, the Help sheet and the behaviour.
11. **Multi-select and bulk verbs.**
    - Shift/⌘-click or `v` to select rows in List, Tiles or Grid, then s / w / r / p acts on all of them.
    - One dialog for the whole set, using Go's existing `DoAll` sequencing.
    - Test: select 3 rows, sleep, then 3 verb calls in order on the fixture daemon.
12. **Vim-style list motion everywhere.** `j k g g G`, plus `⌃d ⌃u` half pages in logs. Test: keymap tests.

### Command palette

13. ★ **⌘K that knows everything.**
    - Everything Go's palette does: scopes, did-you-mean, the CLI form, recents.
    - Plus:
      - results ranked by recency of use;
      - **inline previews**: highlighting a service row shows its live state, memory and the last 3 log lines in a side panel;
      - **actions with arguments**: `logs api /timeout` opens logs filtered;
      - **⌘K from the menu bar**: tray "Search or do…" opens the window straight into the palette.
    - Test: palette model goldens (ported from Go's), plus a preview render shot.

### Overlays and motion (the owner's bar)

14. ★ **Overlays, never pages, with gentle motion.**
    - The inspector is a `sheet` sliding from the right, from the top of the content to the bottom.
    - Recent notices are a popover. Dialogs enter with `DialogEntrance`.
    - Page changes cross-fade with `NavMotion`; tiles stagger in on first paint.
    - All motion 120–280 ms from `MotionTokens`; reduce motion is honoured (`apply_system_reduce_motion`).
    - Tests: an "overlay contract" test (every board of kind drawer or dialog opens over its parent, which stays rendered); a motion test (with reduce motion on, no animation frames are scheduled).
15. **No layout shift.** Toolbars, filters and banners never move content above the change. Test: element bounds before and after each in-page control, read from `TestAppContext` (the numeric check the owner asked for).

### Native integration

16. ★ **Native everywhere it counts.**
    - OS-drawn context menus (`native_menu`) on every row, tile, card, log line and address.
    - A Dock badge with the red count (`NSDockTile` in `doze-os::ui`).
    - Folders dropped on the Dock icon or the status item open the stack.
    - Notifications with action buttons.
    - Test: per-feature smoke under the bundle (by eye, plus a unit test of the menu model).
17. **Menu-bar quick look.** The status item's menu shows a custom view per running stack: a tiny live memory sparkline and the trouble line. It sticks to the "no numbers by the bat" rule: the numbers are inside the menu, never on the icon. Test: menu model snapshot. The custom NSView needs the `doze-os::ui` path (Q4).

### Data the Go app could not show

18. ★ **Live charts with history and hover.**
    - 1 h at 1 s per instance (3,600 samples × 4 f32, 56 KB each) instead of 4 minutes.
    - Memory, CPU and connections with a CrossLine hover tooltip.
    - The vitals strip gets a stacked memory area over time.
    - Test: chart model unit tests; a hover shot.
19. **Who keeps what awake, as a Sankey.** Holders (dependant, pin, connection) flow into services. The data is real: `InstanceView.holders`. `chart::SankeyChart`. Test: fixture → shot. This needs the board drawn first (Q8).
20. ★ **doze.hcl, seen not edited.**
    - A read-only highlighted viewer (`input::Editor` + `syntect` HCL), with the error line marked from `doze lint --json`.
    - "doze.hcl changed" shows a **diff** against the copy the stack was started with (`diff` component), before you restart.
    - D9 stands: editing stays in your editor (⌘E opens it at the line).
    - Test: config error fixture → shot with the line marked; changed fixture → diff shot.
21. **Undo for reversible verbs.** "Slept api · Undo" for 6 s on sleep, pin and off. Undo sends the inverse verb. Reset, remove and down are not undoable and say so. Test: a toast action dispatches the inverse verb on the fixture daemon.
22. **Exact restore.** Reopening restores the tab, the view, the selected row, the scroll position of logs and lists, the open inspector, and the pane width. Test: prefs round trip plus a shot after reopen.

### Quality

23. ★ **Accessible by construction.**
    - AccessKit roles and labels on every control.
    - VoiceOver reads a row as "api, awake, 2 connections, switch on".
    - Live announcements for crashing/failed.
    - Test: an accessibility-tree snapshot per screen in CI (no screen reader needed).
24. **Screenshot suite with no browser.**
    - Every screen × theme (10) × size (1280×800, 900×620) is rendered offscreen, about 400 PNGs.
    - Compared numerically with references; the threshold per screen is written in a file.
    - Go needed Playwright with two browsers on one worker.
    - Test: it is the test.
25. **User themes.** Drop a JSON theme into `~/.doze/desk/themes/` and it appears in Settings, live (`ThemeRegistry::watch_dir`). Test: a theme file round trip.

### Top ten

The ★ items are 1, 4, 5, 9, 13, 14, 16, 18, 20 and 23. In order of importance:

1. **4**: the whole-run log viewer
2. **5**: search that navigates
3. **9**: one action table
4. **13**: ⌘K that knows everything
5. **14**: overlays and motion
6. **1**: a native window at a fraction of the memory
7. **18**: live charts with history and hover
8. **20**: the doze.hcl viewer and diff
9. **16**: native integration
10. **23**: accessibility

## 4. Architecture

### 4.1 Principles

- **Rust owns every fact, as Go did (D16), but in-process.** The view-model layer has no GPUI dependency: plain structs computed from the daemon's state. It is tested with goldens, and its outputs are differentially checked against Go's view JSON on the same fixtures for as long as Go exists.
- **The GUI layer only renders and dispatches actions.**
- **No Go stitching inside the app.**
  - The app speaks the daemon's control protocol itself.
  - For the few things that only the Go CLI can do today, it runs `doze --json …` as a subprocess:
    - stack up/down/restart (Go does this too);
    - doctor;
    - lint;
    - the shape of a stopped stack;
    - init/add.
  - That boundary is the CLI's documented machine contract (`--json` envelopes, exit codes). The Rust CLI will keep the same contract, so the app does not change when the Rust CLI replaces the Go one.
  - Rejected: reusing the Go app's `--serve` HTTP view API. It would be Go stitching, and it would leave the app empty once Go is gone.
- **Redraw only on change.** The hub coalesces frames and tells the UI which keys changed. Entities call `cx.notify()` only when their model changed. This is the spike lesson: idle 73 MB, but 216 MB while redrawing at 10/s.

### 4.2 Crates (inside doze-rs)

| Crate | What | Depends on | GPUI? |
|---|---|---|---|
| `doze-control` | Control protocol 2: every op, topic and field from `internal/control/types.go` as serde types; a sans-IO NDJSON codec; a blocking client (std `UnixStream` + a thread, as the spike proved) with hello, ids, cancel, subscribe streams and reconnect back-off (500 ms → 5 s). A `test-support` **fixture daemon** that serves scripted frames on a real socket. Shared later with the Rust CLI and (server half) the Rust daemon | serde, serde_json, async-channel, doze-os (the blocking socket in `doze_os::local`; doze-os also compiles tokio, which this crate does not call) | no |
| `doze-projects` | Project records (`projects/*/project.json`, schema 1), socket discovery, stale-pid check, stack keys; read-only for the Go formats | doze-os | no |
| `doze-display` | The words: Look (word, glyph, tone), because, tallies, vitals, durations, rates, fuzzy match. Port of Go's `internal/display` (1,915 lines). Shared with the Rust CLI ("same words") | — | no |
| `doze-journal` | Read-only reader of the Go journal on disk (`internal/journal`, 291 lines) for Activity and Away | — | no |
| `doze-map` | Map layout (port of `v/flow.go` + `geometry.go`), `next_card`. **Being built by another agent on the `map` branch**; this plan consumes it (§5.4) | doze-display | no |
| `doze-desk` | The app's brain, GPUI-free: the hub (state per stack, rings, series, journal, console frames, watch ref-counts, Connecting/Live/Down); view models (library, home, stopped, grid, instance, logs, console, dialog, palette + commands, attention, preflight, away, activity, empty, tray, notify rules); prefs; link; autostart; ports (lsof/ss); leftovers; update check; stack actions via the CLI subprocess | the five above | no |
| `doze-app` | The GPUI binary: shell, screens, widgets, theme, the action table and keymap, overlays, the tray glue, notifications, `shot` and `bench` subcommands. Feature `capture` = gpui `test-support` | gpui-pre, gpui-pre-platform, gpui-component, doze-desk | yes |
| `doze-os` (existing), new module `ui` behind feature `ui` | The only `unsafe`: `NSStatusItem` + `NSMenu` (custom views), Dock badge, URL-open events, activation, a native folder picker if gpui's is not enough; `ksni` on Linux; Windows later | objc2, objc2-app-kit | no |

**Splitting `doze-app` into modules** (each file within the 450-line limit; the spike's `page.rs` is 632 lines and is split):
- `shell/`: titlebar, tabs, sidebar, keys bar, banners, nav
- `library/`
- `overview/`: strip, tiles, list, pane, map, grid, stopped, start sheet
- `instance/`: overview, tabs, runs, snippets, charts
- `logs/`: view, ring adapter, search, selection
- `console/`: node renderer
- `overlays/`: dialog, palette, help, bell, toasts, preflight, away
- `activity/`
- `settings/`
- `empty/`
- `tray`
- `notify`
- `theme/`
- `actions` (the one table)
- `shot`
- `bench`

### 4.3 Daemon client and threading

- **One client thread per running stack.**
  - It holds the long-lived `subscribe` for `instances` + `journal`.
  - It adds `stats` and `logs` while watched, re-subscribing when a watch count crosses zero (Go's rule).
  - Unary verbs and `admin` go on a second, short-lived connection, so a long verb never blocks the stream.
- Frames fold into the hub's state on that thread. Each fold returns a `Changed { key, parts }` set, which is sent to the UI over `async_channel` and coalesced per frame (at most one UI update per 16 ms, and none while hidden).
- This is the spike's thread-plus-channel design. It does not bring in tokio: the app does not need a reactor, and gpui has its own executors for background work (search, filtering).
- The codec is sans-IO, so the future tokio daemon reuses the types and the framing.
- **Windows**: the Go daemon has no named pipe yet. The Rust core plan (doze-rs `modules/PLAN.md` line 234) gives the core a named-pipe admin socket. `doze-control` keeps the transport behind a `Conn` trait from the start.

### 4.4 State and model in GPUI

- `AppState` is a global entity. It holds the `doze-desk` hub handle, prefs, the tab list, nav history, attention, and the theme.
- Each screen is an `Entity<…View>` that reads a **view model**, computed by `doze-desk` from the hub state. Rows, palette results and log windows are computed on demand: cheap, and pure.
- View models are `PartialEq`. An entity re-renders only if its model changed. This is the spike's `Root::apply` returning `bool`, done systematically.
- Overlays are a stack on the root (`Option<Entity<Dialog>>`, palette, sheet, bell popover). gpui-component's `Root` and `WindowExt::open_dialog` / `open_sheet` / `push_notification` own the layering and the focus trap.
- Keymap contexts: `Library`, `Overview`, `Map`, `Grid`, `Instance`, `ConsoleTab`, `Logs`, `Palette`, `Dialog`, `Bell`, `StackMenu`, `StartSheet`, `Empty`, `Settings`. They mirror Go's scopes one to one, so the parity tests can enumerate them.

### 4.5 How the map crate plugs in

- `doze-map` is GPUI-free. Its API:
  - `doze_map::layout(&StackFacts) -> MapView`: boxes, lanes, the band, caps, edge paths as typed segments rather than SVG strings, folds, details.
  - `MapView::next_card(from, Dir)`.
  - `MapView::lit(name) -> LitSet`: what to fade and which lines to draw.
- `StackFacts` is built by `doze-desk` from the hub plus the stack's references (who calls whom). The references come from `doze status --json` blocks until the Rust config crate exists.
- `doze-app/overview/map.rs` keeps the spike's painter, fed typed segments instead of re-parsing SVG strings:
  - PathBuilder curves;
  - dashes, arrowheads and dots;
  - the tessellated-layer cache keyed by a signature;
  - fit-zoom.
- The spike's `nav` check (96 moves against flow.tsx) becomes a `doze-map` unit test with the Go fixture as the oracle.
- If the map agent's crate shape differs, `doze-desk` adapts in one place (`desk::map_facts`). The app never reaches into map internals.

### 4.6 Theme

- `tokens.css` (5 palettes × dark/light) is converted once by an xtask into 10 gpui-component theme JSON files, plus doze's own extra tokens (tones, the scrim, chart series).
- Inter and JetBrains Mono ship as static TTF weights; a variable font loaded from memory is stuck at one weight.
- Letter-spacing keeps the spike's `tracked` emulation, until gpui gains tracking.
- Density changes the row height token. Mode follows the system live.

### 4.7 Screenshot and interaction tests

- **`doze-app shot`** (the spike's `shot.rs`, generalised):

  ```
  doze-app shot --fixture shop --screen list --select api --theme teal --mode light --size 1280x800 --out x.png
  ```

  - It renders offscreen through `render_to_image()`: no screen-recording permission and no window on screen.
  - Screens are named after the boards.
- **Fixtures**:
  - `doze-control`'s fixture daemon replays scripted frames on a real socket.
  - The scripts are captured once from the Go fixture hubs (shop, crash, modules, larder, empty) and committed to `crates/doze-app/fixtures/`.
  - So the app is tested through the same protocol path it uses in production.
- **References and diffs**:
  - First the Go page (`doze open --serve --fixture` + `test/desk-e2e/shot.ts`), then the boards.
  - `xtask app-shots` diffs numerically (mean per channel, and a max-region check) against a per-screen threshold file.
  - Every allowed difference is an allow-list entry. An entry whose reason starts "improvement:" is reported under "Better than the board" (the owner's rule).
- **Interaction tests**:
  - gpui `TestAppContext` simulates keystrokes and clicks: palette flows, dialogs, multi-select, selection, nav, overlay contract, layout-shift bounds.
  - Accessibility-tree snapshots per screen.
  - View-model goldens in `doze-desk`, plus a **differential test** against Go's `internal/desk/view/testdata/*.json` (19 goldens).
- **By eye**: each milestone ends with board pairs rendered side by side, an independent audit agent, and the owner's look. "Self-judged fidelity is too generous."

## 5. Feature availability: Go daemon now vs Rust core later

| Builds now against the Go daemon / Go CLI | Waits for the Rust core |
|---|---|
| Everything live: discovery, status, subscribe (instances, journal, stats, logs, console), verbs, admin actions, module consoles, logs history (`logs` op) | The **single binary** (`doze` = CLI + daemon + app). Until then the app ships as its own binary inside `Doze.app` (Q2) |
| Stack up/down/restart, init/add, lint, doctor, status shape of a stopped stack: the Go CLI's `--json` contract | A doctor/repair that does not shell out (names N5: setup + trust) |
| Library from project records; Activity and Away from the on-disk journal (read-only formats) | Native doze.hcl semantics in-process (grid refs, stopped shape, the Map's call graph without a subprocess): the Rust config/HCL work, measured at about 47 days in the HCL spike |
| Preflight, port clash, leftovers (lsof/ss/ps + files), prefs, autostart, links, update check | Windows (named-pipe admin socket, from the core plan) |
| Notifications, tray, menus, every overlay | Anything the next daemon API (`docs/design/next/daemon-api.md`, "Contract 2") adds beyond today's protocol: why-awake holders as a first-class topic, wake causes. Today's `InstanceView.holders` already gives most of it |

**Result: the whole Go app's feature set, and every "beyond" item, can be built against the Go daemon today.** Nothing in the parity table is blocked. Two things change once the Rust core lands:
- the single binary;
- in-process config, which removes the `doze status --json` subprocess calls.

## 6. Budgets

These are measured by `xtask app-bench` (a port of deskbench) on a macOS CI runner. They are informational on Linux until a by-eye pass.

| Measure | Go (Wails) today | Spike | Budget |
|---|---|---|---|
| Menu bar only, footprint | 23 MB | 11–16 MB | **≤ 18 MB** |
| Menu bar ready after launch | 77–81 ms | ~80 ms | **≤ 100 ms** |
| Window first frame of home after the tray click | 145 ms | not measured | **≤ 100 ms** |
| Window open, idle | 111–121 MB | ~73 MB | **≤ 85 MB** |
| Window while logs follow at 200 lines/s | not measured | — | **≤ 160 MB** sustained; back under 100 MB within 10 s of stopping |
| After the window closes (1 min) | 44–49 MB | 33 MB | **≤ 35 MB** |
| CPU at rest, window closed | 0.08–0.13 % | — | **≤ 0.05 %** |
| CPU, window open, static | — | ~0 % | **≤ 0.3 %** |
| Frames > 16 ms while scrolling 100k log lines | — | — | **0** in a 5 s scroll |
| 100k log lines held | — | — | **≤ 25 MB** |
| App binary (macOS arm64, stripped, fonts included) | Go binary + webview assets | 12 MB | **≤ 16 MB** for `doze-app` alone (Q5 decides whether it joins the `doze` binary) |
| Crate count | — | — | recorded in `weight.md`; gpui pulls in a few hundred crates, so the app gets **its own** limit line and does not move the kafka/aws gates |

The 216 MB redraw peak is the OS compositor floor (a bare Metal view costs 168 MB), so the rules are:
- coalesce updates;
- never animate continuously;
- the dozing countdown repaints once a second, only while visible;
- no spinners left running offscreen.

## 7. Build order and estimates

These are working days for one builder agent with independent audits, and honest rather than compressed. They come from the Go line counts each step ports and the spike's measured pace: the Map took a day to paint at fidelity, and the palette and list one to two days each.

| # | Step | Ports (Go lines) | Days |
|---|---|---|---|
| **M0: Foundations** | | | **26** |
| 0.1 | Crate skeletons; pinned `gpui-pre`, `gpui-component`; macOS CI with the Metal toolchain; app weight line; lint carve-outs written down | — | 2 |
| 0.2 | `doze-control`: all ops/topics/types, codec, client, reconnect, cancel; fixture daemon; smoke against a real Go daemon (read-only ops) | control 2,640 (types only) | 5 |
| 0.3 | `doze-projects` + `doze-journal` readers | 435 | 2 |
| 0.4 | `doze-display` port with tests (shared with the Rust CLI) | 1,915 | 5 |
| 0.5 | `doze-desk` hub: per-stack state, rings, series, journal fold, console fold, watches, status | hub ~1,250 | 4 |
| 0.6 | `doze-os::ui` tray + Dock badge + URL events; app shell: activation, window lifecycle, frame prefs, work-area first open, single instance + arg forwarding, links, file drop | app/tray/link/prefs ~1,000 | 5 |
| 0.7 | Theme (10 JSON themes from tokens.css, fonts, density, motion tokens); the action table + keymap + menus skeleton; `shot` + `xtask app-shots` + fixtures captured from Go | — | 3 |
| **M1: See everything (read-only daily driver)** | | | **39** |
| 1.1 | Library: table, switch (view only), states, others, Machine line (doctor --json), empty library, steps | library 437 + parts of library.go 740 | 5 |
| 1.2 | Overview: head, vitals strip, banners, List + pane, Tiles, density, stopped (status --json) | home 428, stopped 245, configerror 145 | 7 |
| 1.3 | Map: integrate `doze-map` (assumes the crate lands separately; +6–8 days if this step has to build the layout) | — | 3 |
| 1.4 | Grid view with tooltips and refs | grid 266, refs 148 | 4 |
| 1.5 | Instance page: head, tabs, overview cards, snippets, state card, crash + port clash, charts, asleep, task pages, runs | instance 498, portclash 193 | 8 |
| 1.6 | Logs: ring, uniform list, filter, levels, runs, clock, merged + Show, follow/pause, copy | logs 183, vlist | 5 |
| 1.7 | Module console tabs: node renderer, selection → selector, rates, streams | console 409 + 275 | 4 |
| — | Audit M1 + board pairs + fixes | — | 3 |
| **M2: Act** | | | **37** |
| 2.1 | Verbs and stack actions: switches, buttons, CLI subprocess for up/down/restart, busy states, refusal when the daemon is down | serve verbs, library actions | 4 |
| 2.2 | Dialogs (sleep/restart/reset/ask/input), toasts, Recent popover, module actions | dialog 292 | 4 |
| 2.3 | ⌘K: palette + commands model, scopes, did-you-mean, recents, CLI form, routing | palette 744, commands 347 | 7 |
| 2.4 | Needs you: attention model + steadiness rules, bell popover, alerts, away strip | attention 441 + 374, away 144 | 7 |
| 2.5 | Preflight sheet + instance preflight + fixes; StartSheet | preflight 453 | 5 |
| 2.6 | Empty folder flow, Open folder ⌘O, New stack ⌘N, drop | empty 76 + setup | 3 |
| 2.7 | Help sheet, every keymap scope, native menu complete | — | 2 |
| — | Audit M2 + fixes | — | 5 |
| **M3: Around the clock** | | | **26** |
| 3.1 | Tray complete: stack lines, submenus, Needs-you section, alert icon, signature rebuild | tray 217, a/tray 263 | 3 |
| 3.2 | Notifications: rules port + native with fix actions (bundle) | notify 148 + 159 | 3 |
| 3.3 | Activity: journal spans, summary, timelines, What happened | activity 426, timeline 48 | 5 |
| 3.4 | Settings: General, Machine (doctor, repair via the admin prompt, leftovers), About (update check) | leftovers 427, repair 255, update 63 | 7 |
| 3.5 | `Doze.app` bundle, ad-hoc local signing, CI signing from secrets, `doze open` handing off to the bundle (Q2) | — | 4 |
| 3.6 | `xtask app-bench` (budgets in §6) + soak | deskbench | 4 |
| **M4: Beyond Go** | | | **31** |
| 4.1 | Log viewer: 100k ring, history paging, regex search with n/N, selection + copy, JSON expansion, pop-out window | — | 8 |
| 4.2 | ⌘K previews, arguments, from the tray; user keymap file | — | 4 |
| 4.3 | Charts with 1 h history + hover; stacked memory area; Sankey (if drawn) | — | 4 |
| 4.4 | doze.hcl viewer + changed diff | — | 3 |
| 4.5 | Context menus everywhere, multi-select + bulk verbs, undo toasts, exact restore | — | 6 |
| 4.6 | Accessibility pass + a11y-tree snapshots; motion and overlay contract tests; layout-shift tests; user themes | — | 6 |
| **M5: Ship** | | | **12** |
| 5.1 | Board-by-board fidelity sweep with the owner (every board × themes × sizes) | — | 5 |
| 5.2 | Final independent audit + fixes | — | 5 |
| 5.3 | Docs (app pages on the website) | — | 2 |
| **Linux** (after M5) | StatusNotifier tray (ksni), Wayland/X11 by eye on the Lima VMs, notifications, autostart, packages | — | 8–10 |

**Total, macOS: 171 working days.** The honest range is **150–195 days**.

The range covers three things:
- the map crate, which might need 6–8 days added here;
- gpui-pre churn, since the crate is pre-release;
- fidelity rounds the owner asks for.

Linux adds 8–10 days.

**What each milestone delivers:**
- M0 + M1 (65 days): a read-only app showing everything the Go app shows, live.
- M2 (102 days in): it replaces the Go app day to day.
- M3 (128 days in): parity, shippable.
- M4: everything beyond Go.
- M5: the ship-quality pass.

M4 items can move earlier when the owner wants a showpiece; the log viewer is the best candidate.

## 8. Risks

- **gpui-pre is a pre-release fork-rename of Zed's gpui.** It is pinned with `=`, and gpui-component pins it exactly. Upgrades are planned events.
  - Mitigation: keep GPUI use behind `doze-app` only, and keep the brain GPUI-free.
- **No WebView**: the kafka/aws/temporal web consoles (Go: an iframe tab). See Q1.
- **The `unsafe` rule**: tray, Dock badge and URL events need objc2. This plan puts them in `doze-os` (already the one exempt crate, D11). The alternative, the `tray-icon` crate, needs no exemption but runs its own event-loop assumptions and adds weight. Q4.
- **Compositor floor**: memory jumps with any redraw. The redraw discipline is a gate (frame counter test), not a hope.
- **Measuring on the owner's machine is noisy.** The note says input bursts to 250–320 MB, so the benches run in CI or with the HID idle check.
- **The Go formats read by the app** (project records, journal, prefs) are contracts until the Rust core owns them. Differential tests against Go fixtures pin them.

## 9. Open questions for the owner

1. **Q1: web consoles.** GPUI has no WebView. Options:
   - (a) open in the browser, with a native "console" tab showing state, address and a big Open button. Zero weight.
   - (b) embed `wry` (WKWebView) in a child window or panel. This brings back the ~200 MB webview floor, but only while a console is open.
   - (c) render the kafka/aws consoles natively in GPUI later, which is a big job.

   Recommendation: (a) now, and measure (b) as a spike.
2. **Q2: one binary or a bundle binary?** Until the Rust CLI exists, the app is its own binary in `Doze.app`, and Go's `doze open` launches it. Later, is the app in the single `doze` binary (D6), or a separate `doze-app` in the bundle that the CLI execs? It adds ~10 MB and a few hundred crates to every CLI call's binary, and it breaks the 10 MB ceiling if it is joined.

   Recommendation: a separate binary in the bundle; the CLI stays lean.
3. **Q3: multiple windows.** D11 says one window, a tab per stack. Is a pop-out log or console window (beyond #8) acceptable, or does everything stay in one window?
4. **Q4: `unsafe` for the tray.** Can the tray, Dock badge, URL events and custom menu views go into `doze-os` behind a `ui` feature? Or does the owner prefer `tray-icon` (no `unsafe` of ours, more weight, less control: no custom NSMenu views)?
5. **Q5: size.** Is ≤ 16 MB right for the app binary? The 10 MB ceiling was set for doze-kafka.
6. **Q6: update check.** It needs an HTTPS client (rustls, ~1 MB). Keep it, or shell out to `doze version --check` once the CLI has it?
7. **Q7: Recent notices as a popover** instead of a page (the overlay rule), and the Grid view: is it still wanted? It is tagged "new" on the boards.
8. **Q8: boards for the beyond items.** Should the beyond items get boards first (the log viewer's search and selection, the inspector sheet, the Sankey, the doze.hcl viewer and diff, the tray quick look)? The memory rule says boards are grounded first. Recommendation: draw them during M1, and build them in M4.
9. **Q9: Linux timing.** Does Linux ship with the macOS release, or after it (as the Go app decided)?

## 10. Files

- This plan: `docs/port/app/PLAN.md`.
- New crates (§4.2):
  - `crates/doze-control`
  - `crates/doze-projects`
  - `crates/doze-display`
  - `crates/doze-journal`
  - `crates/doze-desk`
  - `crates/doze-app`
  - a new module `crates/doze-os/src/ui/`
- `crates/doze-map`, from the `map` branch.
- Tooling:
  - `xtask` subcommands `app-shots`, `app-bench`, `themes`;
  - `xtask/weight-limits.txt`, with an app line;
  - `docs/port/weight.md`, with the reason.
- Reference sources (Go, `origin/main`):
  - `internal/desk/**`
  - `internal/control/{types,server,client}.go`
  - `internal/display/`, `internal/journal/`, `internal/projects/`
  - `docs/design/desktop/{README,parity,d18-library-and-switch,d19-needs-you}.md`
  - `docs/design/desktop/boards/`
  - `docs/design/next/daemon-api.md`
- Spike: `~/Code/doze-dev/doze-gpui-spike/src/{app,page,palette,map,widgets,theme,model,daemon,tray,shot}.rs`.

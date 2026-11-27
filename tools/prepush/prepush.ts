// `just prepush`: everything this Mac can run that CI (.github/workflows/ci.yml) runs, before a push, so CI stops being the test
// range. The CI workflow is the spec: the scripts it runs (.github/ci/{check,os,modules}.sh) run unchanged, the steps it writes
// inline (the app's, the consoles', the fuzz runs) are read out of ci.yml and run as written, and a job prepush does not know
// fails the run (the `ci-drift` gate) until someone says here how it is covered.
//
//   bun tools/prepush/prepush.ts [--plan] [--base <ref>] [--no-fetch] [--fail-fast] [-v] [--only a,b] [--with suite,...]
//                                [--fuzz-seconds N] [--no-vm]
//
//   --plan           print what would run and why, run nothing
//   --only a,b       run only these gates (a partial run: never a pass, exit 3 when everything that ran passed)
//   --with s         run a diff-selected suite even though nothing it covers changed (e.g. --with console-e2e)
//   --fuzz-seconds   seconds per fuzz target (default 10; CI runs 30)
//   --no-vm          skip the Linux VM gates (reported as skipped, exit 3 at best)
//   -v               stream every command's output (it always goes to the proof log)
//
// The whole-workspace gates always run (fmt, clippy with every feature set CI lints, the workspace tests, rustdoc, the generated
// and catalogued files, supply chain, the Windows type-check, the path lint); the expensive suites run when the diff against
// origin/main reaches what they cover (tools/prepush/plan.ts). Linux runs in the Lima VM `doze-ubuntu` (PREPUSH_VM), after the
// Mac's gates, never beside them: a loaded host makes the signal tests flaky (doze-cli's SIGINT tests at load ~22) and the VM's links need the memory.
//
// Exit: 0 every gate passed (warnings allowed), 1 a gate failed, 3 a partial run (--only, --no-vm, or the VM missing), 2 usage.
// The proof log is <git common dir>/prepush/<sha>.log (inside .git: never committed, survives `just clean`, shared by worktrees).

import { spawn } from "node:child_process";
import { appendFileSync, existsSync, mkdirSync, readFileSync, rmSync, statSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { fuzzCrates, plan, root, type Plan } from "./plan";
import { judge, SPECS } from "./weight";

// ── Arguments ──────────────────────────────────────────────────────────────────────────────────────────────────────────

const argv = process.argv.slice(2);
const flag = (f: string) => argv.includes(f);
const opt = (f: string) => {
  const i = argv.indexOf(f);
  return i >= 0 ? argv[i + 1] : undefined;
};
const known = new Set(["--plan", "--base", "--no-fetch", "--fail-fast", "-v", "--only", "--with", "--fuzz-seconds", "--no-vm"]);
for (let i = 0; i < argv.length; i++) {
  if (!known.has(argv[i])) {
    console.error(`prepush: unknown argument ${argv[i]}`);
    process.exit(2);
  }
  if (["--base", "--only", "--with", "--fuzz-seconds"].includes(argv[i])) i++;
}
const BASE = opt("--base") ?? "origin/main";
const VERBOSE = flag("-v");
const ONLY = opt("--only")?.split(",");
/** Whether --only (if given) names this gate, or a group it is in (`weight` for `weight doze`). */
const wants = (id: string) => !ONLY || ONLY.some((o) => id === o || id.startsWith(o + " "));
const WITH = opt("--with")?.split(",") ?? [];
const FUZZ_SECONDS = Number(opt("--fuzz-seconds") ?? 10);
const VM = process.env.PREPUSH_VM ?? "doze-ubuntu";

// ── Environment: CI's, on this machine ─────────────────────────────────────────────────────────────────────────────────

const TARGET = join(root, "target");
const env: Record<string, string> = {
  ...(process.env as Record<string, string>),
  // The CI steps read binaries at target/...: one target, the repository's.
  CARGO_TARGET_DIR: TARGET,
  CARGO_BUILD_JOBS: process.env.CARGO_BUILD_JOBS ?? "4",
  RUST_TEST_THREADS: process.env.RUST_TEST_THREADS ?? "4",
  // ci.yml's `env:` for every job.
  RUSTFLAGS: "-D warnings",
  CARGO_TERM_COLOR: "never",
  // No debug info in test builds: one full run built 95 GB of target/debug with it (2026-11-27), and the console-e2e step builds
  // doze-aws this way anyway, so the two now share one build. Panics still say file and line; a backtrace has no line numbers
  // (run the failing test by hand for those).
  CARGO_PROFILE_DEV_DEBUG: "0",
  NO_COLOR: "1",
};

function sh(cmd: string, args: string[], cwd = root): string {
  return execFileSync(cmd, args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], env }).trim();
}

// ── The proof log ──────────────────────────────────────────────────────────────────────────────────────────────────────

const started = Date.now();
const HEAD = sh("git", ["rev-parse", "HEAD"]);
const dirty = sh("git", ["status", "--porcelain", "--untracked-files=no"]) !== "";
const logDir = resolve(root, sh("git", ["rev-parse", "--git-common-dir"]), "prepush");
mkdirSync(logDir, { recursive: true });
// A partial run (--only) is no proof: it gets a name of its own, so it never overwrites a full run's log.
const LOG = join(logDir, `${HEAD}${dirty ? "-dirty" : ""}${ONLY ? `-only-${Date.now()}` : ""}.log`);
// --plan writes nothing (a run of the same sha may be writing this file).
const PLAN_ONLY = flag("--plan");
if (!PLAN_ONLY) writeFileSync(LOG, `prepush ${HEAD}${dirty ? " (working tree with uncommitted changes)" : ""} at ${new Date().toISOString()}\n`);
const log = (s: string) => {
  if (!PLAN_ONLY) appendFileSync(LOG, s.endsWith("\n") ? s : s + "\n");
};
const say = (s: string) => {
  console.log(s);
  log(s);
};

// ── Gates ──────────────────────────────────────────────────────────────────────────────────────────────────────────────

type Status = "pass" | "warn" | "fail" | "skip" | "n/a";
interface Result {
  id: string;
  where: string;
  status: Status;
  secs: number;
  note: string;
}
const results: Result[] = [];
let failed = false;
let partial = false;

/** Runs one command in bash, its output to the log (and the terminal with -v); the exit code and the last lines. */
function run(
  cmd: string,
  opts: { cwd?: string; env?: Record<string, string>; timeoutMin?: number; onLine?: (l: string) => void; stdin?: string } = {},
): Promise<{ code: number; tail: string[]; timedOut: boolean }> {
  return new Promise((done) => {
    log(`$ ${cmd}${opts.cwd && opts.cwd !== root ? `   (in ${opts.cwd})` : ""}`);
    const child = spawn("bash", ["-o", "pipefail", "-c", `exec 2>&1\n${cmd}`], {
      cwd: opts.cwd ?? root,
      env: { ...env, ...opts.env },
      // Its own process group, so a timeout or ^C stops everything it started (by group id, never by name).
      detached: true,
      stdio: [opts.stdin === undefined ? "ignore" : "pipe", "pipe", "pipe"],
    });
    current = child.pid!;
    if (opts.stdin !== undefined) {
      child.stdin!.write(opts.stdin);
      child.stdin!.end();
    }
    const tail: string[] = [];
    let partialLine = "";
    child.stdout!.on("data", (b: Buffer) => {
      const s = b.toString();
      appendFileSync(LOG, s);
      if (VERBOSE) process.stdout.write(s);
      const lines = (partialLine + s).split("\n");
      partialLine = lines.pop()!;
      for (const l of lines) {
        tail.push(l);
        if (tail.length > 60) tail.shift();
        opts.onLine?.(l);
      }
    });
    let timedOut = false;
    const timer = setTimeout(
      () => {
        timedOut = true;
        log(`prepush: timed out after ${opts.timeoutMin} minutes; stopping process group ${child.pid}`);
        killGroup(child.pid!);
      },
      (opts.timeoutMin ?? 60) * 60_000,
    );
    child.on("close", (code) => {
      clearTimeout(timer);
      current = 0;
      if (partialLine) tail.push(partialLine);
      done({ code: code ?? 1, tail, timedOut });
    });
  });
}

let current = 0;
function killGroup(pid: number) {
  try {
    process.kill(-pid, "SIGTERM");
  } catch {}
  setTimeout(() => {
    try {
      process.kill(-pid, "SIGKILL");
    } catch {}
  }, 10_000).unref();
}
process.on("SIGINT", () => {
  if (current) killGroup(current);
  say("\nprepush: interrupted");
  process.exit(130);
});

function record(r: Result) {
  results.push(r);
  if (r.status === "fail") failed = true;
  const mark = { pass: "ok  ", warn: "WARN", fail: "FAIL", skip: "skip", "n/a": "n/a " }[r.status];
  say(`${mark} ${r.id.padEnd(22)} ${r.status === "skip" || r.status === "n/a" ? "" : fmtSecs(r.secs).padStart(7)}  ${r.note}`);
}

function fmtSecs(s: number): string {
  return s < 90 ? `${s.toFixed(0)}s` : `${Math.floor(s / 60)}m${String(Math.round(s % 60)).padStart(2, "0")}s`;
}

/** A gate made of commands run one after the other; it fails at the first that fails (CI's `set -e`). */
async function gate(
  id: string,
  cmds: { cmd: string; cwd?: string; env?: Record<string, string>; label?: string }[],
  opts: { timeoutMin?: number; note?: string; where?: string } = {},
): Promise<boolean> {
  if (!wants(id)) return true;
  const t0 = Date.now();
  say(`▸ ${id}`);
  let step = "";
  for (const c of cmds) {
    step = c.label ?? c.cmd.split("\n")[0].slice(0, 90);
    const r = await run(c.cmd, {
      cwd: c.cwd,
      env: c.env,
      timeoutMin: opts.timeoutMin,
      // The CI scripts mark each step with ::group::<command>.
      onLine: (l) => {
        const m = l.match(/^::group::(.*)$/);
        if (m && m[1] !== "start") {
          step = m[1];
          console.log(`    · ${m[1].slice(0, 110)}`);
        }
      },
    });
    if (r.code !== 0) {
      const secs = (Date.now() - t0) / 1000;
      console.log(r.tail.slice(-30).map((l) => `      | ${l}`).join("\n"));
      record({ id, where: opts.where ?? "mac", status: "fail", secs, note: `${r.timedOut ? "TIMED OUT in" : "failed at"}: ${step.slice(0, 100)}` });
      return false;
    }
  }
  record({ id, where: opts.where ?? "mac", status: "pass", secs: (Date.now() - t0) / 1000, note: opts.note ?? "" });
  return true;
}

function skip(id: string, why: string, where = "mac", status: Status = "skip") {
  if (!wants(id)) return;
  if (status === "skip" && /requested|--no-vm|VM/.test(why)) partial = true;
  record({ id, where, status, secs: 0, note: why });
}

// ── ci.yml, read ───────────────────────────────────────────────────────────────────────────────────────────────────────

interface Step {
  name?: string;
  run?: string;
  uses?: string;
  if?: string;
  "working-directory"?: string;
  env?: Record<string, string>;
}
const ci = Bun.YAML.parse(readFileSync(join(root, ".github/workflows/ci.yml"), "utf8")) as {
  jobs: Record<string, { steps?: Step[]; env?: Record<string, string> }>;
};
const steps = (job: string) => (ci.jobs[job]?.steps ?? []).filter((s) => s.run);
const stepNamed = (job: string, re: RegExp) => steps(job).find((s) => re.test(s.name ?? s.run ?? ""));
const asCmd = (s: Step) => ({ cmd: s.run!, cwd: s["working-directory"] ? join(root, s["working-directory"]) : root, env: s.env, label: s.name ?? s.run!.split("\n")[0] });

/**
 * How each CI job is covered here. A job missing from this table fails the `ci-drift` gate: when ci.yml grows a job, say here
 * what of it this Mac runs.
 */
const COVERAGE: Record<string, string> = {
  gate: "nothing to run (it only starts the others)",
  check: "check.sh unchanged (macOS); clippy, tests and os.sh in the Linux arm64 VM; x86_64 Linux not covered",
  os: "os.sh on macOS and in the Linux arm64 VM; Windows: type-check (xwin) and the path lint only; x86_64 Linux not covered",
  modules: "modules.sh aarch64-apple-darwin, and aarch64-unknown-linux-gnu in the VM; x86_64 Linux not covered",
  windows: "cargo xwin check --target x86_64-pc-windows-msvc, the same command",
  macos: "its steps as written: the app's lint, test, weight, bundle (ad hoc), screens and idle benches, then check/os/modules.sh",
  weight: "sizes estimated from macOS builds (tools/prepush/weight.ts), packages exact; idle memory not covered (runner-dependent)",
  "weight-doze": "the doze size estimated from a build in the Linux arm64 VM (macOS as fallback), packages exact; the status timing is CI-only",
  "weight-lean": "the lean size estimated from a macOS build, packages exact; idle memory not covered",
  "weight-tls-idle": "not covered: idle memory with TLS on CI's runner",
  web: "its steps as written",
  "console-check": "its steps as written",
  "console-e2e": "its steps as written, the three browsers one after the other",
  "supply-chain": "cargo deny check (the local cargo-deny's version, not the action's)",
  fuzz: "its steps as written for the crates the diff reaches, --fuzz-seconds each (CI: 30)",
  "all-green": "this summary",
};

// ── The gates ──────────────────────────────────────────────────────────────────────────────────────────────────────────

async function main() {
  if (!flag("--no-fetch")) {
    try {
      sh("git", ["fetch", "-q", "origin", "main"]);
    } catch (e) {
      say(`prepush: could not fetch origin main (${String(e).split("\n")[0]}); comparing with the origin/main already here`);
    }
  }
  const p = plan(BASE, WITH);
  const sel = (s: string) => p.suites.get(s) ?? null;
  say(`prepush ${HEAD.slice(0, 9)}${dirty ? " + uncommitted changes" : ""} against ${BASE} (${p.base.slice(0, 9)}): ${p.changed.length} files changed`);
  say(`  packages touched: ${p.touched.join(", ") || "none"}; reverse-dependency closure: ${p.closure.size} packages`);
  if (p.global) say(`  everything selected: ${p.global} is outside every package`);
  for (const [s, why] of p.suites) say(`  ${s.padEnd(14)} ${why ? `runs (${why})` : "skipped: nothing it covers changed"}`);
  const fuzzSteps = steps("fuzz").filter((s) => {
    const crates = fuzzCrates(`${s["working-directory"] ?? ""}\n${s.run}`);
    return p.global || crates.some((c) => p.closure.has(c)) || WITH.includes("fuzz");
  });
  say(`  fuzz           ${fuzzSteps.length} of ${steps("fuzz").length - 0} steps (${FUZZ_SECONDS} s a target; CI 30 s)`);
  say(`  proof log      ${LOG}`);
  if (flag("--plan")) return;

  // CI jobs this tool does not know about.
  const unknown = Object.keys(ci.jobs).filter((j) => !(j in COVERAGE));
  if (wants("ci-drift")) {
    record({
      id: "ci-drift",
      where: "mac",
      status: unknown.length ? "fail" : "pass",
      secs: 0,
      note: unknown.length ? `ci.yml jobs prepush does not cover: ${unknown.join(", ")} (add them to COVERAGE in tools/prepush/prepush.ts)` : `all ${Object.keys(ci.jobs).length} ci.yml jobs mapped`,
    });
  }
  const stop = () => failed && flag("--fail-fast");

  // The path lint: cheap, first.
  if (wants("path-lint")) {
    const t0 = Date.now();
    const r = await run(`bun tools/prepush/path-lint.ts --base ${p.base}`);
    const m = r.tail.join("\n").match(/path-lint: (\d+) errors, (\d+) warnings/);
    const warns = m ? Number(m[2]) : 0;
    if (warns || r.code) console.log(r.tail.filter((l) => !l.startsWith("path-lint:")).map((l) => `      | ${l}`).join("\n"));
    record({ id: "path-lint", where: "mac", status: r.code ? "fail" : warns ? "warn" : "pass", secs: (Date.now() - t0) / 1000, note: m ? `${m[1]} errors, ${m[2]} warnings (Windows path text, added lines only)` : "did not run" });
  }

  // CI's check job, unchanged.
  if (!stop()) await gate("check.sh", [{ cmd: ".github/ci/check.sh" }], { timeoutMin: 150 });

  // The macos job's clippy runs (the app and doze-os's `ui` feature): always, as every feature set CI lints.
  const lints = steps("macos").filter((s) => s.run!.startsWith("cargo clippy"));
  if (!stop()) await gate("app-lint", lints.map(asCmd), { timeoutMin: 60 });

  // Windows: the `windows` job's own command, cross-checked with cargo-xwin (MSVC headers and libraries downloaded once).
  if (!stop()) {
    const winCmd = ci.jobs.windows?.steps?.find((s) => s.run?.startsWith("cargo check"))?.run ?? "cargo check --workspace --exclude doze-app --all-targets";
    if (!hasCmd("cargo-xwin")) {
      skip("windows-check", "NOT RUN: cargo-xwin is not installed (cargo install cargo-xwin --locked); Windows is unchecked here");
      partial = true;
    } else {
      await gate("windows-check", [{ cmd: `${winCmd.replace(/^cargo check/, "cargo xwin check")} --target x86_64-pc-windows-msvc`, env: { XWIN_ACCEPT_LICENSE: "true" } }], { timeoutMin: 60, note: "x86_64-pc-windows-msvc type-check (no tests: those need Windows)" });
    }
  }

  // os.sh and modules.sh (the macos job runs both).
  if (!stop()) {
    if (sel("os")) await gate("os.sh", [{ cmd: ".github/ci/os.sh" }], { timeoutMin: 90 });
    else skip("os.sh", "nothing it covers changed");
  }
  if (!stop()) {
    if (sel("modules")) {
      const token = ghToken();
      const shm = shmShortage();
      if (!token) skip("modules.sh", "NOT RUN: no GitHub token (gh auth login) for doze-binaries' engine downloads", "mac", "fail");
      else if (shm) skip("modules.sh", `NOT RUN: ${shm}`, "mac", "fail");
      else {
        // modules.sh downloads every engine again each run (--clobber), so its folder is not kept.
        const runner = join(TARGET, "prepush/runner");
        await gate("modules.sh", [{ cmd: ".github/ci/modules.sh aarch64-apple-darwin", env: { GH_TOKEN: token, RUNNER_TEMP: runner } }], { timeoutMin: 60 });
        rmSync(runner, { recursive: true, force: true });
      }
    } else skip("modules.sh", "nothing it covers changed");
  }

  // The desktop app: the macos job's steps after the lint, as written.
  if (!stop()) {
    if (sel("app")) await appGate();
    else skip("app", "nothing it covers changed");
  }

  // Weight.
  for (const spec of SPECS) {
    if (stop()) break;
    const suite = spec.id === "doze" ? "weight-doze" : spec.id === "doze-aws" ? "weight-aws" : "weight-kafka";
    const id = `weight ${spec.id}`;
    if (sel(suite)) await weightGate(id, spec);
    else skip(id, "nothing it covers changed");
  }
  if (sel("weight-kafka") || sel("weight-aws")) skip("weight idle memory", "idle RSS lines are measured on CI's runner; this Mac and the VM do not predict them", "-", "n/a");

  // The consoles.
  if (!stop()) {
    if (sel("web")) await gate("web", steps("web").map(asCmd), { timeoutMin: 30 });
    else skip("web", "nothing under web/ or kafka-console changed");
  }
  if (!stop()) {
    if (sel("console-check")) await gate("console-check", steps("console-check").map(asCmd), { timeoutMin: 30 });
    else skip("console-check", "nothing under console/ or aws-console changed");
  }
  if (!stop()) {
    if (sel("console-e2e")) await e2eGate();
    else skip("console-e2e", "nothing under console/, aws-console or doze-aws's dependencies changed");
  }

  // Supply chain: always (advisories change without the diff).
  if (!stop()) {
    if (hasCmd("cargo-deny")) await gate("supply-chain", [{ cmd: "cargo deny --version && cargo deny check" }], { timeoutMin: 20, note: "local cargo-deny" });
    else skip("supply-chain", "NOT RUN: cargo-deny not installed (cargo install cargo-deny --locked)", "mac", "fail");
  }

  // Fuzz: the steps whose crates the diff reaches.
  if (!stop()) {
    if (fuzzSteps.length === 0) skip("fuzz", "no fuzzed crate is reached by the diff");
    else {
      const cmds = fuzzSteps.map((s) => ({ ...asCmd(s), cmd: s.run!.replaceAll("-max_total_time=30", `-max_total_time=${FUZZ_SECONDS}`), env: { RUSTUP_TOOLCHAIN: "nightly" } }));
      const before = new Set(sh("git", ["status", "--porcelain", "--untracked-files=all"]).split("\n"));
      await gate("fuzz", cmds, { timeoutMin: 120, note: `${fuzzSteps.length} steps, ${FUZZ_SECONDS} s a target (CI 30 s)` });
      restoreFuzzLeftovers(before);
    }
  }

  // Linux, in the VM, after everything on the Mac.
  if (!stop()) await linuxGates(p);

  // What this Mac cannot run.
  skip("windows os.sh", "tests on windows-latest and windows-11-arm (incl. the elevated names setup test) need Windows", "-", "n/a");
  skip("x86_64 linux", "check/os/modules on ubuntu x86_64: the VM is arm64; x86_64-only failures show only in CI", "-", "n/a");
  skip("signing", "Developer ID signing and notarization need the secrets: the bundle here is ad hoc", "-", "n/a");
}

/**
 * macOS gives postgres 32 SysV shared memory segments (kern.sysv.shmmni), and a postgres that is killed leaves its segment
 * behind; once they are gone every initdb fails with "No space left on device" (seen here 2026-11-27: 27 of 32 leaked). Says so
 * before modules.sh spends minutes failing, with the cleanup that touches only segments nothing is attached to.
 */
function shmShortage(): string | null {
  if (process.platform !== "darwin") return null;
  try {
    const max = Number(sh("sysctl", ["-n", "kern.sysv.shmmni"]));
    const rows = sh("ipcs", ["-mo"]).split("\n").filter((l) => l.startsWith("m "));
    const unattached = rows.filter((l) => l.trim().split(/\s+/)[6] === "0").length;
    if (max - rows.length >= 6) return null;
    return `SysV shared memory is nearly full on this Mac (${rows.length} of ${max} segments, ${unattached} with nothing attached: leaks from killed postgres processes), so postgres cannot start. Not the code. To free the unattached ones: ipcs -mo | awk '$1=="m" && $7==0 {print $2}' | xargs -n1 ipcrm -m`;
  } catch {
    return null;
  }
}

/**
 * cargo fuzz rewrites a fuzz crate's Cargo.lock when it is stale (eight were, 2026-11-27) and a crate without a .gitignore for
 * corpus/ leaves its corpus in the tree. CI's runner throws both away; here they would dirty the tree `just push` checks, so what
 * the fuzz gate changed under crates/<name>/fuzz is put back (said in the log; the stale lockfiles are worth a commit of their own).
 */
function restoreFuzzLeftovers(before: Set<string>) {
  const after = sh("git", ["status", "--porcelain", "--untracked-files=all"]).split("\n").filter(Boolean);
  const fresh = after.filter((l) => !before.has(l) && /^.. crates\/[^/]+\/fuzz\//.test(l));
  const modified = fresh.filter((l) => l.startsWith(" M")).map((l) => l.slice(3));
  const untracked = fresh.filter((l) => l.startsWith("??")).map((l) => l.slice(3));
  if (modified.length) sh("git", ["checkout", "--", ...modified]);
  for (const u of untracked) rmSync(join(root, u), { force: true });
  if (fresh.length) say(`  fuzz: put back ${modified.length} changed files (stale fuzz lockfiles: ${modified.join(", ") || "none"}) and ${untracked.length} new corpus files`);
}

function hasCmd(c: string): boolean {
  try {
    execFileSync("bash", ["-c", `command -v ${c}`], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

function ghToken(): string {
  if (process.env.GH_TOKEN) return process.env.GH_TOKEN;
  try {
    return execFileSync("gh", ["auth", "token"], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();
  } catch {
    return "";
  }
}

// ── The app ────────────────────────────────────────────────────────────────────────────────────────────────────────────

async function appGate() {
  // The steps write shots/ and bundle/ into the current folder: give them one under target/, with target/ reachable.
  const work = join(TARGET, "prepush/app");
  rmSync(work, { recursive: true, force: true });
  mkdirSync(work, { recursive: true });
  symlinkSync(TARGET, join(work, "target"));
  const cmds = [];
  for (const s of steps("macos")) {
    const r = s.run!;
    if (r.startsWith("cargo clippy") || s.name?.startsWith("Metal") || /check\.sh|os\.sh|modules\.sh/.test(r)) continue;
    if (s.name?.startsWith("Signing identity")) continue; // the secrets' keychain: never on a person's Mac
    if (s.name === "Doze.app") {
      cmds.push({ cmd: `cargo xtask app-bundle --no-build --id dev.doze.app --version 0.0.0 --out ${join(work, "bundle")} && codesign -dv ${join(work, "bundle/Doze.app")}`, label: "Doze.app (ad hoc)" });
      continue;
    }
    // A step that names target/ runs in the work folder; cargo xtask needs the repository.
    const inWork = /target\/|shots/.test(r) && !r.includes("cargo xtask");
    cmds.push({ ...asCmd(s), cwd: inWork ? work : root, env: { GITHUB_WORKSPACE: root } });
  }
  await gate("app", cmds, { timeoutMin: 90, note: `${cmds.length} macos-job steps; shots in ${join(work, "shots")}` });
}

// ── Weight ─────────────────────────────────────────────────────────────────────────────────────────────────────────────

function lines(): Record<string, { bytes: number; packages: number }> {
  const limits = readFileSync(join(root, "xtask/weight-limits.txt"), "utf8");
  const lim = (pkg: string) => {
    const m = limits.match(new RegExp(`^${pkg} (\\d+) (\\d+)`, "m"));
    if (!m) throw new Error(`xtask/weight-limits.txt has no ${pkg} line`);
    return { bytes: Number(m[1]), packages: Number(m[2]) };
  };
  const block = (job: string, re: RegExp) => steps(job).map((s) => s.run!).find((r) => re.test(r)) ?? "";
  const num = (text: string, re: RegExp) => {
    const m = text.match(re);
    if (!m) throw new Error(`ci.yml: ${re} not found`);
    return Number(m[1]);
  };
  const dozeBlock = block("weight-doze", /release-doze/);
  const leanBlock = block("weight-lean", /lean binary over/);
  const doze = lim("doze-cli");
  return {
    doze: {
      bytes: Math.min(doze.bytes, num(dozeBlock, /"\$size" -le (\d+)/)),
      packages: Math.min(doze.packages, num(dozeBlock, /"\$crates" -le (\d+)/)),
    },
    "doze-kafka": lim("doze-kafka"),
    "doze-aws": lim("doze-aws"),
    "kafka-lean": { bytes: num(leanBlock, /"\$size" -le (\d+)/), packages: num(leanBlock, /"\$crates" -le (\d+)/) },
  };
}

async function weightGate(id: string, spec: (typeof SPECS)[number]) {
  if (!wants(id)) return;
  const t0 = Date.now();
  say(`▸ ${id}`);
  const line = lines()[spec.id];
  const feats = spec.features.join(" ");
  const r = await run(`cargo build --locked --profile ${spec.profile} -p ${spec.package} ${feats}`, { timeoutMin: 45 });
  if (r.code !== 0) {
    console.log(r.tail.slice(-20).map((l) => `      | ${l}`).join("\n"));
    record({ id, where: "mac", status: "fail", secs: (Date.now() - t0) / 1000, note: "release build failed" });
    return;
  }
  const bytes = statSync(join(TARGET, spec.profile, spec.binary)).size;
  const tree = sh("cargo", ["tree", "-p", spec.package, ...spec.features, "--prefix", "none", "--edges", "normal", "--target", "x86_64-unknown-linux-gnu"]);
  const packages = new Set(tree.split("\n").filter(Boolean)).size;
  let { status: v, note } = sizeVerdict("macOS", bytes, spec.ratio, spec.margin, line.bytes);
  // The VM judges this binary (its ratio is the steadier one): here a fail only warns, unless the VM cannot run.
  const vmDecides = spec.vm && vmRunning() && !flag("--no-vm");
  if (vmDecides && v === "fail") {
    v = "warn";
    note += "; the VM's estimate decides (linux-weight-doze)";
  }
  if (packages > line.packages) v = "fail";
  note += `; ${packages}/${line.packages} packages`;
  log(`weight ${spec.id}: ${note}`);
  record({ id, where: "mac", status: v, secs: (Date.now() - t0) / 1000, note });
}

const n = (x: number) => x.toLocaleString("en-US");
function sizeVerdict(where: string, bytes: number, ratio: number, margin: number, line: number): { status: Status; note: string } {
  const est = Math.round(bytes * ratio);
  return {
    status: judge(est, line, margin),
    note: `${where} ${n(bytes)} B x ${ratio} = ~${n(est)} B on CI; line ${n(line)}, fail over ${n(Math.round(line * (1 - margin)))}, warn over ${n(Math.round(line * (1 - 2 * margin)))}`,
  };
}

let vmState: string | undefined;
function vmRunning(): boolean {
  if (vmState === undefined) {
    try {
      vmState = execFileSync("limactl", ["list", VM, "--format", "{{.Status}}"], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();
    } catch {
      vmState = "";
    }
  }
  return vmState === "Running";
}

// ── The aws console's browsers ─────────────────────────────────────────────────────────────────────────────────────────

async function e2eGate() {
  const st = steps("console-e2e");
  const pick = (re: RegExp) => st.find((s) => re.test(s.run!));
  const install = pick(/^bun install/)!;
  const build = pick(/^bun run build/)!;
  const aws = pick(/^cargo build -p doze-aws/)!;
  const runs = st.filter((s) => s.run!.includes("e2e --project="));
  const prep = [asCmd(install), asCmd(build), asCmd(aws)];
  if (!(await gate("console-e2e build", prep, { timeoutMin: 30 }))) return;
  for (const browser of ["firefox", "webkit", "chromium"]) {
    if (stop2()) return;
    const cmds = [
      { cmd: `bunx --bun playwright install ${browser}`, cwd: join(root, "console/apps/aws"), label: `playwright install ${browser}` },
      // CI=true as on the runner: the configs then allow CI's one retry, so the verdict is CI's (a retried pass is printed
      // "flaky" in the log, never hidden). Without it a local crash CI would retry fails here (Firefox "Page crashed", 2026-11-27).
      ...runs.map((s) => {
        const cmd = s.run!.replaceAll("${{ matrix.browser }}", browser);
        return { ...asCmd(s), cmd, label: cmd, env: { CI: "true" } };
      }),
    ];
    await gate(`console-e2e ${browser}`, cmds, { timeoutMin: 90 });
  }
}
const stop2 = () => failed && flag("--fail-fast");

// ── Linux, in the VM ───────────────────────────────────────────────────────────────────────────────────────────────────

async function linuxGates(p: Plan) {
  const wanted: string[] = ["linux-clippy"];
  if (p.global || p.closure.size) wanted.push("linux-test");
  if (p.suites.get("os")) wanted.push("linux-os.sh");
  if (p.suites.get("modules")) wanted.push("linux-modules.sh");
  if (p.suites.get("weight-doze")) wanted.push("linux-weight-doze");
  const all = ["linux-clippy", "linux-test", "linux-os.sh", "linux-modules.sh", "linux-weight-doze"];
  const chosen = wanted.filter(wants);
  for (const g of all.filter((g) => !wanted.includes(g))) skip(g, "nothing it covers changed", "vm");
  if (chosen.length === 0) return;
  if (flag("--no-vm")) {
    for (const g of chosen) skip(g, "NOT RUN: --no-vm", "vm");
    return;
  }
  if (!vmRunning()) {
    for (const g of chosen) skip(g, `NOT RUN: the Lima VM ${VM} is ${vmState || "missing"} (limactl start ${VM}); Linux is unchecked here`, "vm");
    return;
  }
  // The working tree (tracked and untracked, not ignored) goes to ~/prepush/src in the VM; rsync keeps unchanged files' times,
  // so the VM's own target (~/prepush/target) builds incrementally.
  const t0 = Date.now();
  say(`▸ linux (${VM}): sending the tree`);
  const tgz = join(tmpdir(), `prepush-${process.pid}.tgz`);
  const sent = await run(`git ls-files -z -co --exclude-standard | COPYFILE_DISABLE=1 tar --no-mac-metadata --no-xattrs -czf ${tgz} --null -T - && limactl copy ${tgz} ${VM}:/tmp/prepush-src.tgz`, { timeoutMin: 10 });
  rmSync(tgz, { force: true });
  if (sent.code !== 0) {
    for (const g of chosen) record({ id: g, where: "vm", status: "fail", secs: 0, note: "could not copy the tree into the VM" });
    return;
  }
  const guest = readFileSync(join(root, "tools/prepush/linux.sh"), "utf8");
  const token = chosen.includes("linux-modules.sh") ? ghToken() : "";
  let gateStart = Date.now();
  // The guest script reads the token from its first stdin line and its own text after it.
  const r = await run(`limactl shell ${VM} -- bash -c 'IFS= read -r GH_TOKEN; export GH_TOKEN; f=$(mktemp); cat > "$f"; bash "$f" ${chosen.join(" ")}; rc=$?; rm -f "$f"; exit $rc'`, {
    stdin: `${token}\n${guest}`,
    timeoutMin: 240,
    onLine: (l) => {
      const m = l.match(/^@@prepush (\S+) (start|pass|fail) ?(.*)$/);
      if (!m) return;
      if (m[2] === "start") {
        gateStart = Date.now();
        say(`▸ ${m[1]} (${VM})`);
      } else if (m[1] === "linux-weight-doze" && m[2] === "pass") {
        // The guest says the bytes; the line and margin are judged here (tools/prepush/weight.ts).
        const spec = SPECS.find((s) => s.id === "doze")!;
        const bytes = Number(m[3].match(/bytes=(\d+)/)?.[1] ?? NaN);
        const v = Number.isFinite(bytes) ? sizeVerdict("VM", bytes, spec.vm!.ratio, spec.vm!.margin, lines().doze.bytes) : { status: "fail" as Status, note: "no size reported" };
        record({ id: m[1], where: "vm", status: v.status, secs: (Date.now() - gateStart) / 1000, note: v.note });
      } else record({ id: m[1], where: "vm", status: m[2] as Status, secs: (Date.now() - gateStart) / 1000, note: m[3] });
    },
  });
  for (const g of chosen) {
    if (!results.some((x) => x.id === g)) record({ id: g, where: "vm", status: "fail", secs: 0, note: `did not report (the VM run exited ${r.code}${r.timedOut ? ", timed out" : ""})` });
  }
  const disk = r.tail.find((l) => l.startsWith("@@prepush-disk"));
  if (disk) say(`  VM disk: ${disk.replace("@@prepush-disk ", "")} (just prepush-clean empties it)`);
  log(`linux total ${fmtSecs((Date.now() - t0) / 1000)}`);
}

// ── Run, summarise ─────────────────────────────────────────────────────────────────────────────────────────────────────

await main();
if (!flag("--plan")) {
  const total = (Date.now() - started) / 1000;
  say("");
  say(`${"gate".padEnd(24)}${"where".padEnd(6)}${"result".padEnd(8)}${"time".padStart(7)}  note`);
  for (const r of results) {
    say(`${r.id.padEnd(24)}${r.where.padEnd(6)}${(r.status === "skip" ? "skipped" : r.status === "n/a" ? "n/a here" : r.status).padEnd(8)}${(r.status === "skip" || r.status === "n/a" ? "" : fmtSecs(r.secs)).padStart(7)}  ${r.note}`);
  }
  const ranAll = !ONLY && !partial;
  const ciRef = sh("git", ["rev-parse", "--abbrev-ref", "HEAD"]);
  const ciText = readFileSync(join(root, ".github/workflows/ci.yml"), "utf8");
  say("");
  if (ciText.includes("windows-os")) {
    say(`Windows tests run only in CI. Before the full run, the os job on Windows alone (all-green stays red):`);
    say(`  gh workflow run ci --ref ${ciRef} -f only=windows-os`);
  }
  if (dirty) say(`NOTE: the tree has uncommitted changes, so this run is for the working tree, not ${HEAD.slice(0, 9)}.`);
  const verdict = failed ? "FAILED" : ranAll ? "PASSED" : "PARTIAL (not a pass: some gates were not run)";
  say(`prepush ${verdict} in ${fmtSecs(total)} for ${HEAD.slice(0, 9)}; proof log: ${LOG}`);
  process.exit(failed ? 1 : ranAll ? 0 : 3);
}

// What every step of the machine test shares: the run's names and folders, the doze it drives, and the two ways it
// changes the machine (doze's own privileged commands, behind the safety scan, and the few root commands of its own
// cleanup), both of which only print under --dry-run.

import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { type Args, extractScript, type Names, type Os, type Policy, type Result, scanScript, type Status } from "./lib";
import { fileFingerprint, type Ran, run, type RunOpts, say, shq } from "./sys";

export type Ctx = {
  os: Os;
  args: Args;
  repo: string;
  doze: string;
  bun: string;
  /** Short on purpose: a daemon's socket path must fit in 104 bytes. */
  work: string;
  home: string;
  stack: string;
  names: Names;
  /** Where the stack's https front door listens (the URL's port), and the address it is reached on. */
  tls: { bind: string; port: number };
  env: Record<string, string | undefined>;
  /** Every root the run made (current and rotated-out): the only ones a script may untrust, and the cleanup's list. */
  fingerprints: string[];
  /** Every pid the run's stacks wrote (daemons and backends, across restarts): none may outlive the run. */
  pids: number[];
  /** The doze roots the keychain held before the run: the owner's, which must all be there after it. */
  protectedRoots: string[];
  results: Result[];
  /** --with-zone: the owner's zone files, kept (a tar under work) and which of them existed. */
  zone: { tar: string; existed: string[]; installed: boolean };
};

export class Abort extends Error {}

export const caDir = (c: Ctx) => join(c.home, "ca");
export const rootCrt = (c: Ctx) => join(caDir(c), "root.crt");
export const url = (c: Ctx) => `https://${c.names.domain}:${c.tls.port}/`;
export const expectBody = (c: Ctx) => `machine-test ${c.names.id} ok`;

/** doze, with the run's private home and zone. */
export function dz(c: Ctx, argv: string[], o: RunOpts = {}): Promise<Ran> {
  return run([c.doze, ...argv], { cwd: c.stack, ...o, env: { ...c.env, ...o.env } });
}

export function policy(c: Ctx, zone = false): Policy {
  // On Linux every route is a line in the zone unit's drop-in, so with the zone the test's own (--with-zone, required
  // there) every doze script touches it.
  const ours = zone || (c.os === "linux" && c.args.withZone);
  return { os: c.os, home: c.home, fingerprints: c.fingerprints, domains: [c.names.domain], zone: ours };
}

/** Keeps a copy of the current root (the cleanup untrusts by file and fingerprint) and adds it to the run's own. */
export function adoptRoot(c: Ctx): string | null {
  const fp = fileFingerprint(rootCrt(c));
  if (!fp) return null;
  if (!c.fingerprints.includes(fp)) c.fingerprints.push(fp);
  mkdirSync(join(c.work, "roots"), { recursive: true });
  copyFileSync(rootCrt(c), join(c.work, "roots", `${fp}.pem`));
  return fp;
}

/** The rotated-out fingerprints the authority keeps (previous.json), which a trust run untrusts. */
export function previousFingerprints(c: Ctx): string[] {
  try {
    const v = JSON.parse(readFileSync(join(caDir(c), "previous.json"), "utf8"));
    return Array.isArray(v) ? v.map(String).map((s) => s.toLowerCase()) : [];
  } catch {
    return [];
  }
}

/** A privileged doze command, only after its `--dry-run` script passed the safety scan; under --dry-run the script is
 *  printed and nothing runs. The touches are returned (what the script would change). */
export async function guarded(
  c: Ctx,
  argv: string[],
  o: { zone?: boolean; env?: Record<string, string> } = {},
): Promise<{ touches: string[]; ran: Ran | null }> {
  const dry = await dz(c, [...argv, "--dry-run"], { env: o.env });
  if (dry.code !== 0) throw new Abort(`doze ${argv.join(" ")} --dry-run failed (exit ${dry.code}): ${dry.err.trim()}`);
  const script = extractScript(dry.out);
  const scan = scanScript(script, policy(c, o.zone));
  if (scan.violations.length) {
    for (const v of scan.violations) say(`    ✗ refused: ${v}`);
    throw new Abort(`doze ${argv.join(" ")} would change what the test does not own; nothing was run`);
  }
  if (!script) return { touches: [], ran: null };
  if (c.args.dryRun) {
    say(`    [dry-run] doze ${argv.join(" ")} would run (the sudo part as root, a DOZE_AS_YOU part as you):`);
    for (const l of script.split("\n")) say(`      ${l}`);
    return { touches: scan.touches, ran: null };
  }
  const ran = await dz(c, argv, { tty: true, env: o.env });
  if (ran.code !== 0) throw new Abort(`doze ${argv.join(" ")} failed (exit ${ran.code}): ${(ran.err || ran.out).trim()}`);
  return { touches: scan.touches, ran };
}

/** One of the run's own root commands (cleanup and the --with-zone backup): printed under --dry-run. */
export async function root(c: Ctx, cmd: string[]): Promise<Ran | null> {
  if (c.args.dryRun) {
    say(`    [dry-run] would run as root: ${cmd.map(shq).join(" ")}`);
    return null;
  }
  return run(["sudo", "-n", ...cmd]);
}

/** The pids the run's stacks wrote (daemon and backends), from their run folders under the private home. */
export function stackPids(c: Ctx): { file: string; pid: number }[] {
  const out: { file: string; pid: number }[] = [];
  const projects = join(c.home, "projects");
  if (!existsSync(projects)) return out;
  for (const p of readdirSync(projects)) {
    const runDir = join(projects, p, "run");
    if (!existsSync(runDir)) continue;
    for (const f of readdirSync(runDir).filter((f) => f.endsWith(".pid"))) {
      const pid = Number(readFileSync(join(runDir, f), "utf8").trim());
      if (Number.isInteger(pid) && pid > 1) out.push({ file: join(p, "run", f), pid });
    }
  }
  return out;
}

/** Adds the pids the stacks wrote now to the run's list. */
export function remember(c: Ctx): void {
  for (const p of stackPids(c)) if (!c.pids.includes(p.pid)) c.pids.push(p.pid);
}

export function daemonPid(c: Ctx): number | null {
  return stackPids(c).find((p) => p.file.endsWith("/doze.pid"))?.pid ?? null;
}

/** Runs one scenario: a pass/fail line with its time on the screen and in the log. A step that throws fails it; an
 *  Abort also stops the run (the cleanup still runs). */
export async function scenario(
  c: Ctx,
  id: string,
  title: string,
  body: () => Promise<{ status: Status; detail: string }>,
): Promise<void> {
  say();
  say(`── ${id}. ${title}`);
  const start = performance.now();
  let r: { status: Status; detail: string };
  let abort: Abort | null = null;
  try {
    r = await body();
  } catch (e) {
    if (e instanceof Abort) abort = e;
    r = { status: "FAIL", detail: e instanceof Error ? e.message : String(e) };
  }
  const ms = performance.now() - start;
  c.results.push({ id, title, ms, ...r });
  say(`   ${r.status.padEnd(5)} ${(ms / 1000).toFixed(1)}s  ${r.detail}`);
  if (abort) throw abort;
}

/** A check inside a scenario: said, and collected so the scenario passes only if every one did. */
export class Checks {
  private failed: string[] = [];
  private passed: string[] = [];
  ok(cond: boolean, what: string, why = ""): boolean {
    say(`   ${cond ? "✓" : "✗"} ${what}${!cond && why ? ` · ${why}` : ""}`);
    (cond ? this.passed : this.failed).push(what);
    return cond;
  }
  result(): { status: Status; detail: string } {
    return this.failed.length
      ? { status: "FAIL", detail: `failed: ${this.failed.join("; ")}` }
      : { status: "PASS", detail: `${this.passed.length} checks` };
  }
}

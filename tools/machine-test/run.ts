// The machine test: doze's networking and trust on the real machine (the keychain, /etc/resolver, mDNSResponder,
// launchd; systemd-resolved and the CA bundle on Linux), which the unit tests only see as the scripts doze writes.
//
//   just machine-test [--dry-run] [--proof-dir DIR] [--with-zone] [--app-prompt] [--wait S] [--doze BIN] [--keep]
//
// Run it as yourself. It asks for sudo once (sudo -v, kept alive for the run); macOS also shows its own dialog when a
// CA's trust changes. Everything it changes it undoes at the end, also on failure or Ctrl-C.
//
// Isolation: a private DOZE_HOME under /tmp with its own CA, a stack and a domain named for the run
// (<id>.doze-machine-test.dev), and a private zone (its resolver and front door on ports of their own). Before every
// privileged doze command its --dry-run script is scanned (lib.ts scanScript): a script that would trust, untrust or
// delete a certificate not the test's, or change a route or the zone's setup the test does not own, is refused and
// nothing runs. Your ~/.doze, your stacks and your doze CA are never touched; scenario 7 proves it by comparing the
// System keychain, the admin trust settings, /etc/resolver and the zone files before and after.
//
// --with-zone also installs the zone itself (doze dns-setup: the dev.doze.netd job and /etc/resolver/doze on macOS,
// the units on Linux) for the test home, keeping your copies and putting them back. While it runs your .doze names do
// not answer. Linux requires it: the zone's resolver has one fixed address there (192.0.2.53:5323), held by the
// daemon of whichever home set it up, so a private home's domains resolve only once its own zone is installed.
//
// --app-prompt runs the rotation (scenario 5) through the system's administrator prompt, as the app does
// (DOZE_ADMIN_PROMPT=1): the path that failed with "no user interaction was possible".
//
// --dry-run runs nothing as root and changes nothing outside the private home: the stack still comes up (unprivileged,
// on its own ports), so every privileged script printed is the one doze would really run, scanned as for a real run.
//
// The scenarios: 1 setup from scratch (CA made and trusted; Foundation and curl believe it), 2 a declared domain
// resolves through the system resolver with no manual HUP and serves trusted https, 3 doze trust --remove takes the
// route and the name away, 4 setup again plans nothing destructive and keeps the CA, 5 doze trust --rotate (new CA
// trusted, old untrusted, leaves reissued), 6 doze down -c <dir> (expected to fail: issue #186), 7 nothing is left.

import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { createSocket } from "node:dgram";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { Abort, type Ctx, dz, remember, root, stackPids } from "./ctx";
import { failed, isTestDomain, judgeDiff, parseArgs, proofFileName, randomLabel, summaryTable, testNames } from "./lib";
import { restoreZone, s1Setup, s2Domain, s3Remove, s4Again, s5Rotate, s6DownC, zoneLinux, zoneMac } from "./scenarios";
import { alive, keychainFingerprints, proofText, rootTrusted, run, say, sleep, snapshot, SYSTEM_KEYCHAIN } from "./sys";

const repo = resolve(import.meta.dir, "../..");

async function freePort(): Promise<number> {
  for (;;) {
    const port = await new Promise<number>((ok, no) => {
      const s = createServer();
      s.once("error", no);
      s.listen(0, "127.0.0.1", () => {
        const p = (s.address() as { port: number }).port;
        s.close(() => ok(p));
      });
    });
    const udpFree = await new Promise<boolean>((ok) => {
      const u = createSocket("udp4");
      u.once("error", () => ok(false));
      u.bind(port, "127.0.0.1", () => u.close(() => ok(true)));
    });
    if (udpFree) return port;
  }
}

function usage(): string {
  return `usage: just machine-test [--dry-run] [--proof-dir DIR] [--with-zone] [--app-prompt] [--wait S] [--doze BIN] [--keep]

  --dry-run      run nothing as root; print every privileged command (the stack still comes up, privately)
  --proof-dir    where the proof log goes (default: a folder under ${tmpdir()})
  --with-zone    also install the zone for the test home, keeping yours and putting it back (required on Linux)
  --app-prompt   rotate through the system's administrator prompt, as the app does (macOS)
  --wait S       how long a name, route or leaf may take to change (default 30)
  --doze BIN     the doze to test (default: cargo build -p doze-cli, then target/debug/doze)
  --keep         keep the work folder (the test CA's files are deleted all the same)`;
}

async function main(): Promise<number> {
  let args;
  try {
    args = parseArgs(process.argv.slice(2));
  } catch (e) {
    console.error(`${(e as Error).message}\n\n${usage()}`);
    return 2;
  }
  if (args.help) {
    console.log(usage());
    return 0;
  }
  const os = process.platform === "darwin" ? "mac" : process.platform === "linux" ? "linux" : null;
  if (!os) return refuse("macOS and Linux only");
  if (process.getuid?.() === 0) return refuse("run it as yourself, not under sudo: it asks for sudo itself");
  if (/^(off|0|false|no)$/i.test(process.env.DOZE_ZONE?.trim() ?? "")) {
    return refuse("DOZE_ZONE is off: with the zone off, doze trust --remove untrusts every doze CA on the machine");
  }
  if (os === "linux" && !args.withZone && !args.dryRun) {
    return refuse("Linux needs --with-zone: the zone's resolver has one fixed address, so the test home needs its own zone");
  }
  if (args.appPrompt && os !== "mac") return refuse("--app-prompt is macOS's administrator prompt");

  const sha = (await run(["git", "-C", repo, "rev-parse", "HEAD"])).out.trim() || "unknown";
  const dirty = (await run(["git", "-C", repo, "status", "--porcelain"])).out.trim() !== "";
  let doze = args.doze;
  if (!doze) {
    say("==> building doze (cargo build -p doze-cli)");
    const b = await run(["cargo", "build", "-q", "-p", "doze-cli"], { cwd: repo, show: true });
    if (b.code !== 0) return refuse("the build failed");
    doze = join(process.env.CARGO_TARGET_DIR ?? join(repo, "target"), "debug", "doze");
  }
  doze = resolve(doze);

  const work = mkdtempSync(join("/tmp", "dzmt."));
  const id = randomLabel(crypto.getRandomValues(new Uint8Array(8)));
  const names = testNames(id);
  const bindHost = os === "mac" ? "127.0.0.1" : "0.0.0.0";
  const [plain, tls, resolver] = [await freePort(), await freePort(), await freePort()];
  const home = join(work, "home");
  const stack = join(work, "stack");
  mkdirSync(home, { recursive: true });
  mkdirSync(stack, { recursive: true });
  const c: Ctx = {
    os,
    args,
    repo,
    doze,
    bun: process.execPath,
    work,
    home,
    stack,
    names,
    tls: { bind: `${bindHost}:${tls}`, port: tls },
    env: {
      ...process.env,
      DOZE_HOME: home,
      // Linux's zone resolver has one address (see the header); macOS's goes where /etc/resolver says.
      DOZE_ZONE_RESOLVER: os === "mac" ? `127.0.0.1:${resolver}` : undefined,
      DOZE_ZONE_INGRESS: `${bindHost}:${plain}`,
      DOZE_ZONE_INGRESS_TLS: `${bindHost}:${tls}`,
      DOZE_NO_UPDATE_NOTIFIER: "1",
      NO_COLOR: "1",
      DOZE_ADMIN_PROMPT: undefined,
      DOZE_ZONE: undefined,
      DOZE_JSON: undefined,
    },
    fingerprints: [],
    protectedRoots: [],
    pids: [],
    results: [],
    zone: { tar: join(work, "zone-kept.tar"), existed: [], installed: false },
  };

  say(`doze machine test · ${os} · ${sha.slice(0, 12)}${dirty ? " (uncommitted changes)" : ""} · ${new Date().toISOString()}`);
  say(`  doze        ${doze}`);
  say(`  work        ${work} (private DOZE_HOME ${home})`);
  say(`  stack       ${names.stack}, domain ${names.domain}`);
  say(`  zone        resolver ${c.env.DOZE_ZONE_RESOLVER ?? "192.0.2.53:5323 (Linux's fixed address)"}, front door ${c.env.DOZE_ZONE_INGRESS} and ${c.env.DOZE_ZONE_INGRESS_TLS}`);
  say(`  mode        ${args.dryRun ? "DRY RUN: nothing runs as root" : "real"}${args.withZone ? ", --with-zone" : ""}${args.appPrompt ? ", --app-prompt" : ""}`);

  const before = await snapshot(os);
  if (os === "mac") {
    const doze = await run(["security", "find-certificate", "-a", "-Z", "-c", "doze local CA", SYSTEM_KEYCHAIN]);
    c.protectedRoots = [...doze.out.matchAll(/SHA-256 hash:\s*([0-9A-F]{64})/gi)].map((m) => m[1].toLowerCase());
  }
  plan(c);

  let keepalive: ReturnType<typeof setInterval> | undefined;
  if (!args.dryRun) {
    say("==> sudo, once for the run");
    if ((await run(["sudo", "-v"], { tty: true })).code !== 0) return refuse("sudo was refused; nothing was changed");
    keepalive = setInterval(() => Bun.spawn(["sudo", "-n", "-v"], { stdin: "ignore", stdout: "ignore", stderr: "ignore" }), 50_000);
  }

  let cleaned = false;
  const finish = async (): Promise<number> => {
    if (cleaned) return 1;
    cleaned = true;
    await cleanup(c);
    if (keepalive) clearInterval(keepalive);
    await verifyClean(c, before);
    return report(c, sha);
  };
  for (const sig of ["SIGINT", "SIGTERM", "SIGHUP"] as const) {
    process.on(sig, () => {
      say(`\n==> ${sig}: cleaning up`);
      void finish().then(() => process.exit(130));
    });
  }

  try {
    if (os === "linux" && args.withZone) await zoneLinux(c);
    await s1Setup(c);
    await s2Domain(c);
    await s3Remove(c);
    await s4Again(c);
    await s5Rotate(c);
    await s6DownC(c);
    if (os === "mac" && args.withZone) await zoneMac(c);
  } catch (e) {
    say(`\n==> stopped: ${(e as Error).message}${e instanceof Abort ? "" : `\n${(e as Error).stack ?? ""}`}`);
    if (!(e instanceof Abort)) {
      c.results.push({ id: "!", title: "run stopped early", status: "FAIL", ms: 0, detail: (e as Error).message });
    }
  }
  return finish();
}

function refuse(why: string): number {
  console.error(`machine-test: ${why}`);
  return 2;
}

/** What the run changes as root and how each is undone, said before sudo is asked for. */
function plan(c: Ctx): void {
  say();
  say("This run changes, as root (and undoes at the end, also on failure or Ctrl-C):");
  if (c.os === "mac") {
    say(`  + /etc/resolver/${c.names.domain}   written by doze trust, removed by doze trust --remove (the cleanup removes it if left)`);
    say(`  + the test CA in ${SYSTEM_KEYCHAIN}, trusted for TLS by doze trust and replaced by doze trust --rotate;`);
    say("    every root the run made is untrusted and deleted by its fingerprint at the end");
    if (c.args.withZone) {
      say("  + the zone for the test home (doze dns-setup): the dev.doze.netd job, its daemon copy, /etc/resolver/doze;");
      say(`    your copies are kept in ${c.zone.tar} and put back. Your .doze names do not answer meanwhile.`);
    }
    say("macOS shows its own dialog when a CA's trust changes (doze trust, doze trust --rotate, and the cleanup if trust was set as you): approve it.");
  } else {
    say("  + the zone for the test home (doze dns-setup): the doze units, the daemon copy, sysctl, /etc/hosts' doze block,");
    say("    the doze0 routes and the CA in the system bundle; your copies are kept and put back, units restarted");
  }
  say("Never touched: ~/.doze, your stacks, and your doze CA" +
    (c.protectedRoots.length ? ` (${c.protectedRoots.map((f) => f.slice(0, 12)).join(", ")})` : "") +
    ": a script that would touch them is refused before it runs.");
}

/** Stops the test's stack (its own pids only), takes away its route and roots, puts the zone back, deletes the work
 *  folder. Each part runs whatever happened before it. */
async function cleanup(c: Ctx): Promise<void> {
  say();
  say("── cleanup");
  const step = async (what: string, f: () => Promise<void>) => {
    try {
      await f();
    } catch (e) {
      say(`   ✗ ${what}: ${(e as Error).message}`);
    }
  };
  await step("stopping the stack", async () => {
    remember(c);
    const pids = stackPids(c).filter((p) => alive(p.pid));
    if (!pids.length) return;
    await dz(c, ["down"], { timeoutMs: 60_000 });
    for (const p of stackPids(c).filter((p) => alive(p.pid))) {
      say(`   · ${p.file} (pid ${p.pid}) still running: SIGTERM, then SIGKILL`);
      process.kill(p.pid, "SIGTERM");
      for (let i = 0; i < 50 && alive(p.pid); i++) await sleep(100);
      if (alive(p.pid)) process.kill(p.pid, "SIGKILL");
    }
  });
  if (c.os === "mac") {
    await step("the route", async () => {
      const f = `/etc/resolver/${c.names.domain}`;
      if (existsSync(f) || c.args.dryRun) await root(c, ["rm", "-f", f]);
    });
    await step("the test's roots", async () => {
      const held = await keychainFingerprints();
      for (const fp of c.fingerprints.filter((f) => held.includes(f) || c.args.dryRun)) {
        if (c.protectedRoots.includes(fp)) continue; // never: it was there before the run
        const pem = join(c.work, "roots", `${fp}.pem`);
        await root(c, ["security", "remove-trusted-cert", "-d", pem]);
        // Trust set as the person (macOS's dialog, PR #187) may need the person to take it away: the same dialog.
        if (!c.args.dryRun && (await rootTrusted("mac", pem))) {
          say("   · the test root is still trusted: removing its trust as you (macOS asks in a dialog)");
          await run(["security", "remove-trusted-cert", "-d", pem], { tty: true });
        }
        await root(c, ["security", "delete-certificate", "-t", "-Z", fp.toUpperCase(), SYSTEM_KEYCHAIN]);
      }
    });
  }
  await step("the zone", () => restoreZone(c));
  await step("the work folder", async () => {
    rmSync(join(c.home, "ca"), { recursive: true, force: true });
    if (!c.args.keep) {
      if (existsSync(c.zone.tar)) await root(c, ["rm", "-f", c.zone.tar]);
      rmSync(c.work, { recursive: true, force: true });
    }
  });
}

/** Scenario 7: nothing of the run is left, and everything of the owner's is where it was. */
async function verifyClean(c: Ctx, before: Record<string, string[]>): Promise<void> {
  const start = performance.now();
  const after = await snapshot(c.os);
  const { problems, notes } = judgeDiff(c.os, before, after, {
    testFps: c.fingerprints,
    protectedFps: c.protectedRoots,
    zone: c.args.withZone,
  });
  if (c.os === "mac") {
    const held = await keychainFingerprints();
    for (const fp of c.fingerprints) if (held.includes(fp)) problems.push(`the test root ${fp.slice(0, 12)} is still in the keychain`);
    for (const fp of c.protectedRoots) if (!held.includes(fp)) problems.push(`your doze CA ${fp.slice(0, 12)} is gone`);
    for (const f of existsSync("/etc/resolver") ? readdirSync("/etc/resolver") : []) {
      if (isTestDomain(f)) problems.push(`left behind: /etc/resolver/${f}`);
    }
  }
  for (const pid of c.pids) if (alive(pid)) problems.push(`still running: pid ${pid}`);
  const ps = await run(["ps", "-axo", "pid=,command="]);
  for (const l of ps.out.split("\n")) {
    if (l.includes(c.work) && !l.includes("ps -axo")) problems.push(`still running: ${l.trim()}`);
  }
  say();
  say("── 7. cleanup verification: nothing of the test left, nothing of yours changed");
  for (const p of [...new Set(problems)]) say(`   ✗ ${p}`);
  for (const n of notes) say(`   · changed meanwhile, not by the test (the scan refuses such scripts): ${n}`);
  if (!problems.length) say(`   ✓ ${Object.keys(before).join(", ")}: as before; no test daemon or process left`);
  const ms = performance.now() - start;
  const status = problems.length ? "FAIL" : "PASS";
  const detail = problems.length
    ? `${problems.length} left or changed by the test`
    : `nothing left${notes.length ? `; ${notes.length} change(s) by something else meanwhile, see the log` : ""}`;
  c.results.push({ id: "7", title: "cleanup verification: before and after match", status, ms, detail });
  say(`   ${status.padEnd(5)} ${(ms / 1000).toFixed(1)}s`);
}

async function report(c: Ctx, sha: string): Promise<number> {
  say();
  say(summaryTable(c.results));
  const bad = failed(c.results);
  say();
  say(bad ? "==> FAILED" : c.args.dryRun ? "==> dry run done: nothing ran as root" : "==> all passed (expected failures marked XFAIL)");
  const dir = c.args.proofDir ?? join(tmpdir(), "doze-machine-test");
  mkdirSync(dir, { recursive: true });
  const file = join(dir, proofFileName(new Date(), sha, c.args.dryRun));
  writeFileSync(file, proofText());
  console.log(`proof log: ${file}`);
  return bad ? 1 : 0;
}

process.exit(await main());

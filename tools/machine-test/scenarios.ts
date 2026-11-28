// The scenarios (numbered as the plan has them; see run.ts for the order and the cleanup).

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  Abort,
  adoptRoot,
  Checks,
  type Ctx,
  caDir,
  daemonPid,
  dz,
  remember,
  expectBody,
  guarded,
  previousFingerprints,
  root,
  rootCrt,
  scenario,
  url,
} from "./ctx";
import { extractScript, scanScript, scutilDomains, serverTs, stackHcl } from "./lib";
import {
  alive,
  curlTrusted,
  fileFingerprint,
  foundationLoad,
  keychainFingerprints,
  LINUX_KEEP,
  MAC_JOB,
  MAC_KEEP,
  rootTrusted,
  routed,
  run,
  say,
  servedLeaf,
  systemResolves,
  within,
} from "./sys";

const DOWN_C_ISSUE = "https://github.com/doze-dev/doze-rs/issues/186";

/** Brings the throwaway stack up from its own folder (the daemon makes the authority on first start). */
async function up(c: Ctx): Promise<void> {
  const r = await dz(c, ["-y", "up", "--all"], { timeoutMs: 180_000 });
  if (r.code !== 0) throw new Abort(`doze up failed (exit ${r.code}): ${(r.err || r.out).trim()}`);
  remember(c);
}

async function down(c: Ctx): Promise<void> {
  await dz(c, ["down"], { timeoutMs: 60_000 });
}

/** https to `target` through the system's trust: curl without -k and, on macOS, Foundation. */
async function trustedLoad(c: Ctx, k: Checks, target: string, body: string, resolve?: string): Promise<void> {
  const curl = await curlTrusted(target, resolve);
  k.ok(curl.ok && curl.said.includes(body), `curl ${target} (no -k)`, curl.said);
  if (c.os === "mac") {
    const f = await foundationLoad(target);
    k.ok(f.ok && f.said.includes(body), `Foundation loads ${target}`, f.said);
  }
}

/** A TLS server of the run's own, its leaf signed with openssl from the test authority: proves the system trusts the
 *  root itself, apart from anything doze serves. */
async function ownTlsServer(c: Ctx): Promise<{ url: string; stop: () => void } | null> {
  const dir = join(c.work, "leaf");
  const ext = join(c.work, "leaf.ext");
  writeFileSync(
    ext,
    "basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth\nsubjectAltName=IP:127.0.0.1\n",
  );
  const steps = [
    ["mkdir", "-p", dir],
    ["openssl", "ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", join(dir, "leaf.key")],
    ["openssl", "req", "-new", "-key", join(dir, "leaf.key"), "-subj", "/CN=doze machine test", "-out", join(dir, "leaf.csr")],
    ["openssl", "x509", "-req", "-in", join(dir, "leaf.csr"), "-CA", rootCrt(c), "-CAkey", join(caDir(c), "root.key"),
      "-set_serial", `0x${c.names.id.split("").map((ch) => ch.charCodeAt(0).toString(16)).join("")}`,
      "-days", "2", "-sha256", "-extfile", ext, "-out", join(dir, "leaf.crt")],
  ];
  for (const s of steps) if ((await run(s)).code !== 0) return null;
  const server = Bun.serve({
    hostname: "127.0.0.1",
    port: 0,
    tls: { cert: readFileSync(join(dir, "leaf.crt"), "utf8"), key: readFileSync(join(dir, "leaf.key"), "utf8") },
    fetch: () => new Response(`${expectBody(c)} (own leaf)\n`),
  });
  return { url: `https://127.0.0.1:${server.port}/`, stop: () => server.stop(true) };
}

export async function s1Setup(c: Ctx): Promise<void> {
  await scenario(c, "1", "setup from scratch: the test CA made and trusted", async () => {
    const k = new Checks();
    writeFileSync(join(c.stack, "server.ts"), serverTs(c.names.id));
    writeFileSync(join(c.stack, "doze.hcl"), stackHcl(c.names, c.bun, join(c.stack, "server.ts")));
    await up(c);
    const fp = adoptRoot(c);
    if (!k.ok(fp !== null, `the daemon made the test CA (${fp?.slice(0, 12)}) in the private home`)) return k.result();
    k.ok(previousFingerprints(c).length === 0, "the new CA lists no rotated-out roots (so trust untrusts nothing)");
    const { touches } = await guarded(c, ["trust"]);
    say(`   · doze trust ${c.args.dryRun ? "would change" : "changed"}: ${touches.join("; ") || "nothing"}`);
    if (c.args.dryRun) return { status: "DRY", detail: `planned: ${touches.join("; ")}` };
    k.ok((await dz(c, ["trust", "--check"])).code === 0, "doze trust --check passes");
    k.ok(await rootTrusted(c.os, rootCrt(c)), c.os === "mac" ? "security verify-cert trusts the root" : "the root is in the system bundle");
    const own = await ownTlsServer(c);
    if (k.ok(own !== null, "a leaf of the run's own, signed by the test CA")) {
      try {
        await trustedLoad(c, k, own!.url, expectBody(c));
      } finally {
        own!.stop();
      }
    }
    return k.result();
  });
}

export async function s2Domain(c: Ctx): Promise<void> {
  await scenario(c, "2", "a declared domain resolves and serves trusted https, no manual HUP", async () => {
    if (c.args.dryRun) return { status: "DRY", detail: "needs the route doze trust writes" };
    const k = new Checks();
    const d = c.names.domain;
    const t = await within(c.args.wait, async () => (await systemResolves(c.os, d)).some((ip) => ip.startsWith("127.")));
    k.ok(t !== null, `${d} resolves through the system resolver${t !== null ? ` in ${t.toFixed(1)}s` : ` (not within ${c.args.wait}s)`}`,
      c.os === "mac" ? "mDNSResponder did not read the new /etc/resolver file: the HUP fix (PR #185) is missing" : "");
    if (c.os === "mac") {
      const port = scutilDomains((await run(["scutil", "--dns"])).out).find((x) => x.domain === d)?.port;
      k.ok(port === Number(c.env.DOZE_ZONE_RESOLVER?.split(":")[1]), `scutil --dns routes ${d} to the test resolver (port ${port})`);
    }
    await trustedLoad(c, k, url(c), expectBody(c));
    return k.result();
  });
}

export async function s3Remove(c: Ctx): Promise<void> {
  await scenario(c, "3", "doze trust --remove: the route goes and the name stops resolving", async () => {
    const { touches } = await guarded(c, ["trust", "--remove"]);
    if (c.args.dryRun) return { status: "DRY", detail: `planned: ${touches.join("; ")}` };
    const k = new Checks();
    const d = c.names.domain;
    const r = await within(c.args.wait, async () => !(await routed(c.os, d)));
    k.ok(r !== null, `the route is gone (${c.os === "mac" ? "scutil --dns" : "resolvectl domain doze0"})${r !== null ? ` in ${r.toFixed(1)}s` : ""}`);
    const n = await within(c.args.wait, async () => (await systemResolves(c.os, d)).length === 0);
    k.ok(n !== null, `${d} no longer resolves${n !== null ? ` (${n.toFixed(1)}s)` : ` (still answers after ${c.args.wait}s)`}`);
    k.ok(await rootTrusted(c.os, rootCrt(c)), "the CA stays trusted (the zone's names still use it)");
    return k.result();
  });
}

export async function s4Again(c: Ctx): Promise<void> {
  await scenario(c, "4", "setup again on a working machine: nothing destructive, same CA", async () => {
    const k = new Checks();
    const fp0 = fileFingerprint(rootCrt(c));
    const prev0 = previousFingerprints(c).join(",");
    // Once the CA is trusted a second run has only the route S3 took away to write back; under --dry-run the CA was
    // never trusted, so trusting it is still in the plan.
    const unexpected = (t: string) => /^(untrust|delete certificate)/.test(t) || (t.startsWith("trust ") && !c.args.dryRun);
    const again = await guarded(c, ["trust"]);
    k.ok(!again.touches.some(unexpected), `doze trust again only routes: ${again.touches.join("; ") || "nothing"}`);
    if (!c.args.dryRun) {
      const third = extractScript((await dz(c, ["trust", "--dry-run"])).out);
      k.ok(third === "", "a third doze trust plans nothing at all", third);
    }
    for (const argv of [["dns-setup"], ["doctor", "--fix"]]) {
      const dry = await dz(c, [...argv, "--dry-run"]);
      const scan = scanScript(extractScript(dry.out), { ...policyOf(c), zone: true });
      const bad = scan.touches.filter((t) => /^(untrust|delete certificate)/.test(t));
      k.ok(scan.violations.length === 0 && bad.length === 0, `doze ${argv.join(" ")} plans no untrust and no new CA`, [...scan.violations, ...bad].join("; "));
    }
    if (c.os === "linux" && c.zone.installed && !c.args.dryRun) {
      await guarded(c, ["dns-setup"], { zone: true });
      k.ok(fileFingerprint(rootCrt(c)) === fp0, "doze dns-setup again keeps the CA");
    }
    await down(c);
    await up(c);
    k.ok(fileFingerprint(rootCrt(c)) === fp0, `the stack restarted with the same CA (${fp0?.slice(0, 12)})`);
    k.ok(previousFingerprints(c).join(",") === prev0, "no root was rotated out");
    if (!c.args.dryRun) k.ok((await dz(c, ["trust", "--check"])).code === 0, "doze trust --check passes");
    const r = k.result();
    return c.args.dryRun && r.status === "PASS" ? { status: "DRY", detail: "plans checked; nothing run as root" } : r;
  });
}

const policyOf = (c: Ctx) => ({ os: c.os, home: c.home, fingerprints: c.fingerprints, domains: [c.names.domain], zone: false });

export async function s5Rotate(c: Ctx): Promise<void> {
  await scenario(c, "5", "doze trust --rotate: new CA trusted, old untrusted, leaves reissued", async () => {
    const k = new Checks();
    const prev = previousFingerprints(c);
    const foreign = prev.filter((f) => !c.fingerprints.includes(f));
    if (!existsSync(join(caDir(c), "root.key")) || foreign.length) {
      // A rotation of an authority it cannot read untrusts every doze root on the machine (Ca::rotate): never here.
      throw new Abort(`refusing to rotate: ${foreign.length ? `previous.json names roots the test did not make (${foreign.join(", ")})` : "the CA is not readable"}`);
    }
    if (c.args.dryRun) {
      say("    [dry-run] would run: doze trust --rotate (it has no --dry-run: a rotation makes the new CA at once)");
      return { status: "DRY", detail: `pre-checks passed; previous.json holds ${prev.length} root(s), all the test's` };
    }
    const fp0 = fileFingerprint(rootCrt(c))!;
    const old = join(c.work, "roots", `${fp0}.pem`);
    const leaf0 = await servedLeaf(`127.0.0.1:${c.tls.port}`, c.names.domain);
    k.ok(leaf0 !== null, `the front door serves a leaf for ${c.names.domain}`);
    const env = c.args.appPrompt ? { DOZE_ADMIN_PROMPT: "1" } : undefined;
    if (c.args.appPrompt) say("   · through the system's administrator prompt, as the app runs it (DOZE_ADMIN_PROMPT=1)");
    const r = await dz(c, ["trust", "--rotate"], { tty: true, env });
    const fp1 = adoptRoot(c);
    if (!k.ok(r.code === 0, "doze trust --rotate", (r.err || r.out).trim())) return k.result();
    k.ok(fp1 !== null && fp1 !== fp0, `a new CA (${fp1?.slice(0, 12)}, was ${fp0.slice(0, 12)})`);
    k.ok(await rootTrusted(c.os, rootCrt(c)), "the new CA is trusted");
    k.ok(!(await rootTrusted(c.os, old)), "the old CA is no longer trusted");
    if (c.os === "mac") k.ok(!(await keychainFingerprints()).includes(fp0), "the old CA left the System keychain");
    const t = await within(c.args.wait, async () => {
      const l = await servedLeaf(`127.0.0.1:${c.tls.port}`, c.names.domain);
      if (!l || l.fp === leaf0?.fp) return false;
      writeFileSync(join(c.work, "leaf-after.pem"), l.pem);
      return (await run(["openssl", "verify", "-CAfile", rootCrt(c), join(c.work, "leaf-after.pem")])).code === 0;
    });
    k.ok(t !== null, `the front door reissued the leaf from the new CA${t !== null ? ` in ${t.toFixed(1)}s` : ""}`);
    await trustedLoad(c, k, url(c), expectBody(c));
    return k.result();
  });
}

export async function s6DownC(c: Ctx): Promise<void> {
  await scenario(c, "6", "doze down -c <dir> stops the stack started from <dir> (expected to fail)", async () => {
    const pid = daemonPid(c);
    if (!pid || !alive(pid)) return { status: "FAIL", detail: "the stack is not running, so there is nothing to stop" };
    const r = await run([c.doze, "-c", c.stack, "down"], { cwd: c.work, env: c.env, timeoutMs: 60_000 });
    const gone = await within(25, async () => !alive(pid));
    say(`   · doze -c ${c.stack} down (from ${c.work}): ${r.out.trim() || r.err.trim()}`);
    if (gone !== null) return { status: "XPASS", detail: `stopped daemon ${pid}: ${DOWN_C_ISSUE} looks fixed, take the mark off` };
    await down(c);
    return { status: "XFAIL", detail: `daemon ${pid} kept running; stopped from its folder instead (${DOWN_C_ISSUE})` };
  });
}

// ── --with-zone ─────────────────────────────────────────────────────────────────────────────────────────────────────

/** Keeps the owner's zone files (a tar, as root) before the zone's setup writes over them. */
async function keepZone(c: Ctx): Promise<void> {
  const keep = c.os === "mac" ? MAC_KEEP : LINUX_KEEP;
  c.zone.existed = keep.filter((f) => existsSync(f));
  if (c.zone.existed.length) {
    const r = await root(c, ["tar", "-cpf", c.zone.tar, "-C", "/", ...c.zone.existed.map((f) => f.slice(1))]);
    if (r && r.code !== 0) throw new Abort(`could not keep the zone's files: ${r.err.trim()}`);
  }
  say(`   · kept: ${c.zone.existed.join(", ") || "nothing (no zone was set up)"}`);
}

/** Linux: the zone's setup for the test home, first (the routes for domains live on its link). */
export async function zoneLinux(c: Ctx): Promise<void> {
  await scenario(c, "Z", "zone setup for the test home (doze dns-setup; Linux)", async () => {
    const k = new Checks();
    await keepZone(c);
    c.zone.installed = true;
    await guarded(c, ["dns-setup"], { zone: true });
    if (c.args.dryRun) return { status: "DRY", detail: "planned the zone install" };
    k.ok((await dz(c, ["dns-setup", "--check"])).code === 0, "doze dns-setup --check passes");
    return k.result();
  });
}

/** macOS: the zone's setup for the test home, with the stack down, then the owner's put back at the cleanup. */
export async function zoneMac(c: Ctx): Promise<void> {
  await scenario(c, "Z", "zone setup for the test home (doze dns-setup; macOS, --with-zone)", async () => {
    const k = new Checks();
    const fp0 = fileFingerprint(rootCrt(c));
    await keepZone(c);
    c.zone.installed = true;
    await guarded(c, ["dns-setup"], { zone: true });
    if (c.args.dryRun) return { status: "DRY", detail: "planned the zone install" };
    k.ok((await dz(c, ["dns-setup", "--check"])).code === 0, "doze dns-setup --check passes");
    const port = Number(c.env.DOZE_ZONE_RESOLVER?.split(":")[1]);
    const t = await within(c.args.wait, async () =>
      scutilDomains((await run(["scutil", "--dns"])).out).some((d) => d.domain === "doze" && d.port === port),
    );
    k.ok(t !== null, `scutil --dns routes .doze to the test resolver (port ${port})`);
    const again = extractScript((await dz(c, ["dns-setup", "--dry-run"])).out);
    k.ok(again === "", "doze dns-setup again plans nothing", again);
    k.ok(fileFingerprint(rootCrt(c)) === fp0, "the zone's setup kept the test CA");
    return k.result();
  });
}

/** Puts the owner's zone back: the test's job stopped, the kept files restored, their job started again. */
export async function restoreZone(c: Ctx): Promise<void> {
  if (!c.zone.installed) return;
  say("   · putting the zone's own setup back");
  if (c.os === "mac") {
    await root(c, ["launchctl", "bootout", "system", MAC_JOB]);
    for (const f of MAC_KEEP.filter((f) => !c.zone.existed.includes(f))) await root(c, ["rm", "-f", f]);
    if (c.zone.existed.length) await root(c, ["tar", "-xpf", c.zone.tar, "-C", "/"]);
    if (c.zone.existed.includes(MAC_JOB)) await root(c, ["launchctl", "bootstrap", "system", MAC_JOB]);
  } else {
    try {
      await guarded(c, ["dns-setup", "--uninstall"], { zone: true });
    } catch (e) {
      say(`   ✗ doze dns-setup --uninstall: ${(e as Error).message}; putting the kept files back anyway`);
    }
    for (const f of LINUX_KEEP.filter((f) => !c.zone.existed.includes(f))) await root(c, ["rm", "-rf", f]);
    if (c.zone.existed.length) await root(c, ["tar", "-xpf", c.zone.tar, "-C", "/"]);
    await root(c, ["systemctl", "daemon-reload"]);
    for (const u of ["doze-names.service", "doze-netd.service"]) {
      if (c.zone.existed.includes(`/etc/systemd/system/${u}`)) {
        await root(c, ["systemctl", "enable", "--now", u]);
        await root(c, ["systemctl", "restart", u]);
      }
    }
    if (c.zone.existed.includes("/etc/sysctl.d/60-doze.conf")) await root(c, ["sysctl", "-q", "-p", "/etc/sysctl.d/60-doze.conf"]);
    await root(c, ["update-ca-certificates", "--fresh"]);
  }
  c.zone.installed = false;
}

// The machine test's contact with the machine: running programs (every one logged to the proof log), the system's
// own view of names, routes and trust, and the before/after snapshot. Reading only: what changes the machine is in
// run.ts, behind the safety scan and --dry-run.

import { existsSync, readdirSync, readFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { join } from "node:path";
import {
  dscacheAddresses,
  getentAddresses,
  isTestDomain,
  linkDomains,
  type Os,
  opensslFingerprint,
  pemFingerprint,
  redact,
  scutilDomains,
  sha256Lines,
  type Snapshot,
} from "./lib";

export const SYSTEM_KEYCHAIN = "/Library/Keychains/System.keychain";
export const MAC_JOB = "/Library/LaunchDaemons/dev.doze.netd.plist";
/** What the zone's setup writes over on macOS (kept and put back around --with-zone). */
export const MAC_KEEP = [MAC_JOB, "/Library/PrivilegedHelperTools/dev.doze.netd", "/etc/resolver/doze"];
/** The same on Linux: the units, the daemon's copy, sysctl, the CA in the bundle, hosts (scripts/core-privileged-e2e-linux.sh). */
export const LINUX_KEEP = [
  "/etc/systemd/system/doze-names.service",
  "/etc/systemd/system/doze-names.service.d",
  "/etc/systemd/system/doze-netd.service",
  "/usr/local/libexec/doze-netd",
  "/etc/sysctl.d/60-doze.conf",
  "/usr/local/share/ca-certificates/doze-local-ca.crt",
  "/etc/hosts",
];
export const LINUX_BUNDLE = "/usr/local/share/ca-certificates/doze-local-ca.crt";
export const LINUX_SYSTEM_BUNDLE = "/etc/ssl/certs/ca-certificates.crt";

// ── The log ─────────────────────────────────────────────────────────────────────────────────────────────────────────

const proof: string[] = [];

/** A line in the proof log only. */
export function note(line: string): void {
  proof.push(redact(`${new Date().toISOString().slice(11, 23)} ${line}`));
}

/** A line on the screen and in the proof log. */
export function say(line = ""): void {
  console.log(line);
  proof.push(redact(line));
}

export function proofText(): string {
  return `${proof.join("\n")}\n`;
}

// ── Programs ────────────────────────────────────────────────────────────────────────────────────────────────────────

export type Ran = { code: number; out: string; err: string; ms: number };

export type RunOpts = {
  cwd?: string;
  env?: Record<string, string | undefined>;
  /** The terminal's stdin (for sudo's password and the program's own questions); output is still captured. */
  tty?: boolean;
  /** Echo the output to the screen as well. */
  show?: boolean;
  timeoutMs?: number;
};

/** Runs `cmd`, logs the command, its exit and its output (redacted) to the proof log. */
export async function run(cmd: string[], o: RunOpts = {}): Promise<Ran> {
  const start = performance.now();
  note(`$ ${o.cwd ? `(cd ${o.cwd}) ` : ""}${cmd.map(shq).join(" ")}`);
  const p = Bun.spawn(cmd, {
    cwd: o.cwd,
    env: o.env ? clean(o.env) : undefined,
    stdin: o.tty ? "inherit" : "ignore",
    stdout: "pipe",
    stderr: "pipe",
  });
  let timer: ReturnType<typeof setTimeout> | undefined;
  if (o.timeoutMs) timer = setTimeout(() => p.kill(), o.timeoutMs);
  const [out, err] = await Promise.all([new Response(p.stdout).text(), new Response(p.stderr).text()]);
  const code = await p.exited;
  if (timer) clearTimeout(timer);
  const ms = performance.now() - start;
  const text = (out + (err ? `\n[stderr]\n${err}` : "")).trimEnd();
  note(`  → exit ${code} in ${(ms / 1000).toFixed(2)}s${text ? `\n${indent(text)}` : ""}`);
  if (o.show && text) console.log(indent(redact(text)));
  return { code, out, err, ms };
}

function clean(env: Record<string, string | undefined>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, v] of Object.entries(env)) if (v !== undefined) out[k] = v;
  return out;
}

const indent = (t: string) =>
  t
    .split("\n")
    .map((l) => `    ${l}`)
    .join("\n");

export const shq = (s: string) => (/^[\w@%+=:,./-]+$/.test(s) ? s : `'${s.replace(/'/g, "'\\''")}'`);

export const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Polls `probe` every half second until it says true or `seconds` pass; the seconds it took, or null. */
export async function within(seconds: number, probe: () => Promise<boolean>): Promise<number | null> {
  const start = performance.now();
  while (performance.now() - start < seconds * 1000) {
    if (await probe()) return (performance.now() - start) / 1000;
    await sleep(500);
  }
  return (await probe()) ? (performance.now() - start) / 1000 : null;
}

export function alive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    return (e as NodeJS.ErrnoException).code === "EPERM";
  }
}

// ── Certificates ────────────────────────────────────────────────────────────────────────────────────────────────────

export function fileFingerprint(path: string): string | null {
  try {
    return pemFingerprint(readFileSync(path, "utf8"));
  } catch {
    return null;
  }
}

/** Every certificate fingerprint in the System keychain (macOS). */
export async function keychainFingerprints(): Promise<string[]> {
  return sha256Lines((await run(["security", "find-certificate", "-a", "-Z", SYSTEM_KEYCHAIN])).out);
}

/** Whether the system trusts a root certificate file: the keychain's own verify (macOS), the bundle (Linux). */
export async function rootTrusted(os: Os, file: string): Promise<boolean> {
  if (os === "mac") {
    return (await run(["security", "verify-cert", "-L", "-l", "-q", "-p", "basic", "-c", file])).code === 0;
  }
  try {
    const body = readFileSync(file, "utf8").replace(/-----[A-Z ]+-----|\s+/g, "");
    const bundle = readFileSync(LINUX_SYSTEM_BUNDLE, "utf8").replace(/-----[A-Z ]+-----|\s+/g, "");
    return body.length > 0 && bundle.includes(body);
  } catch {
    return false;
  }
}

/** The certificate a TLS server presents for `sni` at `addr`: its fingerprint and PEM. */
export async function servedLeaf(addr: string, sni: string): Promise<{ fp: string; pem: string } | null> {
  const s = await run(["sh", "-c", `openssl s_client -connect ${shq(addr)} -servername ${shq(sni)} </dev/null 2>/dev/null`], {
    timeoutMs: 10_000,
  });
  const pem = s.out.match(/-----BEGIN CERTIFICATE-----[\s\S]*?-----END CERTIFICATE-----/)?.[0];
  if (!pem) return null;
  const f = Bun.spawnSync(["openssl", "x509", "-noout", "-fingerprint", "-sha256"], { stdin: Buffer.from(pem) });
  const fp = opensslFingerprint(f.stdout.toString());
  return fp ? { fp, pem } : null;
}

// ── Names and routes, as the system sees them ───────────────────────────────────────────────────────────────────────

/** What the system resolver answers for `host`: dscacheutil (macOS, through mDNSResponder and /etc/resolver), getent
 *  (Linux, through nss and systemd-resolved). */
export async function systemResolves(os: Os, host: string): Promise<string[]> {
  if (os === "mac") return dscacheAddresses((await run(["dscacheutil", "-q", "host", "-a", "name", host])).out);
  return getentAddresses((await run(["getent", "ahostsv4", host])).out);
}

/** Whether the system has a route for `domain`: `scutil --dns` (macOS), the zone link's domains (Linux). */
export async function routed(os: Os, domain: string): Promise<boolean> {
  if (os === "mac") return scutilDomains((await run(["scutil", "--dns"])).out).some((d) => d.domain === domain);
  return linkDomains((await run(["resolvectl", "domain", "doze0"])).out).includes(`~${domain}`);
}

/** A URL loaded through Apple's own stack (Foundation's NSData, from JavaScript for Automation: no compiler needed). */
export async function foundationLoad(url: string): Promise<{ ok: boolean; said: string }> {
  const js = `ObjC.import('Foundation');
function run(argv) {
  const err = Ref();
  const d = $.NSData.dataWithContentsOfURLOptionsError($.NSURL.URLWithString(argv[0]), 0, err);
  if (d.isNil()) return 'ERR ' + (err[0] && !err[0].isNil() ? ObjC.unwrap(err[0].localizedDescription) : 'no data');
  return 'OK ' + ObjC.unwrap($.NSString.alloc.initWithDataEncoding(d, $.NSUTF8StringEncoding)).trim();
}`;
  const r = await run(["osascript", "-l", "JavaScript", "-e", js, url], { timeoutMs: 30_000 });
  const said = (r.out || r.err).trim();
  return { ok: r.code === 0 && said.startsWith("OK "), said };
}

/** curl with the system's trust (no -k): the body, or why not. */
export async function curlTrusted(url: string, resolve?: string): Promise<{ ok: boolean; said: string }> {
  const args = ["curl", "-sS", "--max-time", "10", ...(resolve ? ["--resolve", resolve] : []), url];
  const r = await run(args);
  return { ok: r.code === 0, said: (r.code === 0 ? r.out : r.err).trim() };
}

// ── The snapshot ────────────────────────────────────────────────────────────────────────────────────────────────────

const hashOf = (path: string): string => {
  try {
    return createHash("sha256").update(readFileSync(path)).digest("hex").slice(0, 16);
  } catch {
    return existsSync(path) ? "unreadable" : "absent";
  }
};

/** What the run may change, read without privilege: compared before and after (scenario 7). */
export async function snapshot(os: Os): Promise<Snapshot> {
  if (os === "mac") {
    const resolver = existsSync("/etc/resolver") ? readdirSync("/etc/resolver").sort() : [];
    const trust = await run(["security", "dump-trust-settings", "-d"]);
    const scutil = scutilDomains((await run(["scutil", "--dns"])).out);
    return {
      "/etc/resolver": resolver.map((f) => `${f}=${hashOf(join("/etc/resolver", f))}`),
      "System keychain": await keychainFingerprints(),
      "admin trust settings": (trust.out + trust.err).split("\n").filter((l) => /^Cert \d+:|No Trust Settings/.test(l)),
      "scutil --dns (doze and test domains)": scutil
        .filter((d) => d.domain === "doze" || isTestDomain(d.domain))
        .map((d) => `${d.domain}:${d.port ?? "-"}`),
      "zone files": MAC_KEEP.map((f) => `${f}=${hashOf(f)}`),
    };
  }
  const units = await run(["systemctl", "is-active", "doze-names.service", "doze-netd.service"]);
  const bundle = existsSync(LINUX_SYSTEM_BUNDLE) ? readFileSync(LINUX_SYSTEM_BUNDLE, "utf8") : "";
  return {
    "zone files": LINUX_KEEP.map((f) => `${f}=${hashOf(f)}`),
    "doze0 domains": linkDomains((await run(["resolvectl", "domain", "doze0"])).out),
    "units active": units.out.split("\n").filter(Boolean),
    "system bundle roots": [`${(bundle.match(/BEGIN CERTIFICATE/g) ?? []).length} roots`],
  };
}

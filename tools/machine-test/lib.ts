// The pure parts of the machine test (tools/machine-test/run.ts): parsing what the system's tools print, the names the
// run uses, the stack it writes, the safety scan of every privileged script doze would run, and the report. Nothing
// here runs a program or touches a file, so all of it is unit-tested (lib.test.ts) on any machine.

import { createHash } from "node:crypto";

export type Os = "mac" | "linux";

// ── Arguments ───────────────────────────────────────────────────────────────────────────────────────────────────────

export type Args = {
  dryRun: boolean;
  withZone: boolean;
  appPrompt: boolean;
  proofDir: string | null;
  doze: string | null;
  wait: number;
  keep: boolean;
  help: boolean;
};

export function parseArgs(argv: string[]): Args {
  const a: Args = {
    dryRun: false,
    withZone: false,
    appPrompt: false,
    proofDir: null,
    doze: null,
    wait: 30,
    keep: false,
    help: false,
  };
  const value = (i: number, flag: string): string => {
    const v = argv[i + 1];
    if (v === undefined || v.startsWith("--")) throw new Error(`${flag} needs a value`);
    return v;
  };
  for (let i = 0; i < argv.length; i++) {
    const f = argv[i];
    const [flag, inline] = f.includes("=") ? [f.slice(0, f.indexOf("=")), f.slice(f.indexOf("=") + 1)] : [f, undefined];
    const take = (): string => {
      if (inline !== undefined) return inline;
      const v = value(i, flag);
      i++;
      return v;
    };
    switch (flag) {
      case "--dry-run":
        a.dryRun = true;
        break;
      case "--with-zone":
        a.withZone = true;
        break;
      case "--app-prompt":
        a.appPrompt = true;
        break;
      case "--keep":
        a.keep = true;
        break;
      case "--proof-dir":
        a.proofDir = take();
        break;
      case "--doze":
        a.doze = take();
        break;
      case "--wait": {
        const n = Number(take());
        if (!Number.isInteger(n) || n < 1 || n > 600) throw new Error("--wait takes whole seconds, 1 to 600");
        a.wait = n;
        break;
      }
      case "-h":
      case "--help":
        a.help = true;
        break;
      default:
        throw new Error(`unknown argument ${f} (--help lists them)`);
    }
  }
  return a;
}

// ── Names ───────────────────────────────────────────────────────────────────────────────────────────────────────────

/** The parent of every domain a run declares: a name no real stack uses, so a leftover is recognisably the test's. */
export const TEST_PARENT = "doze-machine-test.dev";

/** A DNS label of `n` lowercase letters and digits, starting with a letter. */
export function randomLabel(bytes: Uint8Array, n = 8): string {
  const alphabet = "abcdefghijklmnopqrstuvwxyz0123456789";
  let s = "";
  for (let i = 0; i < n; i++) {
    const b = bytes[i % bytes.length] ?? 0;
    s += i === 0 ? alphabet[b % 26] : alphabet[(b + i * 7) % alphabet.length];
  }
  return s;
}

export type Names = { id: string; stack: string; domain: string };

/** The run's names: a stack and a domain no other stack can hold, both carrying the run's id. */
export function testNames(id: string): Names {
  if (!/^[a-z][a-z0-9]{3,15}$/.test(id)) throw new Error(`a run id is 4 to 16 lowercase letters and digits: ${id}`);
  return { id, stack: `mt${id}`, domain: `${id}.${TEST_PARENT}` };
}

/** Whether `domain` is one a machine test declares (a leftover check must find only these). */
export function isTestDomain(domain: string): boolean {
  return domain.endsWith(`.${TEST_PARENT}`) && /^[a-z0-9-]+$/.test(domain.slice(0, -TEST_PARENT.length - 1));
}

/** The throwaway stack: one app with the run's domain, names on, served by bun on the $HOST and $PORT doze gives it. */
export function stackHcl(n: Names, bun: string, server: string): string {
  return `# The machine test's throwaway stack (tools/machine-test). Safe to delete.
stack = "${n.stack}"

defaults {
  names = true
}

app "web" {
  run     = "${bun} run ${server}"
  port    = 3000
  domains = ["${n.domain}"]
}
`;
}

/** The app: answers every request with one line that says it is this run's. */
export function serverTs(id: string): string {
  return `Bun.serve({
  hostname: process.env.HOST ?? "127.0.0.1",
  port: Number(process.env.PORT),
  fetch: () => new Response("machine-test ${id} ok\\n"),
});
`;
}

// ── What the system's tools print ───────────────────────────────────────────────────────────────────────────────────

/** SHA-256 of a PEM certificate's DER, lowercase hex: the fingerprint the keychain (`-Z`) and doze use. */
export function pemFingerprint(pem: string): string {
  const m = pem.match(/-----BEGIN CERTIFICATE-----([\s\S]*?)-----END CERTIFICATE-----/);
  if (!m) throw new Error("no certificate in the PEM");
  const der = Buffer.from(m[1].replace(/\s+/g, ""), "base64");
  return createHash("sha256").update(der).digest("hex");
}

/** The SHA-256 fingerprints in `security find-certificate -a -Z` output, lowercase, each once, in order. */
export function sha256Lines(out: string): string[] {
  const fps: string[] = [];
  for (const line of out.split("\n")) {
    const m = line.match(/SHA-256 hash:\s*([0-9A-Fa-f]{64})\s*$/);
    if (m && !fps.includes(m[1].toLowerCase())) fps.push(m[1].toLowerCase());
  }
  return fps;
}

/** The resolvers `scutil --dns` lists: each domain with its port, once (the first section only: per-interface
 *  scoped queries repeat the same resolvers). */
export function scutilDomains(out: string): { domain: string; port: number | null }[] {
  const firstSection = out.split(/\nDNS configuration \(for scoped queries\)/)[0];
  const found: { domain: string; port: number | null }[] = [];
  for (const block of firstSection.split(/\nresolver #\d+/).slice(1)) {
    const d = block.match(/^\s*domain\s*:\s*(\S+)/m);
    if (!d) continue;
    const p = block.match(/^\s*port\s*:\s*(\d+)/m);
    const entry = { domain: d[1].replace(/\.$/, "").toLowerCase(), port: p ? Number(p[1]) : null };
    if (!found.some((f) => f.domain === entry.domain && f.port === entry.port)) found.push(entry);
  }
  return found;
}

/** The addresses in `dscacheutil -q host -a name <host>` output. */
export function dscacheAddresses(out: string): string[] {
  return [...out.matchAll(/^ipv?6?_?address:\s*(\S+)/gm)].map((m) => m[1]);
}

/** The addresses in `getent ahostsv4 <host>` (or `getent hosts`) output, each once. */
export function getentAddresses(out: string): string[] {
  const ips: string[] = [];
  for (const line of out.split("\n")) {
    const ip = line.trim().split(/\s+/)[0];
    if (ip && /^[0-9a-f.:]+$/i.test(ip) && !ips.includes(ip)) ips.push(ip);
  }
  return ips;
}

/** The domain list in `resolvectl domain <link>` output (`Link 3 (doze0): ~doze ~dev.acme.com`). */
export function linkDomains(out: string): string[] {
  const i = out.indexOf(":");
  return i < 0 ? [] : out.slice(i + 1).split(/\s+/).filter(Boolean);
}

/** The privileged script in a `doze … --dry-run` print: the lines of every `<<'DOZE_…'` block, the root part
 *  (`sudo sh <<'DOZE_SETUP'`) and, where trust runs as the person (macOS's dialog, PR #187), the
 *  `sh <<'DOZE_AS_YOU'` part after it; empty when doze has nothing to run. */
export function extractScript(out: string): string {
  const body: string[] = [];
  let end: string | null = null;
  for (const l of out.split("\n")) {
    if (end !== null) {
      if (l.trim() === end) end = null;
      else body.push(l);
      continue;
    }
    const m = l.match(/<<'?(DOZE_[A-Z_]+)'?\s*$/);
    if (m) end = m[1];
  }
  return body.join("\n");
}

/** The SHA-256 of a served certificate, from `openssl x509 -noout -fingerprint -sha256` output. */
export function opensslFingerprint(out: string): string | null {
  const m = out.match(/Fingerprint=([0-9A-Fa-f:]+)/);
  return m ? m[1].replace(/:/g, "").toLowerCase() : null;
}

// ── The safety scan ─────────────────────────────────────────────────────────────────────────────────────────────────

export type Policy = {
  os: Os;
  /** The test's doze home: every certificate file a script trusts or untrusts must be under it. */
  home: string;
  /** Fingerprints the run made (current and rotated-out): the only ones a script may delete. */
  fingerprints: string[];
  /** The run's domains: the only routes a script may write or remove. */
  domains: string[];
  /** --with-zone: the zone's own setup (the launchd job or units, /etc/resolver/doze) may be installed. */
  zone: boolean;
};

export type Scan = { touches: string[]; violations: string[] };

const unquote = (s: string): string => s.replace(/^'(.*)'$/, "$1").replace(/'\\''/g, "'");

/** A script's commands: a single-quoted string that spans lines (a unit file written with printf) is one command, and
 *  a here-document's body is passed through line by line, its quotes not counted. */
export function logicalLines(script: string): string[] {
  const out: string[] = [];
  let open: string | null = null;
  let heredoc: string | null = null;
  for (const line of script.split("\n")) {
    if (heredoc !== null) {
      out.push(line);
      if (line.trim() === heredoc) heredoc = null;
      continue;
    }
    const joined: string = open === null ? line : `${open}\n${line}`;
    const here = joined.match(/<<'?([A-Z_]+)'?\s*$/);
    if (here) {
      open = null;
      heredoc = here[1];
      out.push(joined);
      continue;
    }
    const quotes = (joined.match(/'/g) ?? []).length;
    if (quotes % 2 === 1) {
      open = joined;
      continue;
    }
    open = null;
    out.push(joined);
  }
  if (open !== null) out.push(open);
  return out;
}

/** What a privileged script doze would run touches, and every part of it the run's policy does not allow. A script
 *  is run only when this finds no violation: it is the guard that keeps the test off the owner's own CA, routes and
 *  network daemon even if doze's planning is wrong (one cleanup bug deleted the in-use CA). */
export function scanScript(script: string, p: Policy): Scan {
  const touches: string[] = [];
  const violations: string[] = [];
  const underHome = (path: string) => path.startsWith(p.home.endsWith("/") ? p.home : `${p.home}/`);
  const fps = new Set(p.fingerprints.map((f) => f.toLowerCase()));
  let inHeredoc: string | null = null;
  for (const raw of logicalLines(script)) {
    const line = raw.trim();
    if (inHeredoc !== null) {
      if (line === inHeredoc) inHeredoc = null;
      continue;
    }
    if (line === "" || line.startsWith("#") || /^[()]$/.test(line) || line === "set -e") continue;
    const here = line.match(/<<'?([A-Z_]+)'?\s*$/);
    if (here) inHeredoc = here[1];
    const bad = (why: string) => violations.push(`${why}: ${line}`);

    // Certificates.
    let m = line.match(/^security add-trusted-cert .* (\S+)$/);
    if (m) {
      const file = unquote(m[1]);
      touches.push(`trust ${file}`);
      if (!underHome(file)) bad("trusts a certificate outside the test home");
      continue;
    }
    m = line.match(/^security add-certificates .* (\S+)$/);
    if (m) {
      const file = unquote(m[1]);
      touches.push(`add certificate ${file}`);
      if (!underHome(file)) bad("adds a certificate outside the test home to a keychain");
      continue;
    }
    m = line.match(/^security remove-trusted-cert -d (\S+)/);
    if (m) {
      const file = unquote(m[1]);
      touches.push(`untrust ${file}`);
      if (!underHome(file)) bad("untrusts a certificate outside the test home");
      continue;
    }
    m = line.match(/^security delete-certificate .*-Z (\S+)/);
    if (m) {
      const fp = unquote(m[1]).toLowerCase();
      touches.push(`delete certificate ${fp.slice(0, 12)}`);
      if (!fps.has(fp)) bad("deletes a certificate the test did not make");
      continue;
    }
    if (/^security /.test(line)) {
      bad("an unrecognised keychain change");
      continue;
    }

    // macOS routes.
    m = line.match(/\/etc\/resolver\/([^'"\s]+)/);
    if (m) {
      const d = m[1];
      touches.push(`${line.startsWith("rm ") ? "remove" : "write"} /etc/resolver/${d}`);
      if (d === "doze") {
        if (!p.zone) bad("changes the zone's own route (/etc/resolver/doze) without --with-zone");
      } else if (!p.domains.includes(d)) bad("changes a route the test did not declare");
      continue;
    }
    if (line === "mkdir -p /etc/resolver" || line === "mkdir -p '/etc/resolver'") continue;

    // macOS rereading /etc/resolver after a route changes (PR #185): doze's own reload, harmless to anything else.
    if (/^(killall -HUP mDNSResponder|dscacheutil -flushcache)( 2>\/dev\/null)?( \|\| true)?$/.test(line)) {
      touches.push("reload mDNSResponder");
      continue;
    }

    // Linux trust and routes.
    m = line.match(/^cp -f (\S+) (\S+)$/);
    if (m && /ca-certificates|ca-trust/.test(m[2])) {
      const file = unquote(m[1]);
      touches.push(`trust ${file} (system bundle)`);
      if (!underHome(file)) bad("puts a certificate outside the test home in the system bundle");
      continue;
    }
    if (/^update-ca-(certificates|trust)/.test(line)) continue;
    m = line.match(/^resolvectl domain (\S+) (.*?)( 2>\/dev\/null \|\| true)?$/);
    if (m) {
      const extra = m[2]
        .split(/\s+/)
        .map((d) => d.replace(/^~/, ""))
        .filter((d) => d !== "doze" && !p.domains.includes(d));
      touches.push(`routes on ${m[1]}: ${m[2]}`);
      if (extra.length) bad(`restates routes the test did not declare (${extra.join(", ")})`);
      continue;
    }

    // The zone's own setup: allowed only when asked for.
    if (/launchctl|LaunchDaemons|PrivilegedHelperTools|systemctl|\/etc\/systemd|sysctl|ip link|ip addr|\/etc\/hosts|dnsmasq|chown|chmod|^cp -f|^cat >|^printf|^rm -f|^mkdir -p|^install /.test(line)) {
      const one = line.replace(/\s+/g, " ");
      touches.push(`zone setup: ${one.length > 90 ? `${one.slice(0, 87)}...` : one}`);
      if (!p.zone) bad("changes the zone's own setup without --with-zone");
      continue;
    }
    bad("an unrecognised privileged line");
  }
  return { touches, violations };
}

// ── Before and after ────────────────────────────────────────────────────────────────────────────────────────────────

export type Snapshot = Record<string, string[]>;

/** Scenario 7's judgement of a before/after difference: what the test left or broke (a problem), and what changed
 *  that the test cannot have changed (a note: another program, or the owner, at work on the machine meanwhile; the
 *  safety scan refuses every script that would touch those). */
export function judgeDiff(
  os: Os,
  before: Snapshot,
  after: Snapshot,
  o: { testFps: string[]; protectedFps: string[]; zone: boolean },
): { problems: string[]; notes: string[] } {
  const problems: string[] = [];
  const notes: string[] = [];
  for (const key of [...new Set([...Object.keys(before), ...Object.keys(after)])].sort()) {
    const b = before[key] ?? [];
    const a = after[key] ?? [];
    for (const [sign, items] of [["+", a.filter((x) => !b.includes(x))], ["-", b.filter((x) => !a.includes(x))]] as const) {
      for (const x of items) {
        const line = `${key}: ${sign}${x}`;
        let ours: boolean;
        if (os === "linux") ours = true;
        else if (key === "System keychain") ours = sign === "+" ? o.testFps.includes(x) : o.protectedFps.includes(x);
        else if (key === "/etc/resolver" || key.startsWith("scutil")) {
          const name = x.split(/[=:]/)[0];
          ours = isTestDomain(name) || (name === "doze" && o.zone);
        } else if (key === "zone files") ours = o.zone;
        else ours = false;
        (ours ? problems : notes).push(line);
      }
    }
  }
  return { problems, notes };
}

// ── The report ──────────────────────────────────────────────────────────────────────────────────────────────────────

export type Status = "PASS" | "FAIL" | "XFAIL" | "XPASS" | "SKIP" | "DRY";

export type Result = { id: string; title: string; status: Status; ms: number; detail: string };

/** Whether the run as a whole failed: a FAIL, or an expected failure that passed (the mark must come off). */
export function failed(results: Result[]): boolean {
  return results.some((r) => r.status === "FAIL" || r.status === "XPASS");
}

export function summaryTable(results: Result[]): string {
  const w = Math.max(8, ...results.map((r) => r.title.length));
  const rows = results.map(
    (r) => `${r.id.padEnd(3)} ${r.title.padEnd(w)}  ${r.status.padEnd(5)}  ${`${(r.ms / 1000).toFixed(1)}s`.padStart(7)}  ${r.detail}`,
  );
  const head = `${"#".padEnd(3)} ${"scenario".padEnd(w)}  ${"result".padEnd(5)}  ${"time".padStart(7)}  detail`;
  return [head, "-".repeat(head.length + 20), ...rows].join("\n");
}

/** The proof log's file name: `machine-test-<date>-<sha>.log` (`-dry` before the extension for a dry run). */
export function proofFileName(date: Date, sha: string, dry: boolean): string {
  const d = date.toISOString().slice(0, 10);
  return `machine-test-${d}-${sha.slice(0, 12)}${dry ? "-dry" : ""}.log`;
}

/** Text as it may go in the log: never a private key, whatever printed it. */
export function redact(text: string): string {
  return text.replace(/-----BEGIN ([A-Z ]*)PRIVATE KEY-----[\s\S]*?-----END \1PRIVATE KEY-----/g, "[private key withheld]");
}

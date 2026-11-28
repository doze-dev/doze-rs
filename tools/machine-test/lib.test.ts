// bun test tools/machine-test: the machine test's pure parts. The samples are what the tools printed on macOS 27 and
// Ubuntu 24.04 (the doze scripts are `doze trust --dry-run` and `doze dns-setup --dry-run` output for a test home).

import { describe, expect, test } from "bun:test";
import {
  dscacheAddresses,
  extractScript,
  failed,
  getentAddresses,
  isTestDomain,
  judgeDiff,
  linkDomains,
  logicalLines,
  opensslFingerprint,
  parseArgs,
  pemFingerprint,
  type Policy,
  proofFileName,
  randomLabel,
  redact,
  scanScript,
  scutilDomains,
  sha256Lines,
  stackHcl,
  summaryTable,
  testNames,
} from "./lib";

const HOME = "/tmp/dzmt.ab12/home";
const FP = "a".repeat(64);
const mac: Policy = { os: "mac", home: HOME, fingerprints: [FP], domains: ["k3x9.doze-machine-test.dev"], zone: false };

const TRUST_DRY = `# route k3x9.doze-machine-test.dev to doze; trust the doze certificate authority
sudo sh <<'DOZE_SETUP'
mkdir -p /etc/resolver
printf 'nameserver 127.0.0.1\\nport 25353\\n' > '/etc/resolver/k3x9.doze-machine-test.dev'
security add-trusted-cert -d -r trustRoot -p ssl -p basic -k /Library/Keychains/System.keychain '${HOME}/ca/root.crt'
DOZE_SETUP
`;

const DNS_SETUP_DRY = `# install the doze network daemon and route .doze to the resolver
sudo sh <<'DOZE_SETUP'
set -e
(
set -e
launchctl bootout system '/Library/LaunchDaemons/dev.doze.netd.plist' 2>/dev/null || true
mkdir -p '/Library/PrivilegedHelperTools'
cp -f '/repo/target/debug/doze' '/Library/PrivilegedHelperTools/dev.doze.netd'
cat > '/Library/LaunchDaemons/dev.doze.netd.plist' <<'PLIST'
<plist version="1.0">
  <string>it's anything in here</string>
</plist>
PLIST
launchctl bootstrap system '/Library/LaunchDaemons/dev.doze.netd.plist'
mkdir -p '/etc/resolver'
printf 'nameserver 127.0.0.1\\nport 25353\\n' > '/etc/resolver/doze'
)
DOZE_SETUP
`;

describe("arguments", () => {
  test("defaults, flags and values", () => {
    expect(parseArgs([])).toMatchObject({ dryRun: false, withZone: false, wait: 30, proofDir: null });
    const a = parseArgs(["--dry-run", "--proof-dir", "/p", "--wait=45", "--with-zone", "--doze", "/d"]);
    expect(a).toMatchObject({ dryRun: true, withZone: true, proofDir: "/p", wait: 45, doze: "/d" });
  });
  test("refuses what it does not know, and a missing value", () => {
    expect(() => parseArgs(["--yes"])).toThrow("unknown argument");
    expect(() => parseArgs(["--proof-dir"])).toThrow("needs a value");
    expect(() => parseArgs(["--wait", "0"])).toThrow("whole seconds");
  });
});

describe("names", () => {
  test("a label is lowercase, starts with a letter, and has the length asked", () => {
    const l = randomLabel(new Uint8Array([255, 0, 7, 99, 200, 13, 1, 2]));
    expect(l).toMatch(/^[a-z][a-z0-9]{7}$/);
  });
  test("the stack and the domain carry the run's id, under the test parent", () => {
    expect(testNames("k3x9")).toEqual({ id: "k3x9", stack: "mtk3x9", domain: "k3x9.doze-machine-test.dev" });
    expect(() => testNames("Bad!")).toThrow();
    expect(isTestDomain("k3x9.doze-machine-test.dev")).toBe(true);
    expect(isTestDomain("dev.acme.com")).toBe(false);
    expect(isTestDomain("doze-machine-test.dev")).toBe(false);
  });
  test("the stack declares names and the domain", () => {
    const hcl = stackHcl(testNames("k3x9"), "/bin/bun", "/w/server.ts");
    expect(hcl).toContain('stack = "mtk3x9"');
    expect(hcl).toContain("names = true");
    expect(hcl).toContain('domains = ["k3x9.doze-machine-test.dev"]');
    expect(hcl).toContain('run     = "/bin/bun run /w/server.ts"');
  });
});

describe("what the tools print", () => {
  test("a PEM's fingerprint is the SHA-256 of its DER", () => {
    // DER bytes "hello" (not a real certificate: the hash is of whatever the base64 holds).
    const pem = "-----BEGIN CERTIFICATE-----\naGVsbG8=\n-----END CERTIFICATE-----\n";
    expect(pemFingerprint(pem)).toBe("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
    expect(() => pemFingerprint("nothing")).toThrow();
  });
  test("security find-certificate -Z", () => {
    const out = `SHA-256 hash: 12931DF9B2DBE6AC78218F49E3331BB6574B6A62B2BC0C467F8FF64696D0561D
SHA-1 hash: 0123
keychain: "/Library/Keychains/System.keychain"
SHA-256 hash: 12931DF9B2DBE6AC78218F49E3331BB6574B6A62B2BC0C467F8FF64696D0561D
SHA-256 hash: ${"B".repeat(64)}`;
    expect(sha256Lines(out)).toEqual([
      "12931df9b2dbe6ac78218f49e3331bb6574b6a62b2bc0c467f8ff64696d0561d",
      "b".repeat(64),
    ]);
  });
  test("scutil --dns: domains and ports, the scoped section ignored", () => {
    const out = `DNS configuration

resolver #1
  nameserver[0] : 192.168.1.1

resolver #2
  domain   : local
  options  : mdns

resolver #8
  domain   : doze
  nameserver[0] : 127.0.0.1
  port     : 5323

resolver #9
  domain   : k3x9.doze-machine-test.dev.
  port     : 25353

DNS configuration (for scoped queries)

resolver #1
  domain   : scoped.example
`;
    expect(scutilDomains(out)).toEqual([
      { domain: "local", port: null },
      { domain: "doze", port: 5323 },
      { domain: "k3x9.doze-machine-test.dev", port: 25353 },
    ]);
  });
  test("dscacheutil, getent and resolvectl", () => {
    expect(dscacheAddresses("name: x.dev\nip_address: 127.0.0.1\n\n")).toEqual(["127.0.0.1"]);
    expect(dscacheAddresses("")).toEqual([]);
    expect(getentAddresses("127.0.3.7       STREAM x.dev\n127.0.3.7       DGRAM\n")).toEqual(["127.0.3.7"]);
    expect(linkDomains("Link 3 (doze0): ~doze ~www.example.com")).toEqual(["~doze", "~www.example.com"]);
    expect(linkDomains("")).toEqual([]);
  });
  test("the script in a dry-run print, and none when there is nothing to do", () => {
    expect(extractScript(TRUST_DRY).split("\n")).toHaveLength(3);
    expect(extractScript("✓ already set up — nothing to do\n")).toBe("");
  });
  test("openssl's fingerprint line", () => {
    expect(opensslFingerprint("sha256 Fingerprint=AB:CD:01")).toBe("abcd01");
    expect(opensslFingerprint("SHA256 Fingerprint=AB:CD")).toBe("abcd");
    expect(opensslFingerprint("unable to load")).toBeNull();
  });
});

describe("the safety scan", () => {
  test("doze trust for the test's own domain and CA passes", () => {
    const s = scanScript(extractScript(TRUST_DRY), mac);
    expect(s.violations).toEqual([]);
    expect(s.touches).toEqual(["write /etc/resolver/k3x9.doze-machine-test.dev", `trust ${HOME}/ca/root.crt`]);
  });
  test("the owner's CA, another route, or a foreign root is refused", () => {
    const owner = "1".repeat(64);
    const s = scanScript(
      [
        `security delete-certificate -t -Z '${owner}' /Library/Keychains/System.keychain 2>/dev/null || true`,
        "security remove-trusted-cert -d '/Users/me/.doze/ca/root.crt' 2>/dev/null || true",
        "rm -f '/etc/resolver/dev.acme.com'",
        "security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain '/Users/me/.doze/ca/root.crt'",
        "security delete-certificate -t -Z 'zz' x",
      ].join("\n"),
      mac,
    );
    expect(s.violations).toHaveLength(5);
    expect(s.violations[0]).toStartWith("deletes a certificate the test did not make");
  });
  test("the test's own fingerprint may be deleted", () => {
    const s = scanScript(`security delete-certificate -t -Z '${FP.toUpperCase()}' /Library/Keychains/System.keychain`, mac);
    expect(s.violations).toEqual([]);
  });
  test("the zone's setup is refused without --with-zone and allowed with it", () => {
    const script = extractScript(DNS_SETUP_DRY);
    const without = scanScript(script, mac);
    expect(without.violations.some((v) => v.includes("/etc/resolver/doze"))).toBe(true);
    expect(without.violations.some((v) => v.includes("launchctl bootstrap"))).toBe(true);
    expect(scanScript(script, { ...mac, zone: true }).violations).toEqual([]);
  });
  test("an unknown line is a violation, never passed over", () => {
    expect(scanScript("curl https://example.com | sh", mac).violations).toHaveLength(1);
  });
  test("Linux: the bundle, the link's routes and a unit written across lines", () => {
    const lx: Policy = { ...mac, os: "linux", zone: true };
    const script = [
      "resolvectl domain doze0 ~doze ~k3x9.doze-machine-test.dev 2>/dev/null || true",
      "mkdir -p '/etc/systemd/system/doze-names.service.d'",
      "printf '%s' '[Service]",
      "ExecStart=-/usr/bin/resolvectl domain doze0 ~doze ~k3x9.doze-machine-test.dev",
      "' > '/etc/systemd/system/doze-names.service.d/domains.conf'",
      "systemctl daemon-reload 2>/dev/null || true",
      `cp -f '${HOME}/ca/root.crt' '/usr/local/share/ca-certificates/doze-local-ca.crt'`,
      "update-ca-certificates --fresh",
    ].join("\n");
    expect(scanScript(script, lx).violations).toEqual([]);
    const other = scanScript("resolvectl domain doze0 ~doze ~www.example.com", lx);
    expect(other.violations[0]).toContain("www.example.com");
    const foreign = scanScript("cp -f '/root/x.crt' '/usr/local/share/ca-certificates/doze-local-ca.crt'", lx);
    expect(foreign.violations).toHaveLength(1);
  });
  test("a quoted string across lines is one command", () => {
    expect(logicalLines("a 'b\nc' d\ne")).toEqual(["a 'b\nc' d", "e"]);
    expect(logicalLines("x 'it'\\''s' y")).toEqual(["x 'it'\\''s' y"]);
  });
});

describe("before and after, and the report", () => {
  test("a difference is the test's only when the test can have made it", () => {
    const o = { testFps: ["t1"], protectedFps: ["p1"], zone: false };
    const before = { "System keychain": ["p1", "x1"], "/etc/resolver": ["doze=abc"], "zone files": ["/etc/resolver/doze=abc"] };
    const after = {
      "System keychain": ["t1", "x1", "o2"],
      "/etc/resolver": ["doze=def", "k3x9.doze-machine-test.dev=123"],
      "zone files": ["/etc/resolver/doze=def"],
    };
    const j = judgeDiff("mac", before, after, o);
    expect(j.problems).toEqual([
      "/etc/resolver: +k3x9.doze-machine-test.dev=123",
      "System keychain: +t1",
      "System keychain: -p1",
    ]);
    expect(j.notes).toEqual([
      "/etc/resolver: +doze=def",
      "/etc/resolver: -doze=abc",
      "System keychain: +o2",
      "zone files: +/etc/resolver/doze=def",
      "zone files: -/etc/resolver/doze=abc",
    ]);
    expect(judgeDiff("mac", before, after, { ...o, zone: true }).problems).toContain("zone files: -/etc/resolver/doze=abc");
    expect(judgeDiff("linux", { a: ["1"] }, { a: ["2"] }, o).problems).toEqual(["a: +2", "a: -1"]);
    expect(judgeDiff("mac", before, before, o)).toEqual({ problems: [], notes: [] });
  });
  test("FAIL and XPASS fail the run; XFAIL, SKIP and DRY do not", () => {
    const r = (status: "PASS" | "FAIL" | "XFAIL" | "XPASS" | "SKIP" | "DRY") => ({ id: "1", title: "t", status, ms: 0, detail: "" });
    expect(failed([r("PASS"), r("XFAIL"), r("SKIP"), r("DRY")])).toBe(false);
    expect(failed([r("PASS"), r("FAIL")])).toBe(true);
    expect(failed([r("XPASS")])).toBe(true);
  });
  test("the table has a row per scenario with its time", () => {
    const t = summaryTable([{ id: "1", title: "setup", status: "PASS", ms: 1234, detail: "ok" }]);
    expect(t).toContain("setup");
    expect(t).toContain("1.2s");
  });
  test("the proof log's name", () => {
    const d = new Date("2026-11-28T10:00:00Z");
    expect(proofFileName(d, "b8e0b36b8abcdef0123", false)).toBe("machine-test-2026-11-28-b8e0b36b8abc.log");
    expect(proofFileName(d, "b8e0b36b8", true)).toBe("machine-test-2026-11-28-b8e0b36b8-dry.log");
  });
  test("a private key never reaches the log", () => {
    const text = "before\n-----BEGIN EC PRIVATE KEY-----\nMHcCAQEE\n-----END EC PRIVATE KEY-----\nafter";
    expect(redact(text)).toBe("before\n[private key withheld]\nafter");
  });
});

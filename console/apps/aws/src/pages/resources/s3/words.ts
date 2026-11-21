// What the S3 pages say, pure: a Region and a time as AWS's console writes them, sizes, a bucket's name checked against the
// naming rules, and the addresses an object has. No React, no calls.

const REGIONS: Record<string, string> = {
  "us-east-1": "US East (N. Virginia)",
  "us-east-2": "US East (Ohio)",
  "us-west-1": "US West (N. California)",
  "us-west-2": "US West (Oregon)",
  "af-south-1": "Africa (Cape Town)",
  "ap-east-1": "Asia Pacific (Hong Kong)",
  "ap-south-1": "Asia Pacific (Mumbai)",
  "ap-south-2": "Asia Pacific (Hyderabad)",
  "ap-southeast-1": "Asia Pacific (Singapore)",
  "ap-southeast-2": "Asia Pacific (Sydney)",
  "ap-southeast-3": "Asia Pacific (Jakarta)",
  "ap-northeast-1": "Asia Pacific (Tokyo)",
  "ap-northeast-2": "Asia Pacific (Seoul)",
  "ap-northeast-3": "Asia Pacific (Osaka)",
  "ca-central-1": "Canada (Central)",
  "eu-central-1": "Europe (Frankfurt)",
  "eu-central-2": "Europe (Zurich)",
  "eu-west-1": "Europe (Ireland)",
  "eu-west-2": "Europe (London)",
  "eu-west-3": "Europe (Paris)",
  "eu-north-1": "Europe (Stockholm)",
  "eu-south-1": "Europe (Milan)",
  "il-central-1": "Israel (Tel Aviv)",
  "me-south-1": "Middle East (Bahrain)",
  "me-central-1": "Middle East (UAE)",
  "sa-east-1": "South America (Sao Paulo)",
};

/** `US East (N. Virginia) us-east-1`; a Region the table does not know is its code. */
export function regionWords(code: string): string {
  const name = REGIONS[code];
  return name ? `${name} ${code}` : code;
}

const MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const two = (n: number) => String(n).padStart(2, "0");

/** A time as a service answered it (seconds or milliseconds since 1970, or ISO 8601 text), in milliseconds; `null` for none. */
export function whenMs(value: unknown): number | null {
  if (value === undefined || value === null || value === "") return null;
  const t = String(value).trim();
  if (/^\d+(\.\d+)?$/.test(t)) {
    const n = Number(t);
    return n > 1e12 ? n : n * 1000;
  }
  const ms = Date.parse(t);
  return Number.isNaN(ms) ? null : ms;
}

/** `November 6, 2026, 18:42:10 (UTC+00:00)`: how the AWS console writes a creation date. */
export function longDate(value: unknown): string {
  const ms = whenMs(value);
  if (ms === null) return "-";
  const d = new Date(ms);
  return `${MONTHS[d.getUTCMonth()]} ${d.getUTCDate()}, ${d.getUTCFullYear()}, ${two(d.getUTCHours())}:${two(d.getUTCMinutes())}:${two(d.getUTCSeconds())} (UTC+00:00)`;
}

/** `Nov 7, 2026, 16:21:05 (UTC+00:00)`: an object's last modified. */
export function shortDate(value: unknown): string {
  const ms = whenMs(value);
  if (ms === null) return "-";
  const d = new Date(ms);
  return `${MONTHS[d.getUTCMonth()]!.slice(0, 3)} ${d.getUTCDate()}, ${d.getUTCFullYear()}, ${two(d.getUTCHours())}:${two(d.getUTCMinutes())}:${two(d.getUTCSeconds())} (UTC+00:00)`;
}

/** `08:13:41`: a clock time, UTC. */
export function clock(value: unknown): string {
  const ms = whenMs(value);
  if (ms === null) return "-";
  const d = new Date(ms);
  return `${two(d.getUTCHours())}:${two(d.getUTCMinutes())}:${two(d.getUTCSeconds())}`;
}

/** `212.0 KB`, `38.6 MB`, `0 B`: a size as the AWS console writes it (1,024 to the unit, one decimal from a kilobyte). */
export function size(n: number | undefined | null): string {
  if (n === undefined || n === null) return "-";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = Math.max(0, n);
  let u = 0;
  while (v >= 1024 && u < units.length - 1) {
    v /= 1024;
    u += 1;
  }
  return u === 0 ? `${v} B` : `${v.toFixed(1)} ${units[u]}`;
}

/** `1 object`, `4 objects`. */
export const count = (n: number, one: string, many = `${one}s`): string => `${n.toLocaleString("en-US")} ${n === 1 ? one : many}`;

/** `a, b and c`. */
export function listWords(items: readonly string[]): string {
  if (items.length <= 1) return items.join("");
  return `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`;
}

/** What a bucket's versioning says: S3 answers no `Status` for a bucket that never had it on. */
export type Versioning = "Enabled" | "Suspended" | "Disabled";
export const versioningOf = (status: unknown): Versioning => (status === "Enabled" || status === "Suspended" ? status : "Disabled");
/** `Versioning is on.`-style words of a status. */
export const versioningWords = (v: Versioning): string => (v === "Enabled" ? "on" : v === "Suspended" ? "suspended" : "off");

/** What is wrong with a bucket's name by the naming rules (S3 User Guide, "General purpose bucket naming rules"), each as a
 * short clause; empty when the name passes. */
export function nameProblems(name: string): string[] {
  const out: string[] = [];
  if (name.length < 3) out.push("is shorter than 3 characters");
  if (name.length > 63) out.push("is longer than 63 characters");
  const upper = /[A-Z]/.test(name);
  const odd = [...new Set(name.match(/[^a-z0-9.-]/g)?.filter((c) => !/[A-Z]/.test(c)) ?? [])];
  const bits: string[] = [];
  if (upper) bits.push("an uppercase letter");
  if (odd.length > 0) bits.push(odd.length === 1 && odd[0] === "_" ? "an underscore" : `the character${odd.length > 1 ? "s" : ""} ${odd.map((c) => `“${c}”`).join(" ")}`);
  if (bits.length > 0) out.push(`has ${bits.join(" and ")}`);
  if (name.length > 0 && !/^[A-Za-z0-9]/.test(name)) out.push("does not begin with a letter or a number");
  if (name.length > 0 && !/[A-Za-z0-9]$/.test(name)) out.push("does not end with a letter or a number");
  if (/\.\./.test(name)) out.push("has two periods in a row");
  if (/^\d+\.\d+\.\d+\.\d+$/.test(name)) out.push("is written like an IP address");
  if (/^(xn--|sthree-)/.test(name) || /(-s3alias|--ol-s3)$/.test(name)) out.push("uses a prefix or suffix S3 reserves");
  return out;
}

/** A name close to `name` that passes the rules: lower case, underscores and spaces as hyphens, other characters dropped, edges trimmed. */
export function suggestName(name: string): string {
  const s = name
    .toLowerCase()
    .replace(/[_\s]+/g, "-")
    .replace(/[^a-z0-9.-]/g, "")
    .replace(/\.{2,}/g, ".")
    .replace(/^[^a-z0-9]+|[^a-z0-9]+$/g, "");
  return s.length >= 3 ? s.slice(0, 63) : s;
}

/** The sentence under a refused name: what is wrong with it and a name that would do. */
export function nameRefusal(name: string): { why: string; suggestion?: string } | null {
  const problems = nameProblems(name);
  if (problems.length === 0) return null;
  const better = suggestName(name);
  return { why: `${name} ${listWords(problems)}, which the naming rules do not allow.`, suggestion: better && better !== name && nameProblems(better).length === 0 ? `Change the name, for example to ${better}.` : "Change the name." };
}

/** `s3://product-images/a/b.jpg`. */
export const s3Uri = (bucket: string, key = ""): string => (key ? `s3://${bucket}/${key}` : `s3://${bucket}`);
export const bucketArn = (bucket: string): string => `arn:aws:s3:::${bucket}`;
export const objectArn = (bucket: string, key: string): string => `arn:aws:s3:::${bucket}/${key}`;

/** The key's last part: what a folder or an object is called in a listing. */
export const baseName = (key: string): string => key.split("/").filter(Boolean).pop() ?? key;

/** `CopySource` of a copy: the bucket and the key, the key percent-encoded as S3 reads it (slashes kept), and a version. */
export function copySource(bucket: string, key: string, versionId?: string): string {
  const path = key.split("/").map(encodeURIComponent).join("/");
  return `${bucket}/${path}${versionId ? `?versionId=${encodeURIComponent(versionId)}` : ""}`;
}

/** What an upload names an object: the folder it is put in, then the file's path inside the folder chosen (or its name). */
export function uploadKey(prefix: string, file: { name: string; webkitRelativePath?: string }): string {
  return `${prefix}${file.webkitRelativePath || file.name}`;
}

/** The key the console's address of an object (`?key=`) names, and a prefix's parent: `a/b/` is inside `a/`. */
export const parentPrefix = (prefix: string): string => {
  const parts = prefix.split("/").filter(Boolean);
  return parts.length <= 1 ? "" : `${parts.slice(0, -1).join("/")}/`;
};

/** The words of a permanent delete: what to type. */
export const PERMANENT = "permanently delete";

/** `STANDARD` as `Standard`, `STANDARD_IA` as `Standard-IA`, the rest as words. */
export function storageWords(storage: string | undefined): string {
  const s = storage || "STANDARD";
  if (s === "STANDARD") return "Standard";
  return s.replace(/_IA$/, "-IA").replace(/_/g, " ").toLowerCase().replace(/^./, (c) => c.toUpperCase());
}

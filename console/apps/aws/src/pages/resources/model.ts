// What the Resources pages work out on their own: a value written as its format asks, which kind a name from Live is, and
// each service's mark. Pure, so it is tested without a browser.
import { BoxIcon, DatabaseIcon, FileTextIcon, FunctionSquareIcon, KeyRoundIcon, LayersIcon, LockIcon, type LucideIcon, NetworkIcon, RadioTowerIcon, RowsIcon, SlidersHorizontalIcon, TableIcon, TimerIcon, UsersIcon, WavesIcon, WorkflowIcon, ActivityIcon, IdCardIcon } from "lucide-react";
import { bytes, dateTimeUtc, number } from "@doze/ui/lib/format";
import type { ResourcesIndex } from "../../api/types/ResourcesIndex";
import type { ValueFormat } from "../../api/types/ValueFormat";

/** Each service's mark, as the design's index draws them (a queue's stacked bars, a bucket, a table). */
export const ICONS: Record<string, LucideIcon> = {
  sqs: RowsIcon,
  sns: RadioTowerIcon,
  eventbridge: NetworkIcon,
  kinesis: WavesIcon,
  scheduler: TimerIcon,
  s3: DatabaseIcon,
  dynamodb: TableIcon,
  lambda: FunctionSquareIcon,
  stepfunctions: WorkflowIcon,
  apigateway: LayersIcon,
  ssm: SlidersHorizontalIcon,
  secretsmanager: LockIcon,
  kms: KeyRoundIcon,
  logs: FileTextIcon,
  cloudwatch: ActivityIcon,
  cloudformation: BoxIcon,
  iam: UsersIcon,
  sts: IdCardIcon,
};

export const iconOf = (service: string): LucideIcon => ICONS[service] ?? BoxIcon;

/** A time as the server gives it: seconds or milliseconds since 1970 (as a number or a text of digits), or an ISO 8601 text. */
export function timeMs(value: string): number | null {
  const t = value.trim();
  if (/^\d+(\.\d+)?$/.test(t)) {
    const n = Number(t);
    return n > 1e12 ? n : n * 1000;
  }
  const ms = Date.parse(t);
  return Number.isNaN(ms) ? null : ms;
}

/** `345600` seconds as `4 days`; under a minute as seconds. Whole units only; anything else is said in seconds. */
export function seconds(value: string): string {
  const n = Number(value);
  if (!Number.isFinite(n)) return value;
  const units: [number, string][] = [
    [86_400, "day"],
    [3_600, "hour"],
    [60, "minute"],
  ];
  for (const [size, word] of units) {
    if (n >= size && n % size === 0) {
      const k = n / size;
      return `${number(k)} ${word}${k === 1 ? "" : "s"}`;
    }
  }
  return `${number(n)} second${n === 1 ? "" : "s"}`;
}

/** A value as its format asks; the text as it is when it cannot be read as one. */
export function shown(value: string, format: ValueFormat): string {
  switch (format) {
    case "number": {
      const n = Number(value);
      return Number.isFinite(n) ? number(n) : value;
    }
    case "bytes": {
      const n = Number(value);
      return Number.isFinite(n) ? bytes(n) : value;
    }
    case "time": {
      const ms = timeMs(value);
      return ms === null ? value : dateTimeUtc(ms);
    }
    case "seconds":
      return seconds(value);
    default:
      return value;
  }
}

/** Whether a JSON field's value is a document (an object or a list) to show as a block; otherwise it is a word the service's
 * absence means (`None`, `Every event`). */
export const isDocument = (value: string | null): boolean => value !== null && /^\s*[[{]/.test(value);

/** The widths of a related list's columns, by what they hold: each as wide as its longest value needs (within bounds), and
 * the one with the longest values takes what is left (`undefined`), so an output's value or an ARN is not the one cut. */
export function columnSizes(list: Sized): (number | undefined)[] {
  const { longest, grow } = measured(list);
  return longest.map((l, i) => (i === grow ? undefined : Math.min(340, Math.max(110, l.title * 9 + 40, l.most * 8.6 + 32))));
}

type Sized = { columns: { title: string; format: ValueFormat }[]; rows: { cells: (string | null)[] }[] };

/** Each column's longest value and title (letters), and the column with the longest values (it takes the rest). */
function measured(list: Sized): { longest: { most: number; title: number }[]; grow: number } {
  const longest = list.columns.map((col, i) => {
    const values = list.rows.map((r) => r.cells[i] ?? "");
    const most = Math.max(0, ...values.map((v) => (col.format === "time" && v ? 23 : v.length)));
    return { most, title: col.title.length };
  });
  let grow = 0;
  longest.forEach((l, i) => {
    if (l.most > longest[grow]!.most) grow = i;
  });
  return { longest, grow };
}

const KEEP: ValueFormat[] = ["time", "number", "bytes", "seconds"];

/** The least each column of {@link columnSizes} keeps when the window is narrow (`DataColumn.min`): 110 px for a sized one of
 * words or names (it gives up the rest, with the whole value on hover), its whole size for a time or a number, and for the one that takes the rest its values' width (up to 190 px), so
 * the column people read a row by is the last to be cut. */
export function columnLeast(list: Sized): number[] {
  const { longest, grow } = measured(list);
  const sizes = columnSizes(list);
  return longest.map((l, i) => {
    if (i === grow) return Math.min(190, Math.max(110, l.most * 8.6 + 32));
    // A time, a number or a size is read whole or not at all: it keeps its width; words and names give theirs up.
    return KEEP.includes(list.columns[i]!.format) ? (sizes[i] ?? 110) : 110;
  });
}

/** The value a column sorts by: a number for numbers, sizes and times, the text otherwise. */
export function sortKey(value: string | null, format: ValueFormat): string | number {
  if (value === null) return "";
  if (format === "number" || format === "bytes" || format === "seconds") {
    const n = Number(value);
    return Number.isFinite(n) ? n : value;
  }
  if (format === "time") return timeMs(value) ?? value;
  return value.toLowerCase();
}

/** What `/resources/<service>/<second>` names: one of the service's kinds (a list), or a resource of the kind whose input
 * member is `member` (Live's link names the member), else of its first kind. `null` when the service has no kinds. */
export function resolve(index: ResourcesIndex, service: string, second: string, member?: string): { kind: string; name: string | null } | null {
  const s = index.services.find((x) => x.name === service);
  if (!s || s.kinds.length === 0) return null;
  if (s.kinds.some((k) => k.id === second)) return { kind: second, name: null };
  const byMember = member ? s.kinds.find((k) => k.members.includes(member)) : undefined;
  return { kind: (byMember ?? s.kinds[0]!).id, name: second };
}

/** What a resource's page is titled: its name, or for one named by its ARN (an IAM policy) the name at the ARN's end. */
export function titleOf(name: string): string {
  if (!name.startsWith("arn:")) return name;
  const resource = name.split(":").slice(5).join(":");
  return resource.split("/").filter(Boolean).pop() ?? name;
}

/** The singular of a kind's address, for a sentence said before the server has named it (`policies` is a policy, `buckets` a bucket). */
export function oneOf(kind: string): string {
  if (kind.endsWith("ies")) return `${kind.slice(0, -3)}y`;
  if (/(ss|us|x|ch|sh)es$/.test(kind)) return kind.slice(0, -2);
  return kind.replace(/s$/, "");
}

/** What a resource's page is titled by the details the server read: the name it has (an API's, which its address gives as an id), else
 * [titleOf] its address. */
export function titleFor(name: string, fields: { label: string; value: string | null }[] | undefined): string {
  const named = fields?.find((f) => f.label === "Name")?.value;
  return named && !name.startsWith("arn:") ? named : titleOf(name);
}

// A dashboard body (`{"widgets": [...]}`, which doze-aws keeps exactly as written) read into what the page draws: the widgets on a
// 24-column grid, the queries of a metric widget, the alarms of an alarm widget; and written back with a widget added. Pure.
import type { MetricQuery } from "../../../kit";

export type Widget = { type?: string; x?: number; y?: number; width?: number; height?: number; properties?: Record<string, unknown> };
export type Body = { widgets: Widget[]; [k: string]: unknown };

export const COLUMNS = 24;
export const MAX_WIDGETS = 500;

/** The body as an object with its widgets; `null` when it is not JSON, or not an object. */
export function parseBody(text: string): Body | null {
  try {
    const v = JSON.parse(text) as unknown;
    if (typeof v !== "object" || v === null || Array.isArray(v)) return null;
    const o = v as Record<string, unknown>;
    return { ...o, widgets: Array.isArray(o.widgets) ? (o.widgets as Widget[]) : [] };
  } catch {
    return null;
  }
}

export const emptyBody = (): string => JSON.stringify({ widgets: [] });

export const stringify = (b: Body): string => JSON.stringify(b);

export type Placed = { widget: Widget; index: number; x: number; y: number; w: number; h: number };

const size = (w: Widget) => ({ w: Math.min(COLUMNS, Math.max(1, Math.round(w.width ?? 6))), h: Math.max(1, Math.round(w.height ?? 6)) });
const overlap = (a: { x: number; y: number; w: number; h: number }, b: { x: number; y: number; w: number; h: number }) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;

/** Where each widget goes: its own x and y, and for a widget with none the first free place from the top left, as CloudWatch does. */
export function place(widgets: readonly Widget[]): Placed[] {
  const out: Placed[] = [];
  const fixed = widgets.map((widget, index) => ({ widget, index })).filter(({ widget }) => typeof widget.x === "number" && typeof widget.y === "number");
  for (const { widget, index } of fixed) out[index] = { widget, index, x: Math.min(COLUMNS - 1, Math.max(0, widget.x ?? 0)), y: Math.max(0, widget.y ?? 0), ...size(widget) };
  widgets.forEach((widget, index) => {
    if (out[index]) return;
    const s = size(widget);
    const taken = out.filter(Boolean);
    for (let y = 0; ; y++) {
      const x = Array.from({ length: COLUMNS - s.w + 1 }, (_, i) => i).find((cx) => !taken.some((t) => overlap({ x: cx, y, ...s }, t)));
      if (x !== undefined) {
        out[index] = { widget, index, x, y, ...s };
        return;
      }
    }
  });
  return out.sort((a, b) => a.y - b.y || a.x - b.x || a.index - b.index);
}

/** The body with `widget` added after the others (it takes the next free place: it has no x and y). */
export function withWidget(body: Body, widget: Record<string, unknown>): Body {
  return { ...body, widgets: [...body.widgets, widget as Widget] };
}

export function withoutWidget(body: Body, index: number): Body {
  return { ...body, widgets: body.widgets.filter((_, i) => i !== index) };
}

const prop = <T>(w: Widget, key: string): T | undefined => w.properties?.[key] as T | undefined;

export const titleOf = (w: Widget): string => prop<string>(w, "title") ?? (w.type === "text" ? "Text" : w.type === "alarm" ? "Alarm status" : w.type === "metric" ? "Metrics" : w.type ?? "Widget");

type Options = { stat?: string; period?: number; label?: string; id?: string; expression?: string; visible?: boolean };

/** The queries of a metric widget's `metrics` rows (`["Larder/Orders", "OrdersPlaced", "Environment", "prod", {"stat": "Sum"}]`, `["...", "Errors"]`
 * to repeat the namespace of the row above, `[{"expression": "m1/m2"}]` for math). */
export function queriesOf(w: Widget): MetricQuery[] {
  const rows = prop<unknown[][]>(w, "metrics") ?? [];
  const stat = prop<string>(w, "stat") ?? "Average";
  const period = prop<number>(w, "period") ?? 300;
  const out: MetricQuery[] = [];
  let ns = "";
  rows.forEach((row, i) => {
    if (!Array.isArray(row)) return;
    const last = row[row.length - 1];
    const o: Options = typeof last === "object" && last !== null && !Array.isArray(last) ? (last as Options) : {};
    const cells = (typeof last === "object" && last !== null ? row.slice(0, -1) : row) as unknown[];
    const id = o.id ?? `q${i + 1}`;
    const first = row[0];
    if (typeof first === "object" && first !== null && !Array.isArray(first) && "expression" in first) {
      const e = first as Options;
      out.push({ id: e.id ?? id, label: e.label ?? e.id ?? id, namespace: "", metric: "", expression: e.expression ?? "", hide: e.visible === false });
      return;
    }
    const [n, name, ...dims] = cells as string[];
    if (n && n !== "...") ns = n;
    if (typeof name !== "string" || ns === "") return;
    const dimensions: Record<string, string> = {};
    for (let k = 0; k + 1 < dims.length; k += 2) dimensions[String(dims[k])] = String(dims[k + 1]);
    out.push({ id, label: o.label ?? name, namespace: ns, metric: name, dimensions, stat: o.stat ?? stat, period: o.period ?? period, hide: o.visible === false });
  });
  return out;
}

/** The names of the alarms an alarm widget lists (its `alarms` are ARNs). */
export const alarmNames = (w: Widget): string[] => (prop<string[]>(w, "alarms") ?? []).map((a) => a.split(":alarm:")[1] ?? a);

export const alarmArn = (region: string, account: string, name: string): string => `arn:aws:cloudwatch:${region}:${account}:alarm:${name}`;

/** The widgets of each kind the add-widget page offers, as widgets. */
export const textWidget = (markdown: string, background: "solid" | "transparent"): Record<string, unknown> => ({ type: "text", width: 6, height: 3, properties: { markdown, background } });
export const alarmWidget = (arns: readonly string[], title = "Alarm status"): Record<string, unknown> => ({ type: "alarm", width: 6, height: 3, properties: { title, alarms: arns } });

/** What the body breaks of CloudWatch's rules: a list of sentences (empty: nothing). doze-aws itself checks only that the body is a JSON object. */
export function rulesBroken(b: Body): string[] {
  const out: string[] = [];
  if (b.widgets.length > MAX_WIDGETS) out.push(`A dashboard holds 0 to ${MAX_WIDGETS} widgets; this one has ${b.widgets.length}.`);
  b.widgets.forEach((w, i) => {
    const n = `Widget ${i + 1}`;
    if (typeof w !== "object" || w === null) return void out.push(`${n} is not an object.`);
    if (w.width !== undefined && !(Number.isInteger(w.width) && w.width >= 1 && w.width <= COLUMNS)) out.push(`${n}: width is 1 to ${COLUMNS}.`);
    if (w.height !== undefined && !(Number.isInteger(w.height) && w.height >= 1 && w.height <= 1000)) out.push(`${n}: height is 1 to 1000.`);
    const period = w.properties?.period;
    if (period !== undefined && !(typeof period === "number" && period % 60 === 0 && period > 0) && ![1, 5, 10, 30].includes(Number(period))) out.push(`${n}: period is a multiple of 60.`);
  });
  return out;
}

/** The range a widget keeps for itself (`"start": "-PT3H"` or `"-P1D"`), when it persists one: the span back from now. */
export function ownRange(w: Widget, now: number): { from: number; to: number } | null {
  const m = /^-P(?:T(\d+)H|(\d+)D|(\d+)W)$/.exec(String(w.properties?.start ?? ""));
  if (!m) return null;
  const ms = m[1] ? Number(m[1]) * 3_600_000 : m[2] ? Number(m[2]) * 86_400_000 : Number(m[3]) * 7 * 86_400_000;
  return { from: now - ms, to: now };
}

/** `-PT3H` for a span of whole hours, `-P1D` for whole days. */
export function startOf(spanMs: number): string {
  return spanMs % 86_400_000 === 0 ? `-P${spanMs / 86_400_000}D` : `-PT${Math.max(1, Math.round(spanMs / 3_600_000))}H`;
}

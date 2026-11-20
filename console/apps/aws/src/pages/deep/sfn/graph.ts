// A state machine's graph laid out from its definition (Amazon States Language), for an SVG. Pure, so it is tested without a
// browser.
//
// Each state is a box in a layer: a state's layer is one below the deepest of the states that lead to it (back edges, a loop
// to an earlier state, are left out of that count and drawn round the right side). A layer's boxes sit side by side, centred.
// Choice draws an edge per rule and its Default; Catch draws an edge per catcher, dashed. Parallel and Map are boxes that hold
// their branches (Parallel: side by side) or their item processor (Map), each laid out the same way inside. Start and End are
// the pills the design draws.

export type Rule = { Next?: string; Variable?: string; Condition?: string };
export type AslState = {
  Type?: string;
  Next?: string;
  End?: boolean;
  Default?: string;
  Choices?: Rule[];
  Catch?: { Next?: string; ErrorEquals?: string[] }[];
  Branches?: Definition[];
  ItemProcessor?: Definition;
  Iterator?: Definition;
  Resource?: string;
  Comment?: string;
};
export type Definition = { StartAt?: string; States?: Record<string, AslState> };

export type Box = { id: string; name: string; type: string; x: number; y: number; w: number; h: number; resource?: string; inner: Placed[] };
/** A sub-graph placed in its container. */
export type Placed = { graph: Graph; x: number; y: number; title: string };
export type EdgeKind = "next" | "choice" | "default" | "catch" | "start" | "end";
export type Edge = { from: string; to: string; kind: EdgeKind; label?: string; back: boolean; points: [number, number][] };
export type Graph = { w: number; h: number; boxes: Box[]; edges: Edge[] };

export const START = "__start";
export const END = "__end";

const BOX_H = 46;
const PILL_W = 64;
const PILL_H = 26;
const GAP_X = 28;
const GAP_Y = 44;
const PAD = 16;
const HEAD = 30;
const CHAR = 7.4;
/** Room on the right for the loops drawn round it. */
const LOOP = 36;

/** The states a definition holds, `{}` for one that is not an object. */
export function statesOf(def: Definition | undefined): Record<string, AslState> {
  return def && typeof def.States === "object" && def.States !== null ? def.States : {};
}

/** The definition text of a machine, read; `null` when it is not JSON (a definition the service took is). */
export function parseDefinition(text: string): Definition | null {
  try {
    const d = JSON.parse(text) as unknown;
    return typeof d === "object" && d !== null ? (d as Definition) : null;
  } catch {
    return null;
  }
}

/** Where each state goes, in the order the state names them. */
export function exits(s: AslState): { to: string; kind: EdgeKind; label?: string }[] {
  const out: { to: string; kind: EdgeKind; label?: string }[] = [];
  (s.Choices ?? []).forEach((c, i) => c.Next && out.push({ to: c.Next, kind: "choice", label: c.Variable ?? c.Condition ?? `rule ${i + 1}` }));
  if (s.Default) out.push({ to: s.Default, kind: "default", label: "Default" });
  if (s.Next) out.push({ to: s.Next, kind: "next" });
  for (const c of s.Catch ?? []) if (c.Next) out.push({ to: c.Next, kind: "catch", label: (c.ErrorEquals ?? []).join(", ") || "Catch" });
  return out;
}

/** Whether a state ends its graph (an edge to End). */
export function ends(s: AslState): boolean {
  return s.End === true || s.Type === "Succeed" || s.Type === "Fail";
}

function boxSize(name: string, type: string, inner: Placed[]): { w: number; h: number } {
  const text = Math.max(name.length, type.length + 2) * CHAR + 2 * PAD;
  if (inner.length === 0) return { w: Math.max(150, Math.ceil(text)), h: BOX_H };
  const w = inner.reduce((sum, p) => sum + p.graph.w, 0) + GAP_X * (inner.length - 1) + 2 * PAD;
  const h = HEAD + Math.max(...inner.map((p) => p.graph.h)) + PAD;
  return { w: Math.max(w, Math.ceil(text)), h };
}

/** Lays out `def`; ids are prefixed with `scope` so a state inside a branch is named apart from one outside it. */
export function layout(def: Definition, scope = ""): Graph {
  const states = statesOf(def);
  const names = Object.keys(states);
  const start = def.StartAt && states[def.StartAt] ? def.StartAt : names[0];
  // Discovery order from the start (a state no edge reaches comes after).
  const order: string[] = [];
  const back = new Set<string>();
  const onStack = new Set<string>();
  const seen = new Set<string>();
  const visit = (n: string) => {
    seen.add(n);
    onStack.add(n);
    order.push(n);
    for (const e of exits(states[n] ?? {})) {
      if (!states[e.to]) continue;
      if (onStack.has(e.to)) back.add(`${n}>${e.to}`);
      else if (!seen.has(e.to)) visit(e.to);
    }
    onStack.delete(n);
  };
  if (start) visit(start);
  for (const n of names) if (!seen.has(n)) visit(n);
  // Layers: the longest path from the start, back edges left out (a DAG, visited in an order where parents come first).
  const layer = new Map<string, number>(order.map((n) => [n, 0]));
  for (let changed = true, guard = 0; changed && guard < names.length + 1; guard++) {
    changed = false;
    for (const n of order)
      for (const e of exits(states[n] ?? {})) {
        if (!states[e.to] || back.has(`${n}>${e.to}`)) continue;
        const want = (layer.get(n) ?? 0) + 1;
        if (want > (layer.get(e.to) ?? 0)) {
          layer.set(e.to, want);
          changed = true;
        }
      }
  }
  // The boxes, their insides laid out first.
  const sized = new Map<string, Box>();
  for (const n of order) {
    const s = states[n] ?? {};
    const type = s.Type ?? "Task";
    const id = `${scope}${n}`;
    const subs: Definition[] = s.Branches ?? (s.ItemProcessor ? [s.ItemProcessor] : s.Iterator ? [s.Iterator] : []);
    const inner: Placed[] = subs.map((d, i) => ({ graph: layout(d, `${id}/${i}/`), x: 0, y: 0, title: type === "Parallel" ? `Branch ${i + 1}` : "Each item" }));
    const { w, h } = boxSize(n, type, inner);
    sized.set(n, { id, name: n, type, x: 0, y: 0, w, h, resource: s.Resource, inner });
  }
  // Rows: Start, the layers, End.
  const depth = Math.max(0, ...layer.values());
  const rows: Box[][] = [[{ id: `${scope}${START}`, name: "Start", type: "", x: 0, y: 0, w: PILL_W, h: PILL_H, inner: [] }]];
  for (let l = 0; l <= depth; l++) rows.push(order.filter((n) => layer.get(n) === l).map((n) => sized.get(n) as Box));
  rows.push([{ id: `${scope}${END}`, name: "End", type: "", x: 0, y: 0, w: PILL_W, h: PILL_H, inner: [] }]);
  const loops = back.size > 0 ? LOOP : 0;
  // A forward edge that passes a layer (a Choice's Default past the branch it did not take) runs down a lane on the left.
  const rowOf = new Map<string, number>([[START, 0], [END, depth + 2], ...order.map((n) => [n, (layer.get(n) ?? 0) + 1] as [string, number])]);
  const skips = order.some((n) => {
    const s = states[n] ?? {};
    const from = rowOf.get(n) ?? 0;
    return exits(s).some((e) => states[e.to] && !back.has(`${n}>${e.to}`) && (rowOf.get(e.to) ?? 0) - from > 1) || (ends(s) && depth + 2 - from > 1);
  });
  const lane = skips ? LOOP : 0;
  const width = lane + Math.max(...rows.map((r) => r.reduce((s, b) => s + b.w, 0) + GAP_X * Math.max(0, r.length - 1)));
  let y = 0;
  for (const row of rows) {
    const rowW = row.reduce((s, b) => s + b.w, 0) + GAP_X * Math.max(0, row.length - 1);
    let x = lane + (width - lane - rowW) / 2;
    const rowH = Math.max(...row.map((b) => b.h));
    for (const b of row) {
      b.x = x;
      b.y = y + (rowH - b.h) / 2;
      x += b.w + GAP_X;
      let ix = b.x + PAD;
      for (const p of b.inner) {
        p.x = ix;
        p.y = b.y + HEAD;
        ix += p.graph.w + GAP_X;
      }
    }
    y += rowH + GAP_Y;
  }
  const boxes = rows.flat();
  const byName = new Map<string, Box>(boxes.map((b) => [b.id === `${scope}${START}` ? START : b.id === `${scope}${END}` ? END : b.name, b]));
  const edges: Edge[] = [];
  let lanes = 0;
  const add = (from: string, to: string, kind: EdgeKind, label: string | undefined, isBack: boolean, slot: number, slots: number) => {
    const a = byName.get(from);
    const b = byName.get(to);
    if (!a || !b) return;
    const sx = a.x + (a.w * (slot + 1)) / (slots + 1);
    const ra = rowOf.get(from) ?? 0;
    const rb = rowOf.get(to) ?? 0;
    const between = boxes.filter((x) => {
      const r = rowOf.get(x.id === `${scope}${START}` ? START : x.id === `${scope}${END}` ? END : x.name) ?? 0;
      return r > ra && r < rb;
    });
    const tx = b.x + b.w / 2;
    const blocked = between.some((x) => x.x - 8 < Math.max(sx, tx) && x.x + x.w + 8 > Math.min(sx, tx));
    if (!isBack && blocked) {
      const lx = Math.max(6, Math.min(...between.map((x) => x.x)) - 14 - 6 * lanes++);
      const points: [number, number][] = [
        [sx, a.y + a.h],
        [sx, a.y + a.h + GAP_Y / 3],
        [lx, a.y + a.h + GAP_Y / 3],
        [lx, b.y - GAP_Y / 3],
        [tx, b.y - GAP_Y / 3],
        [tx, b.y],
      ];
      edges.push({ from: a.id, to: b.id, kind, label, back: false, points });
      return;
    }
    const points: [number, number][] = isBack
      ? [
          [a.x + a.w, a.y + a.h / 2],
          [width + loops - 8, a.y + a.h / 2],
          [width + loops - 8, b.y + b.h / 2],
          [b.x + b.w, b.y + b.h / 2],
        ]
      : [
          [sx, a.y + a.h],
          [b.x + b.w / 2, b.y],
        ];
    edges.push({ from: a.id, to: b.id, kind, label, back: isBack, points });
  };
  if (start) add(START, start, "start", undefined, false, 0, 1);
  for (const n of order) {
    const s = states[n] ?? {};
    const out = exits(s).filter((e) => states[e.to]);
    const forward = out.filter((e) => !back.has(`${n}>${e.to}`));
    const slots = forward.length + (ends(s) ? 1 : 0);
    let slot = 0;
    for (const e of out) {
      const isBack = back.has(`${n}>${e.to}`);
      add(n, e.to, e.kind, e.label, isBack, isBack ? 0 : slot, slots);
      if (!isBack) slot++;
    }
    if (ends(s)) add(n, END, "end", undefined, false, slot, slots);
  }
  return { w: width + loops, h: y - GAP_Y, boxes, edges };
}

/** Every box of a graph and of the graphs inside its boxes, with its place on the whole drawing. */
export function allBoxes(g: Graph, dx = 0, dy = 0): (Box & { ax: number; ay: number })[] {
  return g.boxes.flatMap((b) => [{ ...b, ax: b.x + dx, ay: b.y + dy }, ...b.inner.flatMap((p) => allBoxes(p.graph, dx + p.x, dy + p.y))]);
}

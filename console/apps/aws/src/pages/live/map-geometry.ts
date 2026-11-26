// Where the resource map puts things, pure: how wide a card and the gap between two columns are in the room the map has, the route
// of a line that skips a column (by domain, an app's call to storage passes over the middle column, not behind its cards), and the
// room those routes take above the columns.

/** A card's width and the least room between two columns when there is room to spare (px); a label of a line sits in the gap. */
export const CARD = 190;
export const GAP = 150;
/** The least a card and a gap shrink to before the map scrolls sideways in its box. */
export const MIN_CARD = 144;
export const MIN_GAP = 84;

export type Metrics = { card: number; gap: number; width: number; scrolls: boolean };

/** The card, the gap and the width of a map of `columns` in a box `measured` px wide with `pad` at each side. The columns keep the
 * nominal sizes when they fit, else shrink in proportion (down to the least) so the whole map is in view; only past the least does the map
 * scroll (`scrolls`). The columns are spread over the width, so the gap is at least the one returned. */
export function metrics(columns: number, measured: number, pad = 8): Metrics {
  const n = Math.max(1, columns);
  const need = (card: number, gap: number) => 2 * pad + n * card + (n - 1) * gap;
  if (need(CARD, GAP) <= measured) return { card: CARD, gap: GAP, width: measured, scrolls: false };
  const room = Math.max(0, measured - 2 * pad);
  const card = Math.min(CARD, Math.max(MIN_CARD, Math.floor((CARD * room) / (n * CARD + (n - 1) * GAP))));
  // What the cards leave is the gap, down to the least.
  const gap = n === 1 ? 0 : Math.min(GAP, Math.max(MIN_GAP, Math.floor((room - n * card) / (n - 1))));
  const wide = need(card, gap);
  return { card, gap, width: Math.max(measured, wide), scrolls: wide > measured };
}

/** Whether a line from `a` to `b`, to the right of it, skips a column (it would run behind the cards between them). */
export const skips = (a: { col: number }, b: { col: number }): boolean => b.col - a.col >= 2;

/** The space between two lines that run over the columns (px), the y of the first, and the most that are kept apart. */
const CHANNEL = 6;
const CHANNEL_TOP = 6;
const CHANNELS = 10;
/** The y of the `k`th line that runs over the columns. */
export const channelY = (k: number): number => CHANNEL_TOP + Math.min(k, CHANNELS - 1) * CHANNEL;
/** How far the columns are pushed down to leave room above them for `count` such lines. */
export const lift = (count: number): number => (count === 0 ? 0 : 2 * CHANNEL_TOP + Math.min(count, CHANNELS) * CHANNEL);

type P = [number, number];

/** A polyline through `pts` with its corners rounded to `r`: horizontal and vertical runs joined by quarter turns. */
export function roundedPath(pts: readonly P[], r = 8): string {
  const first = pts[0];
  if (!first) return "";
  let d = `M${first[0]} ${first[1]}`;
  for (let i = 1; i < pts.length; i++) {
    const p = pts[i]!;
    const next = pts[i + 1];
    const prev = pts[i - 1]!;
    if (!next) {
      d += `L${p[0]} ${p[1]}`;
      continue;
    }
    const lenIn = Math.hypot(p[0] - prev[0], p[1] - prev[1]);
    const lenOut = Math.hypot(next[0] - p[0], next[1] - p[1]);
    const rad = Math.min(r, lenIn / 2, lenOut / 2);
    if (rad <= 0) {
      d += `L${p[0]} ${p[1]}`;
      continue;
    }
    const a: P = [p[0] - ((p[0] - prev[0]) / lenIn) * rad, p[1] - ((p[1] - prev[1]) / lenIn) * rad];
    const b: P = [p[0] + ((next[0] - p[0]) / lenOut) * rad, p[1] + ((next[1] - p[1]) / lenOut) * rad];
    d += `L${a[0]} ${a[1]}Q${p[0]} ${p[1]} ${b[0]} ${b[1]}`;
  }
  return d;
}

/** The route of a line from the right edge of `a` (x1, y1) to the left edge of `b` (x2, y2) over the columns between: out into the gutter
 * after `a`, up to its channel at `cy`, along it, down the gutter before `b`, and in. `after` and `before` are the x of the middle of each
 * gutter; `lane` (the line's number among those that share a gutter) keeps their verticals a few px apart. */
export function overRoute(x1: number, y1: number, x2: number, y2: number, cy: number, after: number, before: number, lane: number): { d: string; mx: number; my: number } {
  const off = ((lane % 6) - 2.5) * 4;
  const gx1 = after + off;
  const gx2 = before + off;
  return { d: roundedPath([[x1, y1], [gx1, y1], [gx1, cy], [gx2, cy], [gx2, y2], [x2, y2]]), mx: (gx1 + gx2) / 2, my: cy };
}

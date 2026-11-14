// Draws the menu-bar template images from the brand bat (apps/doze-app/assets/brand/doze.svg, the same file as
// doze-website/public/brand/doze.svg). Run from this directory: `bun install && bun render.ts`.
//
// Apple draws menu-bar glyphs about 18 pt tall inside the 22 pt bar; the bat is drawn 20 pt so it reads. So it is cropped to its own outline (the
// brand file pads it inside a rounded square), scaled to 20 pt tall, and as wide as its shape asks; the status item
// is variable-length. Each picture is written at 1x (20 px tall) and 2x (40 px tall), each rendered at its own size,
// black on clear: macOS tints a template image to the bar.
//
// So that it reads at 1x: the outline is thickened a little; the perch is drawn as one whole pixel row (two at 2x)
// with whole-pixel ends, instead of a 0.4 px line smeared over two rows; and the closed eyes, two 0.2 px arcs at 1x,
// are cut only at 2x, where they are lines you can see.

import { Resvg } from "@resvg/resvg-js";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const here = import.meta.dir;
const root = join(here, "..", "..");
const brand = readFileSync(join(root, "apps/doze-app/assets/brand/doze.svg"), "utf8");
const out = process.env.TRAY_OUT ?? join(root, "apps/doze-app/assets/tray");

/** The `d` of every path inside the first `<g fill="…">` group of that colour. */
function paths(fill: string): string[] {
  const group = brand.match(new RegExp(`<g fill="${fill}">([\\s\\S]*?)</g>`));
  if (!group) throw new Error(`no ${fill} group in the brand bat`);
  return [...group[1].matchAll(/d="([^"]+)"/g)].map((m) => m[1]);
}

const body = paths("#3A2448"); // the bat (the brand file's own coordinates, before its group transform)
const eyes = paths("#F0A24B"); // its closed eyes, cut out of the head
if (body.length !== 4 || eyes.length !== 2) throw new Error("the brand bat changed shape; look before redrawing");

/** Points tall: Apple suggests 18 in the 22 pt bar; 20 reads clearly (`TRAY_POINTS` overrides). */
const POINTS = Number(process.env.TRAY_POINTS ?? 20);
/** Extra outline, in the brand file's units (about 35 of them to a 1x pixel): a quarter pixel each side at 1x. */
const THICKEN = Number(process.env.TRAY_THICKEN ?? 16);
/** The eyes' cut, widened so it is a clear line at 2x. */
const EYE_CUT = 12;

// Where the brand bat sits, in its own units: measured once (resvg's bounding boxes of the four paths) and checked
// below. The perch is the bar from x 416 to 608 at y 203 to 217; the toes above it reach y 184.
const BAT = { left: 363, right: 677, top: 184, bottom: 776 };
const PERCH = { left: 416, right: 608, mid: 210 };
const pad = THICKEN / 2;

// The scale: the perch fills 1x row 1 exactly, the toes sit in row 0, the bat's foot ends the last row.
//   u = units per 1x pixel; top = perch.mid - 1.5u; top + POINTS*u = bottom  =>  (POINTS-1.5)u = bottom - perch.mid
const bottom = BAT.bottom + pad;
const u = (bottom - PERCH.mid) / (POINTS - 1.5);
const top = PERCH.mid - 1.5 * u;

// Width: the bat's, rounded up to whole 1x pixels, the bat centred.
const batW = BAT.right - BAT.left + THICKEN;
const px1 = Math.ceil(batW / u);
const left = BAT.left - pad - (px1 * u - batW) / 2;

// The perch as one whole row, its ends on whole 1x pixels.
const perchL = left + Math.round((PERCH.left - pad - left) / u) * u;
const perchR = left + Math.round((PERCH.right + pad - left) / u) * u;
const perch = `<rect x="${perchL}" y="${top + u}" width="${perchR - perchL}" height="${u}" fill="#000"/>`;

/** Where to put the alert mark. */
type Mark = { cx: number; cy: number; r: number; gap: number; width: number };
// A dot on the bat's top-right shoulder, under the perch's right end and beside the body: 5 pt across with a clear
// 1 pt ring, overhanging the bat's right edge, so the alert picture is wider by the overhang. Both pictures share
// that wider frame, so the item never changes width and the bat never moves when the state turns.
function mark(): Mark {
  const r = 2.5 * u;
  const gap = 1 * u;
  // Its left edge (with the ring) clears the body, whose right side is at x ≈ 545 down to y ≈ 330.
  const cx = 545 + gap + r;
  const cy = top + 2 * u + gap + r;
  return { cx, cy, r, gap, width: Math.ceil((cx + r - left) / u) };
}
const m = mark();
const frameW = Math.max(px1, m.width);

function svg(scale: number, alert: boolean): string {
  const h = POINTS * scale;
  const w = frameW * scale;
  const eyeCut =
    scale >= 2
      ? eyes.map((d) => `<path d="${d}" fill="#000" stroke="#000" stroke-width="${EYE_CUT}" stroke-linejoin="round"/>`).join("")
      : "";
  const ringCut = alert ? `<circle cx="${m.cx}" cy="${m.cy}" r="${m.r + m.gap}" fill="#000"/>` : "";
  const dot = alert ? `<circle cx="${m.cx}" cy="${m.cy}" r="${m.r}" fill="#000"/>` : "";
  const box = `x="${left}" y="${top}" width="${frameW * u}" height="${POINTS * u}"`;
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${left} ${top} ${frameW * u} ${POINTS * u}" width="${w}" height="${h}">` +
    `<defs><mask id="cut" maskUnits="userSpaceOnUse" ${box}><rect ${box} fill="#fff"/>${eyeCut}${ringCut}</mask></defs>` +
    `<g mask="url(#cut)">` +
    body.map((d) => `<path d="${d}" fill="#000" stroke="#000" stroke-width="${THICKEN}" stroke-linejoin="round"/>`).join("") +
    perch +
    `</g>${dot}</svg>`
  );
}

// Check the measured box against the paths themselves.
const probe = new Resvg(
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1000 1000" width="1000" height="1000">${body.map((d) => `<path d="${d}"/>`).join("")}</svg>`,
).innerBBox();
if (
  !probe ||
  Math.abs(probe.x - BAT.left) > 1 ||
  Math.abs(probe.y - BAT.top) > 1 ||
  Math.abs(probe.x + probe.width - BAT.right) > 1 ||
  Math.abs(probe.y + probe.height - BAT.bottom) > 1
) {
  throw new Error(`the brand bat moved: ${probe?.x} ${probe?.y} ${probe?.width} ${probe?.height}`);
}

for (const [name, alert] of [
  ["bat", false],
  ["bat-alert", true],
] as const) {
  for (const scale of [1, 2]) {
    const png = new Resvg(svg(scale, alert), { fitTo: { mode: "original" } }).render().asPng();
    const file = join(out, `${name}-${scale}x.png`);
    writeFileSync(file, png);
    console.log(`${file}: ${frameW * scale} × ${POINTS * scale}`);
  }
}

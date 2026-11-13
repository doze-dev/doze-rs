// Cuts the app's faces from Geist's variable fonts: one static instance per weight, subset to the characters the app
// draws, renamed so each weight has its own PostScript name and OS/2 weight. See README.md for the run.
//   bun make-fonts.ts SRC.ttf OUT_DIR PREFIX PS_BASE 400,500,...
import subsetFont from "subset-font";
import * as fontkit from "fontkit";
const [src, outDir, prefix, psBase, weightsArg] = process.argv.slice(2);
const weights = weightsArg.split(",").map(Number);
const ranges: [number, number][] = [
  [0x20, 0x7e], [0xa0, 0xff], [0x131, 0x131], [0x152, 0x153], [0x3bc, 0x3bc],
  [0x2010, 0x2027], [0x2030, 0x203a], [0x20ac, 0x20ac], [0x2122, 0x2122],
  [0x2190, 0x21ff], [0x2212, 0x2212], [0x2298, 0x2298], [0x2300, 0x23ff],
  [0x2500, 0x257f], [0x25a0, 0x25ff], [0x2610, 0x2612], [0x2700, 0x2727],
];
let text = "";
for (const [a, b] of ranges) for (let c = a; c <= b; c++) text += String.fromCodePoint(c);
const input = Buffer.from(await Bun.file(src).arrayBuffer());
const names: Record<number, string> = { 400: "Regular", 500: "Medium", 600: "SemiBold", 650: "SemiBold650", 700: "Bold", 750: "Bold750" };

function patch(buf: Buffer, weight: number, family: string): Buffer {
  const style = names[weight];
  const ps = `${psBase}-${style}`;
  const full = `${family} ${style}`;
  const numTables = buf.readUInt16BE(4);
  const tables: { tag: string; data: Buffer }[] = [];
  for (let i = 0; i < numTables; i++) {
    const r = 12 + i * 16;
    const tag = buf.toString("latin1", r, r + 4);
    const off = buf.readUInt32BE(r + 8), len = buf.readUInt32BE(r + 12);
    tables.push({ tag, data: Buffer.from(buf.subarray(off, off + len)) });
  }
  // name: rewrite as a fresh format-0 table, Windows Unicode English and Mac Roman.
  const recs: [number, string][] = [
    [0, `Copyright 2024 The Geist Project Authors (https://github.com/vercel/geist-font)`],
    [1, family], [2, "Regular"], [3, `${ps};instance`], [4, full], [5, "Version 1.9.2; subset and instanced for doze"],
    [6, ps], [16, family], [17, style],
  ];
  const strs: Buffer[] = []; const entries: number[][] = []; let off = 0;
  for (const [id, s] of recs) {
    const mac = Buffer.from(s, "latin1");
    entries.push([1, 0, 0, id, mac.length, off]); strs.push(mac); off += mac.length;
  }
  for (const [id, s] of recs) {
    const u = Buffer.alloc(s.length * 2); for (let i = 0; i < s.length; i++) u.writeUInt16BE(s.charCodeAt(i), i * 2);
    entries.push([3, 1, 0x409, id, u.length, off]); strs.push(u); off += u.length;
  }
  const head = Buffer.alloc(6 + entries.length * 12);
  head.writeUInt16BE(0, 0); head.writeUInt16BE(entries.length, 2); head.writeUInt16BE(head.length, 4);
  entries.forEach((e, i) => e.forEach((v, j) => head.writeUInt16BE(v, 6 + i * 12 + j * 2)));
  const name = tables.find(t => t.tag === "name")!; name.data = Buffer.concat([head, ...strs]);
  const os2 = tables.find(t => t.tag === "OS/2")!; os2.data.writeUInt16BE(weight, 4);
  // fsSelection: REGULAR for 400, BOLD for >=700.
  let fs = os2.data.readUInt16BE(62) & ~(0x40 | 0x20 | 0x01);
  fs |= weight >= 700 ? 0x20 : weight === 400 ? 0x40 : 0; os2.data.writeUInt16BE(fs, 62);
  const headT = tables.find(t => t.tag === "head")!; headT.data.writeUInt32BE(0, 8);
  let mac = headT.data.readUInt16BE(44) & ~1; if (weight >= 700) mac |= 1; headT.data.writeUInt16BE(mac, 44);
  // reassemble
  tables.sort((a, b) => (a.tag < b.tag ? -1 : 1));
  const n = tables.length; let es = 0; while (1 << (es + 1) <= n) es++;
  const dir = Buffer.alloc(12 + n * 16);
  dir.writeUInt32BE(buf.readUInt32BE(0), 0); dir.writeUInt16BE(n, 4);
  dir.writeUInt16BE((1 << es) * 16, 6); dir.writeUInt16BE(es, 8); dir.writeUInt16BE(n * 16 - (1 << es) * 16, 10);
  const sum = (b: Buffer) => { const p = Buffer.concat([b, Buffer.alloc((4 - (b.length % 4)) % 4)]); let s = 0; for (let i = 0; i < p.length; i += 4) s = (s + p.readUInt32BE(i)) >>> 0; return s; };
  let cur = dir.length; const body: Buffer[] = [];
  tables.forEach((t, i) => {
    const r = 12 + i * 16; dir.write(t.tag, r, "latin1"); dir.writeUInt32BE(sum(t.data), r + 4);
    dir.writeUInt32BE(cur, r + 8); dir.writeUInt32BE(t.data.length, r + 12);
    const pad = Buffer.alloc((4 - (t.data.length % 4)) % 4); body.push(t.data, pad); cur += t.data.length + pad.length;
  });
  const out = Buffer.concat([dir, ...body]);
  const hOff = dir.readUInt32BE(12 + tables.findIndex(t => t.tag === "head") * 16 + 8);
  out.writeUInt32BE((0xb1b0afba - sum(out)) >>> 0, hOff + 8);
  return out;
}

const family = (fontkit.create(input) as any).familyName;
for (const w of weights) {
  const sub = await subsetFont(input, text, { targetFormat: "truetype", noLayoutClosure: true, variationAxes: { wght: w } });
  const out = patch(Buffer.from(sub), w, family);
  const path = `${outDir}/${prefix}-${w}.ttf`;
  await Bun.write(path, out);
  const f: any = fontkit.create(out);
  console.log(path.split("/").pop(), out.length, f.postscriptName, f.familyName, f.subfamilyName, f["OS/2"].usWeightClass, "axes", Object.keys(f.variationAxes).length, "glyphs", f.numGlyphs, "feat", f.availableFeatures.join(","));
}

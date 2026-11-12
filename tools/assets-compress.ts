// Compresses the console's text assets, so the binary embeds gzip bytes instead of the plain ones.
//
//   bun tools/assets-compress.ts          writes crates/kafka-console/assets/<name>.gz beside each source
//   bun tools/assets-compress.ts --check  writes nothing; exits 1 when a .gz is missing or stale
//
// Why gzip and not brotli: a browser offers `br` only over https, and the console is http on loopback. The Rust side keeps
// no decoder of its own: a client that does not accept gzip gets the file inflated by flate2, which the broker already links.
//
// The stylesheet is compressed AFTER its `@asset:<file>` placeholders are replaced by the content-hashed names of the fonts
// (the same rule as `assets.rs`: FNV-1a, 48 bits, twelve hex digits, `stem.<hash>.ext`), so the bytes in the binary are the
// bytes served. `assets.rs` tests that every .gz inflates to exactly its source (with the same replacement), so a stale one
// fails `cargo test -p kafka-console` and the message names this command. Fonts are woff2 (already compressed) and the
// favicon is 237 bytes: neither is touched.

import { gzipSync } from "node:zlib";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const DIR = join(import.meta.dir, "..", "crates", "kafka-console", "assets");
const SOURCES = [
  "console.css",
  "console.js",
  "components.js",
  "multiselect.js",
  "live.js",
  "htmx-2.0.11.min.js",
  "idiomorph-ext-0.8.0.min.js",
  "alpine-anchor-3.17.5.min.js",
  "alpine-csp-3.17.5.min.js",
];
const FONTS = [
  "inter-latin-wght-normal.woff2",
  "jetbrains-mono-latin-400-normal.woff2",
  "jetbrains-mono-latin-500-normal.woff2",
];

/** `fingerprint` of assets.rs: FNV-1a over the bytes, the low 48 bits as twelve hex digits. */
export function fingerprint(bytes: Uint8Array): string {
  let h = 0xcbf29ce484222325n;
  for (const b of bytes) {
    h ^= BigInt(b);
    h = (h * 0x100000001b3n) & 0xffffffffffffffffn;
  }
  return (h & 0xffffffffffffn).toString(16).padStart(12, "0");
}

/** `hashed` of assets.rs: `inter.woff2` and `abc` make `inter.abc.woff2`. */
function hashed(name: string, hash: string): string {
  const dot = name.lastIndexOf(".");
  return dot < 0 ? `${name}.${hash}` : `${name.slice(0, dot)}.${hash}${name.slice(dot)}`;
}

/** What is compressed for one source: its bytes, the stylesheet with its font names resolved. */
export function plain(name: string): Uint8Array {
  const bytes = readFileSync(join(DIR, name));
  if (name !== "console.css") return bytes;
  let text = new TextDecoder().decode(bytes);
  for (const font of FONTS) {
    const hash = fingerprint(readFileSync(join(DIR, font)));
    text = text.split(`@asset:${font}`).join(hashed(font, hash));
  }
  return new TextEncoder().encode(text);
}

/** The compression every embedded text file gets (the classic assets here, the React bundle in web/scripts/build.ts): level 9 and
 * the largest memory level, the best zlib offers; the header carries no time, so the file is reproducible. */
export function pack(bytes: Uint8Array): Uint8Array {
  return gzipSync(bytes, { level: 9, memLevel: 9 });
}

function main(): number {
  const check = process.argv.includes("--check");
  let stale = 0;
  for (const name of SOURCES) {
    const want = plain(name);
    const path = join(DIR, `${name}.gz`);
    if (check) {
      const ok = existsSync(path) && Buffer.compare(Buffer.from(Bun.gunzipSync(readFileSync(path))), Buffer.from(want)) === 0;
      if (!ok) {
        console.error(`${name}.gz is missing or stale`);
        stale++;
      }
      continue;
    }
    const packed = pack(want);
    writeFileSync(path, packed);
    console.log(`${name}: ${want.length} -> ${packed.length} B`);
  }
  return stale === 0 ? 0 : 1;
}

if (import.meta.main) process.exit(main());

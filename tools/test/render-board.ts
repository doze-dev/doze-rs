// Test helper: turns a board source into a full HTML document the way the real console page would look.
//
//   - the shell the canvas kit draws around a board (top bar, rail, breadcrumb, the "Reached from" line) is written out
//     as the page's real shell, with the scaffolding line marked `data-scaffold`;
//   - glyph tokens `[[g:name 16]]` become <svg aria-hidden> glyphs;
//   - `href="@id"` becomes a path built from a routes map (screen id -> route pattern; a `{param}` is filled in);
//   - a dialog or drawer becomes an overlay element with role=dialog and an aria-labelledby on its heading;
//   - a `.field` that has one label and one control gets the label bound to the control (for/id), which is what a real
//     page must do and a static drawing cannot.
// Scaffolding in the board ("Other moments" rows) is left in, as the real page would leave out only what the plan says.
//
// It is NOT the Rust renderer. It is the reference the tooling's own tests use: a board rendered by this must diff clean
// against its own snapshot, and every deliberate mutation of it must be caught.

import { type BoardFile } from "../lib/boards.ts";
import { type Routes, SECTION_ENTRY } from "../board-signature.ts";
import { findAll, hasClass, parse } from "../lib/html.ts";

/** One route per board: /<section>/<id>. */
export function syntheticRoutes(boards: BoardFile[]): Routes {
  const r: Routes = {};
  for (const b of boards) r[b.id] = `/${b.section}/${b.id}`;
  return r;
}

/** Routes that use parameters and share a pattern between a page and its states, like the real table will. */
export function paramRoutes(boards: BoardFile[]): Routes {
  const r: Routes = {};
  for (const b of boards) r[b.id] = `/${b.section}/{page}/${b.id.replace(/-/g, "_")}`;
  return r;
}

const glyphSvg = (name: string, size: string | undefined, cls: string | undefined): string =>
  `<svg class="gl${cls ? " " + cls : ""}" width="${size ?? 16}" height="${size ?? 16}" viewBox="0 0 16 16" aria-hidden="true" data-glyph="${name}"><path d="M2 8h12"/></svg>`;

export function fillPattern(pattern: string): string {
  return pattern.replace(/\{\*?[^}]*\}/g, "x1");
}

export function renderBoard(b: BoardFile, routes: Routes, opts: { base?: string } = {}): string {
  const base = (opts.base ?? "").replace(/\/+$/, "");
  const pathOf = (id: string): string => {
    const pat = routes[id];
    if (!pat) return "/missing/" + id;
    return base + fillPattern(Array.isArray(pat) ? pat[0] : pat);
  };

  let body = b.html.replace(/^\s*<!--screen[\s\S]*?-->\s*/, "");
  body = body.replace(/\[\[g:([a-z0-9-]+)(?: (\d+))?(?: ([a-z0-9 -]+))?\]\]/g, (_, n: string, s?: string, c?: string) => glyphSvg(n, s, c));
  body = body.replace(/href="@([a-z0-9/_-]+)"/g, (_, t: string) => {
    const id = !t.includes("/") && SECTION_ENTRY[t] ? SECTION_ENTRY[t] : t.split("/").pop()!;
    return `href="${pathOf(id)}"`;
  });
  body = bindLabels(body);

  const overlay = b.fm.kind === "dialog" || b.fm.kind === "drawer";
  if (overlay) body = labelFirstHeading(body, "dlg-title");

  const title = String(b.fm.title);
  const chev = glyphSvg("chev-r", "12", undefined);
  const crumbs = `<nav class="crumbs" aria-label="Breadcrumb"><a href="${pathOf("home")}">Home</a>${chev}<span class="cur" aria-current="page">${esc(title)}</span></nav>`;
  const from = `<div class="from" data-scaffold>Reached from <a href="${pathOf(String(b.fm.from === "index" ? "home" : b.fm.from).split("/").pop()!)}">${esc(String(b.fm.from))}</a> <span class="via">${esc(String(b.fm.via ?? ""))}</span></div>`;

  const shellTop = `<header class="topbar"><div class="brand"><span class="wm">doze-kafka</span></div>
<div class="search">${glyphSvg("search", "16", undefined)}<input type="text" placeholder="Jump to a topic or group" aria-label="Jump to a topic or group"><kbd>K</kbd></div>
<div class="tbr"><button class="chip" type="button" data-ui="copy" aria-label="Copy address 127.0.0.1:9092"><span class="mono">127.0.0.1:9092</span></button>
<span class="chip">Kafka 4 profile</span><a class="chip" href="${pathOf("access-open")}" aria-label="Access: Open"><i class="rec"></i>Open</a></div></header>`;
  const rail = `<nav class="rail" aria-label="Spaces"><div class="nav">
<a class="ni" href="${pathOf("home")}">${glyphSvg("home", "20", undefined)}Home</a>
<a class="ni" href="${pathOf("live-wire")}">${glyphSvg("live", "20", undefined)}Live</a>
<a class="ni" href="${pathOf("topics-list")}">${glyphSvg("grid", "20", undefined)}Resources</a>
<a class="ni" href="${pathOf("access-open")}">${glyphSvg("access", "20", undefined)}Access</a>
<a class="ni" href="${pathOf("settings-home")}">${glyphSvg("settings", "20", undefined)}Settings</a>
</div></nav>`;

  const main = overlay
    ? `<main class="main">${crumbs}${from}</main>`
    : `<main class="main">${crumbs}${from}${body}</main>`;
  const dialog = overlay
    ? b.fm.kind === "drawer"
      ? `<div class="scrim"></div><aside class="drawer-panel" role="dialog" aria-modal="true" aria-labelledby="dlg-title">${body}</aside>`
      : `<div class="scrim"></div><div class="modal" role="dialog" aria-modal="true" aria-labelledby="dlg-title">${body}</div>`
    : "";

  return `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>${esc(title)} · doze-kafka</title><link rel="stylesheet" href="/assets/console.css"></head>
<body><div class="root">${shellTop}<div class="frame">${rail}${main}</div>${dialog}</div></body></html>
`;
}

const esc = (s: string): string => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

/** Bind each `.field`'s label to its control when the field has exactly one of each. */
function bindLabels(html: string): string {
  const root = parse(html);
  const fields = findAll(root, (e) => hasClass(e, "field"));
  const eligible = new Set<number>();
  fields.forEach((f, i) => {
    const labels = findAll(f, (e) => e.tag === "label");
    const controls = findAll(f, (e) => e.tag === "input" || e.tag === "textarea" || e.tag === "select");
    if (labels.length === 1 && controls.length === 1 && !labels[0].attrs.for && !controls[0].attrs.id && !controls[0].attrs["aria-label"]) eligible.add(i);
  });
  if (!eligible.size) return html;
  let idx = -1;
  return new HTMLRewriter()
    .on(".field", { element() { idx++; } })
    .on(".field label", { element(e) { if (eligible.has(idx)) e.setAttribute("for", `field-${idx}`); } })
    .on(".field input, .field textarea, .field select", { element(e) { if (eligible.has(idx)) e.setAttribute("id", `field-${idx}`); } })
    .transform(html);
}

/** Give the first h2 an id so a dialog can name itself with aria-labelledby. */
function labelFirstHeading(html: string, id: string): string {
  let done = false;
  return new HTMLRewriter()
    .on("h2", { element(e) { if (!done) { done = true; e.setAttribute("id", id); } } })
    .transform(html);
}

// Shared by the tool tests: the boards, a render of each, the mutations, and a one-call comparison.

import { type BoardFile, boardSignature, loadBoards } from "../lib/boards.ts";
import { compare, type Difference } from "../lib/compare.ts";
import { type Mode, type Routes, type Signature, signature } from "../board-signature.ts";
import { renderBoard, syntheticRoutes } from "./render-board.ts";
import { closest, findAll, hasClass, parse, rawText } from "../lib/html.ts";

export const boards: BoardFile[] = loadBoards();
export const byId = new Map(boards.map((b) => [b.id, b]));
export const routes: Routes = syntheticRoutes(boards);
export const render = (b: BoardFile): string => renderBoard(b, routes);

/** The signature of a board, from its source (not from boards.json, so tests of mutated boards work). */
export const boardSig = (b: BoardFile): Signature => boardSignature(b, new Map(boards.map((x) => [x.id, x.fm as Record<string, unknown>])));
export const pageSig = (html: string): Signature => signature(html, { routes });

export function diff(b: BoardFile, html: string, mode: Mode = "strict"): Difference[] {
  return compare(boardSig(b), pageSig(html), mode);
}

// ---------------------------------------------------------------------------------------------------------------
// Mutations of a rendered page. Each returns the new HTML, or null when the page has nothing to mutate.

const SCOPE = (sel: string): string => sel.split(",").map((s) => `main ${s.trim()}, [role=dialog] ${s.trim()}`).join(", ");

/** Run `act` on the n-th element matching `selector` (document order, within main or the dialog). */
function onNth(html: string, selector: string, n: number, act: (e: HTMLRewriterTypes.Element) => void, text?: (t: HTMLRewriterTypes.Text) => void): string {
  let i = -1;
  let cur = false;
  return new HTMLRewriter()
    .on(SCOPE(selector), {
      element(e) {
        i++;
        cur = i === n;
        if (cur) act(e);
      },
      text(t) {
        if (cur && text) text(t);
      },
    })
    .transform(html);
}
function count(html: string, selector: string): number {
  let n = 0;
  new HTMLRewriter().on(SCOPE(selector), { element() { n++; } }).transform(html);
  return n;
}

export interface Mutation {
  name: string;
  /** What the gate must report: every listed (kind, field) must be among the differences. */
  expect: Array<{ kind: Difference["kind"]; field: string }>;
  apply: (html: string, b: BoardFile) => string | null;
}

export const failing: Mutation[] = [
  {
    name: "delete a button",
    expect: [{ kind: "missing", field: "button" }],
    apply: (h) => (count(h, ".btn, button") ? onNth(h, ".btn, button", 0, (e) => e.remove()) : null),
  },
  {
    name: "rename a button",
    expect: [{ kind: "missing", field: "button" }, { kind: "extra", field: "button" }],
    apply: (h) => {
      if (!count(h, ".btn, button")) return null;
      let done = false;
      const out = onNth(h, ".btn, button", 0, () => {}, (t) => {
        if (!done && t.text.trim()) {
          t.replace("Renamed button");
          done = true;
        }
      });
      return done ? out : null;
    },
  },
  {
    name: "change a data-op",
    expect: [{ kind: "missing", field: "op" }, { kind: "extra", field: "op" }],
    apply: (h) => (count(h, "[data-op]") ? onNth(h, "[data-op]", 0, (e) => e.setAttribute("data-op", "RenamedOp")) : null),
  },
  {
    name: "delete a table column",
    expect: [{ kind: "missing", field: "table.col" }],
    apply: (h) => (count(h, "thead th") ? onNth(h, "thead th", 0, (e) => e.remove()) : null),
  },
  {
    name: "drop a card's dt",
    expect: [{ kind: "missing", field: "dt" }],
    apply: (h) => (count(h, "dt") ? onNth(h, "dt", 0, (e) => e.remove()) : null),
  },
  {
    name: "remove a tab",
    expect: [{ kind: "missing", field: "tab" }],
    apply: (h) => (count(h, ".tab") ? onNth(h, ".tab", 0, (e) => e.remove()) : null),
  },
  {
    name: "add a link to another screen",
    expect: [{ kind: "extra", field: "link" }],
    apply: (h, b) => {
      const linked = new Set(boardSig(b).links);
      const other = boards.find((x) => !linked.has(x.id) && x.id !== b.id && x.id !== "home");
      if (!other) return null;
      const target = h.includes('role="dialog"') ? "[role=dialog]" : "main";
      return new HTMLRewriter().on(target, { element(e) { e.append(`<p><a href="${routes[other.id]}">elsewhere</a></p>`, { html: true }); } }).transform(h);
    },
  },
  {
    name: "change a state class (st ok -> st bad)",
    expect: [{ kind: "missing", field: "state" }, { kind: "extra", field: "state" }],
    apply: (h) => {
      // The first "st ok" that is not inside a table cell (a state word in a row is data and counts once).
      let inCell = 0;
      let done = false;
      const out = new HTMLRewriter()
        .on("td", { element(e) { inCell++; e.onEndTag(() => { inCell--; }); } })
        .on(SCOPE(".st"), {
          element(e) {
            const cls = e.getAttribute("class") ?? "";
            if (!done && !inCell && /\bok\b/.test(cls)) {
              e.setAttribute("class", cls.replace(/\bok\b/, "bad"));
              done = true;
            }
          },
        })
        .transform(h);
      return done ? out : null;
    },
  },
  {
    name: "remove the h1",
    expect: [{ kind: "missing", field: "h1" }],
    apply: (h) => (count(h, "h1") ? onNth(h, "h1", 0, (e) => e.remove()) : null),
  },
  {
    name: "change the h1 text",
    expect: [{ kind: "missing", field: "h1" }, { kind: "extra", field: "h1" }],
    apply: (h) => {
      if (!count(h, "h1")) return null;
      let done = false;
      const out = onNth(h, "h1", 0, () => {}, (t) => {
        if (!done && t.text.trim()) {
          t.replace("A different title");
          done = true;
        }
      });
      return done ? out : null;
    },
  },
];

export const passing: Mutation[] = [
  {
    name: "change a cell's text",
    expect: [],
    apply: (h) => {
      // The first data cell: a td with text and no heading in it (an empty-state row holds a heading, which is a label).
      const tds = findAll(parse(h), (e) => e.tag === "td" && !closest(e, (a) => hasClass(a, "topbar")));
      const n = tds.findIndex((td) => findAll(td, (e) => /^h[1-6]$/.test(e.tag)).length === 0 && rawText(td).trim() !== "");
      if (n < 0) return null;
      let done = false;
      return onNth(h, "td", n, () => {}, (t) => {
        if (!done && t.text.trim()) {
          t.replace("999,999 changed");
          done = true;
        }
      });
    },
  },
  {
    name: "add a scaffold row",
    expect: [],
    apply: (h) => {
      const target = h.includes('role="dialog"') ? "[role=dialog]" : "main";
      const row = `<div class="row wrap" data-scaffold><span>Other moments of this page</span><a class="lnk" href="/live/live-wire">Somewhere</a><button class="btn pri" type="button" data-op="Produce">Scaffold</button><span class="st bad">scaffold</span></div>`;
      return new HTMLRewriter().on(target, { element(e) { e.append(row, { html: true }); } }).transform(h);
    },
  },
];

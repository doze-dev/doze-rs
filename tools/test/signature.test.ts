import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { designDir } from "../lib/boards.ts";
import { buildSnapshot } from "../boards-snapshot.ts";
import { type Routes, resolveRoute, signature, stableStringify } from "../board-signature.ts";
import { boards } from "./helpers.ts";

describe("the board contract", () => {
  test("there are 96 boards in 10 sections", () => {
    expect(boards.length).toBe(96);
    expect(new Set(boards.map((b) => b.section)).size).toBe(10);
  });

  test("every board has a non-empty, deterministic signature", () => {
    for (const b of boards) {
      const a = signature(b.html);
      const again = signature(b.html);
      expect(stableStringify(a)).toBe(stableStringify(again));
      const parts = a.h1.length + a.headings.length + a.buttons.length + a.links.length + a.cards.length + a.tables.length;
      expect(parts, `${b.id} has an empty signature`).toBeGreaterThan(0);
      expect(a.landmarks).toContain("main");
      expect(a.kind).toBe(b.fm.kind === "dialog" || b.fm.kind === "drawer" ? "overlay" : "page");
    }
  });

  test("boards.json is what the boards give, byte for byte, with a trailing newline", () => {
    const { snapshot, problems } = buildSnapshot();
    expect(problems).toEqual([]);
    const text = readFileSync(`${designDir}/boards.json`, "utf8");
    expect(text).toBe(stableStringify(snapshot) + "\n");
    expect(text.endsWith("}\n")).toBe(true);
    expect(Object.keys(snapshot.boards).length).toBe(96);
  });

  test("every link on a board is a board, and every op is listed in the front matter", () => {
    const ids = new Set(boards.map((b) => b.id));
    for (const b of boards) {
      const s = signature(b.html);
      for (const l of s.links) expect(ids.has(l), `${b.id} links to ${l}`).toBe(true);
      const listed = new Set((b.fm.ops as string[] | undefined) ?? []);
      for (const op of Object.keys(s.ops)) expect(listed.has(op), `${b.id} uses ${op}`).toBe(true);
    }
  });
});

describe("extraction rules", () => {
  const page = (body: string) => `<!--screen {"id":"x","title":"X","kind":"page","from":"index"} -->${body}`;

  test("glyph tokens and svg are dropped from labels", () => {
    const s = signature(page(`<div class="page-h"><h1>X</h1></div><a class="btn" href="@x">[[g:send 16]]Send <svg><title>nope</title></svg></a>`));
    expect(s.buttons[0].label).toBe("Send");
  });

  test("data-vary text is ignored, structure is not", () => {
    const a = signature(page(`<h1>X</h1><h2>Records <span data-vary>48,213</span></h2>`));
    const b = signature(page(`<h1>X</h1><h2>Records <span data-vary>7</span></h2>`));
    expect(a.headings).toEqual(["h2 Records"]);
    expect(stableStringify(a)).toBe(stableStringify(b));
  });

  test("scaffolding, .from and the crumbs are excluded", () => {
    const s = signature(page(`<h1>X</h1><div data-scaffold><a href="@x">a</a><button class="btn" data-op="Produce">b</button></div><div class="from"><a href="@x">c</a></div>`));
    expect(s.links).toEqual([]);
    expect(s.buttons).toEqual([]);
    expect(s.ops).toEqual({});
  });

  test("a rendered page is its <main>; an open dialog makes it an overlay page, an empty closed <dialog> does not", () => {
    const shell = (inner: string) => `<html lang="en"><body><header class="topbar"><button class="btn">Top</button></header><nav class="rail" aria-label="Spaces"></nav><main class="main"><nav class="crumbs" aria-label="Breadcrumb"><a href="/">Home</a><span class="cur">Orders</span></nav><h1>Orders</h1><button class="btn">Go</button>${inner}</main></body></html>`;
    const plain = signature(shell(`<dialog id="d"></dialog>`));
    expect(plain.kind).toBe("page");
    expect(plain.title).toBe("Orders");
    expect(plain.buttons.map((b) => b.label)).toEqual(["Go"]); // the top bar is not part of the page
    expect(plain.landmarks).toEqual(["banner", "breadcrumb", "main", "rail"]);
    const open = signature(shell(`<dialog open aria-labelledby="t"><h2 id="t">Delete?</h2><button class="btn danger">Delete</button></dialog>`));
    expect(open.kind).toBe("overlay");
    expect(open.title).toBeNull();
    expect(open.h1).toEqual([]);
    expect(open.buttons.map((b) => b.label)).toEqual(["Delete"]);
  });

  test("counts in tab labels are dropped", () => {
    const s = signature(page(`<h1>X</h1><div class="tabs"><a class="tab on" href="@x">Messages</a><a class="tab" href="@x">Consumers<span class="n">12</span></a><a class="tab" href="@x">Producers 3</a></div>`));
    expect(s.tabs.map((t) => t.label)).toEqual(["Messages", "Consumers", "Producers"]);
    expect(s.tabs.map((t) => t.on)).toEqual([true, false, false]);
  });

  test("a button's action is the op, else the ui kind, else the target screen", () => {
    const s = signature(page(`<h1>X</h1><a class="btn pri" href="@y" data-op="Produce">Send</a><button class="btn sm" data-ui="copy">Copy</button><a class="btn danger fill" href="@live">Go</a>`));
    expect(s.buttons.map((b) => b.action)).toEqual(["op:Produce", "ui:copy", "to:live-wire"]);
    expect(s.buttons.map((b) => b.variants)).toEqual([["pri"], ["sm"], ["danger", "fill"]]);
  });

  test("cell text never enters the signature; state words in rows count once", () => {
    const s = signature(page(`<h1>X</h1><table class="t"><thead><tr><th>A</th><th>B</th></tr></thead><tbody><tr><td>secret-1</td><td><span class="st ok">Ok</span></td></tr><tr><td>secret-2</td><td><span class="st ok">Ok</span></td></tr></tbody></table>`));
    expect(JSON.stringify(s)).not.toContain("secret");
    expect(s.tables).toEqual([{ cols: ["A", "B"], rows: true, wire: false }]);
    expect(s.states).toEqual({ "st ok in rows": 1 });
  });

  test("a card is keyed by its heading; callout and banner give variant and first bold phrase", () => {
    const s = signature(page(`<h1>X</h1><div class="card"><h2>Instance <span class="mono">orders</span></h2><dl class="kv"><dt>Started</dt><dd>now</dd></dl><div class="callout warn"><b>Careful.</b> text <b>other</b></div></div><div class="banner bad"><b>Down</b></div><div class="field"><label>Name <span class="opt">optional</span></label><input></div>`));
    const card = s.cards.find((c) => c.key === "Instance {}")!;
    expect(card.dt).toEqual(["Started"]);
    expect(card.notes).toEqual([{ kind: "callout", variant: "warn", lead: "Careful." }]);
    const page_ = s.cards.find((c) => c.key === "page")!;
    expect(page_.notes).toEqual([{ kind: "banner", variant: "bad", lead: "Down" }]);
    expect(page_.fields).toEqual(["Name"]);
  });
});

describe("route resolution", () => {
  const routes: Routes = {
    "topics-list": "/topics",
    "topics-none": "/topics",
    "topic-detail": "/topics/{topic}",
    "topic-create": "/topics/new",
    "topic-record": "/topics/{topic}/partitions/{p}/offsets/{offset}",
    "home": "/",
  };
  test("a {param} matches any one segment; the most literal pattern wins", () => {
    expect(resolveRoute("/topics/orders", routes)).toEqual(["topic-detail"]);
    expect(resolveRoute("/topics/new", routes)).toEqual(["topic-create"]);
    expect(resolveRoute("/topics/orders/partitions/2/offsets/16050", routes)).toEqual(["topic-record"]);
    expect(resolveRoute("/topics/orders/partitions/2", routes)).toEqual([]);
    expect(resolveRoute("/", routes)).toEqual(["home"]);
  });
  test("states that share a route resolve to all of them", () => {
    expect(resolveRoute("/topics/", routes)).toEqual(["topics-list", "topics-none"]);
  });
  test("a rendered link is resolved, a base prefix is stripped, an unknown path is unresolved, external and in-page links are ignored", () => {
    const html = `<html lang="en"><body><main class="main"><h1>X</h1><a href="/console/topics/orders?x=1">a</a><a href="/console/nowhere">b</a><a href="https://example.com/">c</a><a href="#top">d</a><a href="/console/topics">e</a></main></body></html>`;
    const s = signature(html, { routes, base: "/console" });
    expect(s.links).toEqual(["?/nowhere", "topic-detail", "topics-list|topics-none"]);
  });
  test("with no routes every internal link is unresolved", () => {
    const s = signature(`<main><a href="/topics">t</a></main>`, {});
    expect(s.links).toEqual(["?/topics"]);
  });
});

// The palette's sections and recents (console.js, "The jump palette"): headings follow their rows, the last five places opened are
// kept as a kind and a name and painted as copies of the list's own rows, and a stored hostile name is never markup.

import { describe, expect, test } from "bun:test";
import { boot, type Page } from "./harness.ts";

const OVL = { "x-overlay": "1" };
const PAGE = `<div class="root"><header class="topbar"><div class="search" data-palette="/jump"><input type="text" disabled aria-label="Jump"></div></header><main id="main"></main></div>`;
const row = (kind: string, name: string, href: string) => `<li><a class="jump-item" href="${href}" data-jump="${name.toLowerCase()}"><span class="mono">${name}</span><span class="jump-kind">${kind}</span></a></li>`;
const palette = (rows: string) =>
  `<div class="ovl ovl-dialog root" data-overlay data-transient data-close="/jump"><a class="scrim" href="/jump" tabindex="-1" data-overlay-close></a>` +
  `<div class="modal" role="dialog" aria-modal="true" aria-labelledby="jump-title" tabindex="-1"><div class="jump"><h2 id="jump-title" class="sr">Jump</h2>` +
  `<input id="jump-q" type="text" data-jump-input><ul class="jump-list">${rows}</ul><p class="jump-empty" hidden>No match</p></div></div></div>`;
const LIST = `<li class="jump-h" role="presentation">Topics</li>${row("topic", "orders", "/topics/orders")}${row("topic", "payments", "/topics/payments")}` +
  `<li class="jump-h" role="presentation">Groups</li>${row("group", "fulfilment", "/groups/fulfilment")}` +
  `<li class="jump-h" role="presentation">Pages</li>${row("page", "Live", "/live")}`;

const open = async (p: Page) => {
  p.autoAnswer(() => ({ headers: OVL, body: palette(LIST) }));
  p.document.body.dispatchEvent(new p.window.KeyboardEvent("keydown", { key: "k", metaKey: true, bubbles: true, cancelable: true }));
  await p.flush();
};
const heads = (p: Page) => [...p.document.querySelectorAll(".jump-list .jump-h")].filter((h) => !(h as HTMLElement).hidden).map((h) => h.textContent);
const type = (p: Page, v: string) => {
  const f = p.document.getElementById("jump-q") as HTMLInputElement;
  f.value = v;
  f.dispatchEvent(new p.window.Event("input", { bubbles: true }));
};

describe("the palette's sections and recents", () => {
  test("a heading is shown only while a row of its section is", async () => {
    const p = await boot(PAGE);
    await open(p);
    expect(heads(p)).toEqual(["Topics", "Groups", "Pages"]);
    type(p, "ful");
    expect(heads(p)).toEqual(["Groups"]);
    type(p, "");
    expect(heads(p)).toEqual(["Topics", "Groups", "Pages"]);
  });

  test("a place opened is kept as a kind and a name, and is the first recent the next time", async () => {
    const p = await boot(PAGE);
    await open(p);
    (p.document.querySelector('a[href="/groups/fulfilment"]') as HTMLElement).dispatchEvent(new p.window.MouseEvent("click", { bubbles: true, cancelable: true }));
    expect(p.window.localStorage.getItem("doze:recent")).toBe('[{"k":"group","n":"fulfilment"}]');
    const q = await boot(PAGE);
    q.window.localStorage.setItem("doze:recent", '[{"k":"group","n":"fulfilment"},{"k":"topic","n":"orders"}]');
    await open(q);
    const first = [...q.document.querySelectorAll(".jump-list li")].map((li) => li.textContent);
    expect(first[0]).toBe("Recent");
    expect(first.slice(1, 3)).toEqual(["fulfilmentgroup", "orderstopic"]);
    // Typing hides them (the originals are in the list) and their heading.
    type(q, "pay");
    expect(heads(q)).toEqual(["Topics"]);
    expect([...q.document.querySelectorAll(".jump-list li:not([hidden])")].map((li) => li.textContent)).toEqual(["Topics", "paymentstopic"]);
  });

  test("at most five are kept, newest first, once each", async () => {
    const p = await boot(PAGE);
    await open(p);
    p.window.localStorage.setItem("doze:recent", JSON.stringify(["a", "b", "c", "d", "e"].map((n) => ({ k: "topic", n }))));
    (p.document.querySelector('a[href="/topics/orders"]') as HTMLElement).dispatchEvent(new p.window.MouseEvent("click", { bubbles: true, cancelable: true }));
    const kept = JSON.parse(p.window.localStorage.getItem("doze:recent")!);
    expect(kept.map((r: { n: string }) => r.n)).toEqual(["orders", "a", "b", "c", "d"]);
  });

  test("a stored name that is markup is matched against the list and never becomes markup", async () => {
    const p = await boot(PAGE);
    p.window.localStorage.setItem("doze:recent", JSON.stringify([{ k: "topic", n: "<img src=x onerror=alert(1)>" }, { k: 1, n: 2 }, "junk"]));
    await open(p);
    expect(p.document.querySelectorAll(".jump-list img").length).toBe(0);
    expect(p.document.querySelector(".jump-list [data-recent]")).toBeNull();
  });
});

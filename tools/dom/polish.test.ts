// The small things of console.js that make it a pleasure: the jump palette (⌘K), j and k through the rows (and through the
// records of an open drawer), and a copy that says it copied.

import { describe, expect, test } from "bun:test";
import { boot, type Page } from "./harness.ts";

const ORIGIN = "http://127.0.0.1:9093";
const OVL = { "x-overlay": "1" };

const PAGE = `<div class="root"><header class="topbar"><div class="search" data-palette="/jump"><input type="text" disabled aria-label="Jump"></div></header>
<main id="main"><table class="t"><tbody>
<tr><td><a id="r1" href="/topics/orders/messages/0/3" data-dialog>3</a></td></tr>
<tr><td><a id="r2" href="/topics/orders/messages/0/2" data-dialog>2</a></td></tr>
<tr><td><a id="r3" href="/topics/orders/messages/0/1" data-dialog>1</a></td></tr>
</tbody></table><input id="field" name="q"><button id="copy" data-copy="abc">Copy</button></main></div>`;

const palette = (names: string[]) =>
  `<div class="ovl ovl-dialog root" data-overlay data-transient data-close="/jump"><a class="scrim" href="/jump" tabindex="-1" data-overlay-close></a>` +
  `<div class="modal" role="dialog" aria-modal="true" aria-labelledby="jump-title" tabindex="-1"><div class="jump"><h2 id="jump-title" class="sr">Jump</h2>` +
  `<input id="jump-q" type="text" data-jump-input><ul class="jump-list">${names.map((n) => `<li><a href="/topics/${n}" data-jump="${n.toLowerCase()}">${n}</a></li>`).join("")}</ul><p class="jump-empty" hidden>No topic</p></div></div></div>`;

const record = (n: number) =>
  `<div class="ovl ovl-drawer root" data-overlay data-close="/topics/orders"><a class="scrim" href="/topics/orders" tabindex="-1" data-overlay-close></a><div class="drawer-panel" role="dialog" aria-modal="true" aria-labelledby="t" tabindex="-1"><h2 id="t">record ${n}</h2></div></div>`;

const key = (p: Page, k: string, extra: Record<string, unknown> = {}, on?: Element) => {
  const e = new p.window.KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...extra });
  (on ?? p.document.activeElement ?? p.document.body).dispatchEvent(e);
  return e;
};
const el = (p: Page, sel: string) => p.document.querySelector(sel) as HTMLElement;

describe("the jump palette", () => {
  const opened = async (names = ["orders", "orders.dlq", "inventory.levels"]) => {
    const p = await boot(PAGE);
    p.autoAnswer(() => ({ headers: OVL, body: palette(names) }));
    key(p, "k", { metaKey: true }, p.document.body);
    await p.flush();
    return p;
  };
  const visible = (p: Page) => [...p.document.querySelectorAll(".jump-list li")].filter((li) => !(li as HTMLElement).hidden).map((li) => li.textContent);

  test("the top bar's search is offered by a script (and a click on it opens the palette)", async () => {
    const p = await boot(PAGE);
    const input = el(p, ".search input") as HTMLInputElement;
    expect(input.disabled).toBe(false);
    p.autoAnswer(() => ({ headers: OVL, body: palette(["orders"]) }));
    p.click(input);
    await p.flush();
    expect(p.calls[0].url).toBe("/jump");
    expect(el(p, "[data-overlay]")).not.toBeNull();
  });

  test("⌘K and Ctrl+K open it over the page with no history entry, focus in its field", async () => {
    const p = await opened();
    expect(p.calls[0].headers["X-Fragment"]).toBe("1");
    expect(p.history.pushes).toEqual([]);
    expect(p.document.activeElement?.id).toBe("jump-q");
    expect(el(p, ".root:not([data-overlay])").hasAttribute("inert")).toBe(true);
    const q = await boot(PAGE);
    q.autoAnswer(() => ({ headers: OVL, body: palette(["orders"]) }));
    key(q, "k", { ctrlKey: true }, q.document.body);
    await q.flush();
    expect(el(q, "[data-overlay]")).not.toBeNull();
  });

  test("typing narrows the topics, ignoring case, and says so when none is left", async () => {
    const p = await opened();
    const f = p.document.getElementById("jump-q") as HTMLInputElement;
    f.value = "ORD";
    f.dispatchEvent(new p.window.Event("input", { bubbles: true }));
    expect(visible(p)).toEqual(["orders", "orders.dlq"]);
    expect(el(p, ".jump-empty").hidden).toBe(true);
    f.value = "zzz";
    f.dispatchEvent(new p.window.Event("input", { bubbles: true }));
    expect(visible(p)).toEqual([]);
    expect(el(p, ".jump-empty").hidden).toBe(false);
  });

  test("the arrows move the selection and Enter goes to the selected topic", async () => {
    const p = await opened();
    const f = p.document.getElementById("jump-q") as HTMLInputElement;
    f.value = "o";
    f.dispatchEvent(new p.window.Event("input", { bubbles: true }));
    const sel = () => [...p.document.querySelectorAll(".jump-list a.sel")].map((a) => a.textContent);
    expect(sel()).toEqual(["orders"]);
    key(p, "ArrowDown");
    expect(sel()).toEqual(["orders.dlq"]);
    key(p, "ArrowDown");
    key(p, "ArrowDown");
    expect(sel()).toEqual(["inventory.levels"]);
    key(p, "ArrowUp");
    expect(sel()).toEqual(["orders.dlq"]);
    let followed = "";
    p.document.addEventListener("click", (e) => { e.preventDefault(); followed = (e.target as HTMLAnchorElement).getAttribute("href")!; });
    key(p, "Enter");
    expect(followed).toBe("/topics/orders.dlq");
  });

  test("Esc closes it, and so does ⌘K again, with the address untouched", async () => {
    const p = await opened();
    key(p, "Escape");
    p.runTimers();
    expect(p.document.querySelector("[data-overlay]")).toBeNull();
    expect(p.history.replaces).toEqual([]);
    expect(p.history.url).toBe(`${ORIGIN}/`);
    p.autoAnswer(() => ({ headers: OVL, body: palette(["orders"]) }));
    key(p, "k", { metaKey: true }, p.document.body);
    await p.flush();
    key(p, "k", { metaKey: true });
    p.runTimers();
    expect(p.document.querySelector("[data-overlay]")).toBeNull();
  });
});

describe("j and k", () => {
  test("move focus through the rows of the page's table, and stop at the ends", async () => {
    const p = await boot(PAGE);
    p.document.body.focus();
    key(p, "j", {}, p.document.body);
    expect(p.document.activeElement?.id).toBe("r1");
    key(p, "j");
    key(p, "j");
    expect(p.document.activeElement?.id).toBe("r3");
    key(p, "j");
    expect(p.document.activeElement?.id).toBe("r3");
    key(p, "k");
    expect(p.document.activeElement?.id).toBe("r2");
  });

  test("do nothing in a field, with a modifier, or with no table", async () => {
    const p = await boot(PAGE);
    const field = p.document.getElementById("field")!;
    field.focus();
    expect(key(p, "j").defaultPrevented).toBe(false);
    expect(p.document.activeElement?.id).toBe("field");
    expect(key(p, "j", { metaKey: true }, p.document.body).defaultPrevented).toBe(false);
    const q = await boot(`<div class="root"><main id="main"><p>none</p></main></div>`);
    expect(key(q, "j", {}, q.document.body).defaultPrevented).toBe(false);
  });

  test("with a drawer open they are the next and the previous record, replacing it in place", async () => {
    const p = await boot(PAGE);
    p.autoAnswer((c) => ({ headers: OVL, body: record(Number(c.url.split("/").pop())) }));
    p.click(p.document.getElementById("r2")!);
    await p.flush();
    expect(el(p, "[data-overlay] h2").textContent).toBe("record 2");
    key(p, "j");
    await p.flush();
    expect(el(p, "[data-overlay] h2").textContent).toBe("record 1");
    expect(p.document.querySelectorAll("[data-overlay]").length).toBe(1);
    key(p, "k");
    key(p, "k");
    await p.flush();
    expect(el(p, "[data-overlay] h2").textContent).toBe("record 3");
    // Past the end nothing is fetched.
    const n = p.calls.length;
    key(p, "k");
    await p.flush();
    expect(p.calls.length).toBe(n);
    // Closing returns focus to the row it ended on.
    key(p, "Escape");
    p.runTimers();
    expect(p.document.activeElement?.id).toBe("r1");
  });
});

describe("a copy", () => {
  test("says Copied for a moment, in the width the button had, then goes back to its label", async () => {
    const p = await boot(PAGE);
    const b = p.document.getElementById("copy")!;
    p.click(b);
    expect(b.textContent).toBe("Copied");
    expect(b.hasAttribute("data-copied")).toBe(true);
    expect(p.copied).toEqual(["abc"]);
    // Pressed again meanwhile: copied again, the label not lost.
    p.click(b);
    expect(p.copied).toEqual(["abc", "abc"]);
    p.runTimers();
    expect(b.textContent).toBe("Copy");
    expect(b.hasAttribute("data-copied")).toBe(false);
  });
});

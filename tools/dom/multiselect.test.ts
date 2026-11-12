// The MultiSelect of crates/kafka-console/assets/multiselect.js on the markup src/ui/components/multiselect.rs renders, under Alpine's CSP
// build in a DOM (happy-dom): toggling, search, Select all / Clear, the chips, Backspace, the keyboard, and the form's value as repeated
// parameters. Positioning (x-anchor) needs layout and is asserted in real Chrome (tools/ux/scenarios/multiselect.ts).

import { describe, expect, test } from "bun:test";
import { boot, type Page } from "./harness.ts";

const TOPICS: Array<[string, string, number]> = [["orders", "orders", 12], ["orders.dlq", "orders.dlq", 3], ["payments.settled", "payments.settled", 7], ["pick-lists", "pick-lists", 2]];
const HOSTILE = `<img src=x onerror=alert(1)> "q" & 'q'`;
const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

// The markup the maud function writes (src/ui/components/multiselect.rs), with the Alpine attributes spelled as Alpine reads them.
const multiHtml = (values: string[] = [], options = TOPICS, autosubmit = true) =>
  `<form id="form"><div class="dz dz-multi dz-chip${values.length ? " on" : ""}" id="topic" x-data="dzMulti" x-on:keydown="key" x-on:click="click" x-on:mousemove="hover" x-on:mousedown="down" x-on:focusout="focusout" x-on:click.outside="away" :data-open="open" data-ui="multiselect" data-name="topic" data-noun="topics" data-placeholder="All topics"${autosubmit ? " data-autosubmit" : ""}>` +
  `<details class="dz-native dz-multi-plain"><summary>All topics</summary><fieldset class="dz-multi-checks">` +
  options.map(([v, l, n], i) => `<label class="dz-multi-check"><input type="checkbox" name="topic" value="${esc(v)}"${values.includes(v) ? " checked" : ""} id="topic-c${i}"><span>${esc(l)}</span><span class="dz-count">${n}</span></label>`).join("") +
  `<button type="submit" class="btn sm" data-noscript>Apply</button></fieldset></details>` +
  `<div class="dz-multi-box"><button type="button" class="dz-trigger dz-multi-trigger" x-ref="trigger" aria-haspopup="listbox" aria-expanded="false" :aria-expanded="open" aria-controls="topic-pop" aria-label="Topics: All topics" hidden><span class="dz-value muted">All topics</span></button>` +
  `<button type="button" class="dz-multi-clear" x-ref="clear" x-show="has" data-clear aria-label="Clear topics" style="display:none">x</button></div>` +
  `<div class="dz-pop dz-multi-pop" id="topic-pop" tabindex="-1" role="group" aria-label="Topics" x-ref="pop" x-show="open" x-anchor.bottom-start.offset.4="$refs.trigger" style="display:none">` +
  `<div class="dz-multi-field"><span class="dz-multi-chips" x-ref="chips"></span><input class="dz-input" x-ref="search" type="text" role="combobox" aria-autocomplete="list" aria-expanded="true" aria-controls="topic-list" aria-label="Search topics" x-on:input="filter"></div>` +
  `<div class="dz-multi-actions"><span class="dz-multi-n" role="status" x-ref="status">0 of ${options.length} selected</span><button type="button" class="dz-link" data-all>Select all</button><button type="button" class="dz-link" data-none>Clear</button></div>` +
  `<div class="dz-multi-list" role="listbox" aria-multiselectable="true" id="topic-list" aria-label="Topics" x-ref="list">` +
  options.map(([v, l, n], i) => `<div class="dz-opt dz-multi-opt" role="option" id="topic-o${i}" data-i="${i}" data-value="${esc(v)}" data-label="${esc(l)}" aria-selected="${values.includes(v)}"><span class="dz-box"></span><span class="dz-opt-t"><span class="dz-opt-label">${esc(l)}</span></span><span class="dz-count">${n}</span></div>`).join("") +
  `<div class="dz-empty" hidden>No match</div></div></div></div></form>`;

const settle = async () => {
  for (let i = 0; i < 3; i++) await new Promise((r) => setImmediate(r));
  await new Promise((r) => setTimeout(r, 12));
  for (let i = 0; i < 3; i++) await new Promise((r) => setImmediate(r));
};
const q = (p: Page, sel: string) => p.document.querySelector(sel) as HTMLElement;
const qa = (p: Page, sel: string) => [...p.document.querySelectorAll(sel)] as HTMLElement[];
const key = (p: Page, k: string, extra: Record<string, unknown> = {}) => {
  const target = (p.document.activeElement ?? p.document.body) as HTMLElement;
  const e = new p.window.KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...extra });
  target.dispatchEvent(e);
  return e;
};
const typeInto = async (p: Page, text: string) => {
  const s = q(p, ".dz-input") as HTMLInputElement;
  s.value = text;
  s.dispatchEvent(new p.window.Event("input", { bubbles: true }));
  await settle();
};
const started = async (values: string[] = [], options = TOPICS) => {
  const p = await boot(multiHtml(values, options), { alpine: true, plain: true });
  await settle();
  const changes = { n: 0, submits: 0 };
  q(p, "#form").addEventListener("change", () => void changes.n++);
  q(p, "#form").addEventListener("submit", (e) => { e.preventDefault(); changes.submits++; });
  return Object.assign(p, { changes });
};
const trigger = (p: Page) => q(p, ".dz-trigger");
const open = async (p: Page) => { trigger(p).focus(); key(p, "Enter"); await settle(); };
const isOpen = (p: Page) => q(p, ".dz").hasAttribute("data-open");
const value = (p: Page) => q(p, ".dz-value").textContent;
const data = (p: Page) => new p.window.FormData(q(p, "#form") as any) as unknown as FormData;
const chosen = (p: Page) => new Set(data(p).getAll("topic") as string[]);
const params = (p: Page) => [...(data(p) as any).entries()].map(([k, v]: [string, string]) => `${k}=${v}`).join("&");
const active = (p: Page) => p.document.querySelector("[data-active]")?.getAttribute("data-label");
const click = (p: Page, el: Element) => el.dispatchEvent(new p.window.MouseEvent("click", { bubbles: true, cancelable: true }));
const optionFor = (p: Page, label: string) => qa(p, "[role=option]").find((o) => o.getAttribute("data-label") === label)!;

describe("MultiSelect", () => {
  test("Alpine hides the checkboxes, shows the button, and the button says 'All topics' (muted) with nothing chosen", async () => {
    const p = await started();
    expect(q(p, ".dz").classList.contains("is-on")).toBe(true);
    expect(trigger(p).hidden).toBe(false);
    expect(value(p)).toBe("All topics");
    expect(q(p, ".dz-value").classList.contains("muted")).toBe(true);
    expect(q(p, ".dz-native").getAttribute("aria-hidden")).toBe("true");
    expect(q(p, "[data-clear]").style.display).toBe("none");
    expect(q(p, ".dz-multi-pop").style.display).toBe("none");
  });

  test("the button says one name, two names, then a count; the title lists them all; the clear x appears", async () => {
    const one = await started(["orders"]);
    expect(value(one)).toBe("orders");
    expect(q(one, "[data-clear]").style.display).not.toBe("none");
    expect(q(one, ".dz").classList.contains("on")).toBe(true);
    const two = await started(["orders", "payments.settled"]);
    expect(value(two)).toBe("orders, payments.settled");
    const three = await started(["orders", "orders.dlq", "pick-lists"]);
    expect(value(three)).toBe("3 topics");
    expect(trigger(three).title).toBe("orders, orders.dlq, pick-lists");
    expect(trigger(three).getAttribute("aria-label")).toBe("Topics: 3 topics");
  });

  test("Enter, Space, ArrowDown and ArrowUp open it; focus goes to the search field and the first option is active", async () => {
    for (const k of ["Enter", " ", "ArrowDown", "ArrowUp"]) {
      const p = await started();
      trigger(p).focus();
      const e = key(p, k);
      await settle();
      expect(e.defaultPrevented).toBe(true);
      expect(isOpen(p)).toBe(true);
      expect(q(p, ".dz-multi-pop").style.display).not.toBe("none");
      expect(p.document.activeElement).toBe(q(p, ".dz-input"));
      expect(active(p)).toBe("orders");
      expect(q(p, ".dz-input").getAttribute("aria-activedescendant")).toBe("topic-o0");
    }
  });

  test("a letter typed on the closed button opens it with that letter in the search, filtering the list", async () => {
    const p = await started();
    trigger(p).focus();
    key(p, "p");
    await settle();
    expect((q(p, ".dz-input") as HTMLInputElement).value).toBe("p");
    expect(qa(p, "[role=option]").filter((o) => !o.hidden).map((o) => o.getAttribute("data-label"))).toEqual(["payments.settled", "pick-lists"]);
  });

  test("a click on an option toggles it: the checkbox, aria-selected, the button, the status; the form holds repeated params", async () => {
    const p = await started();
    await open(p);
    click(p, optionFor(p, "orders"));
    click(p, optionFor(p, "payments.settled"));
    expect(params(p)).toBe("topic=orders&topic=payments.settled");
    expect(optionFor(p, "orders").getAttribute("aria-selected")).toBe("true");
    expect(optionFor(p, "orders.dlq").getAttribute("aria-selected")).toBe("false");
    expect(value(p)).toBe("orders, payments.settled");
    expect(q(p, "[role=status]").textContent).toBe("2 of 4 selected");
    click(p, optionFor(p, "orders"));
    expect(params(p)).toBe("topic=payments.settled");
    expect(isOpen(p)).toBe(true);
    expect(p.document.activeElement).toBe(q(p, ".dz-input"));
  });

  test("arrows, Home and End move the active option; Space and Enter toggle it; Enter does not submit the form", async () => {
    const p = await started();
    await open(p);
    key(p, "ArrowDown");
    expect(active(p)).toBe("orders.dlq");
    key(p, "End");
    expect(active(p)).toBe("pick-lists");
    key(p, "ArrowDown");
    expect(active(p)).toBe("pick-lists");
    key(p, "Home");
    expect(active(p)).toBe("orders");
    const space = key(p, " ");
    expect(space.defaultPrevented).toBe(true);
    expect(chosen(p)).toEqual(new Set(["orders"]));
    key(p, "ArrowDown");
    const enter = key(p, "Enter");
    expect(enter.defaultPrevented).toBe(true);
    expect(chosen(p)).toEqual(new Set(["orders", "orders.dlq"]));
    key(p, " ");
    expect(chosen(p)).toEqual(new Set(["orders"]));
    expect(p.changes.submits).toBe(0);
  });

  test("Space types a space once the search has text (and toggles nothing)", async () => {
    const p = await started();
    await open(p);
    await typeInto(p, "pay");
    const e = key(p, " ");
    expect(e.defaultPrevented).toBe(false);
    expect(chosen(p).size).toBe(0);
  });

  test("the search filters the options (case-insensitive, by label), says 'No match', and the active option follows", async () => {
    const p = await started();
    await open(p);
    await typeInto(p, "ORDERS");
    expect(qa(p, "[role=option]").filter((o) => !o.hidden).map((o) => o.getAttribute("data-label"))).toEqual(["orders", "orders.dlq"]);
    expect(active(p)).toBe("orders");
    expect(q(p, ".dz-empty").hidden).toBe(true);
    await typeInto(p, "zzz");
    expect(q(p, ".dz-empty").hidden).toBe(false);
    expect(active(p)).toBeUndefined();
    await typeInto(p, "");
    expect(qa(p, "[role=option]").every((o) => !o.hidden)).toBe(true);
  });

  test("Select all takes the options that pass the search; Clear takes every one off; each asks the server once it settles", async () => {
    const p = await started(["pick-lists"]);
    await open(p);
    await typeInto(p, "orders");
    click(p, q(p, "[data-all]"));
    expect(chosen(p)).toEqual(new Set(["orders", "orders.dlq", "pick-lists"]));
    expect((q(p, "[data-all]") as HTMLButtonElement).disabled).toBe(true);
    click(p, q(p, "[data-none]"));
    expect(chosen(p).size).toBe(0);
    expect(value(p)).toBe("All topics");
    expect((q(p, "[data-none]") as HTMLButtonElement).disabled).toBe(true);
    await typeInto(p, "");
    click(p, q(p, "[data-all]"));
    expect(chosen(p).size).toBe(4);
    expect(value(p)).toBe("4 topics");
    key(p, "Escape");
    await settle();
    expect(p.changes.n).toBe(1);
  });

  test("the chosen show as removable chips in the popup; the x on a chip takes it off", async () => {
    const p = await started(["orders", "pick-lists"]);
    await open(p);
    expect(qa(p, ".dz-tag").map((c) => c.querySelector(".dz-tag-t")!.textContent)).toEqual(["orders", "pick-lists"]);
    const x = q(p, ".dz-tag-x[data-remove='3']");
    expect(x.getAttribute("aria-label")).toBe("Remove pick-lists");
    click(p, x);
    expect(chosen(p)).toEqual(new Set(["orders"]));
    expect(qa(p, ".dz-tag").length).toBe(1);
  });

  test("Backspace in the empty search removes the last chip; with text in the search it is only a Backspace", async () => {
    const p = await started(["orders", "orders.dlq", "pick-lists"]);
    await open(p);
    const e = key(p, "Backspace");
    expect(e.defaultPrevented).toBe(true);
    expect(chosen(p)).toEqual(new Set(["orders", "orders.dlq"]));
    key(p, "Backspace");
    key(p, "Backspace");
    expect(chosen(p).size).toBe(0);
    key(p, "Backspace");
    expect(chosen(p).size).toBe(0);
    await typeInto(p, "x");
    await typeInto(p, "");
    click(p, optionFor(p, "orders"));
    await typeInto(p, "o");
    expect(key(p, "Backspace").defaultPrevented).toBe(false);
    expect(chosen(p)).toEqual(new Set(["orders"]));
  });

  test("Escape closes it, returns focus to the button, and an overlay behind does not see the key", async () => {
    const p = await started();
    await open(p);
    let seen = false;
    p.document.body.addEventListener("keydown", () => void (seen = true));
    const e = key(p, "Escape");
    await settle();
    expect(e.defaultPrevented).toBe(true);
    expect(seen).toBe(false);
    expect(isOpen(p)).toBe(false);
    expect(p.document.activeElement).toBe(trigger(p));
  });

  // (a click outside closes it: Alpine's `.outside` ignores an element with no layout, which is all happy-dom has; the real thing is
  // asserted in tools/ux/scenarios/multiselect.ts)

  test("ticking three options asks the server once, a moment after the last (and again only when something else changes)", async () => {
    const p = await started();
    await open(p);
    click(p, optionFor(p, "orders"));
    click(p, optionFor(p, "orders.dlq"));
    click(p, optionFor(p, "pick-lists"));
    expect(p.changes.n).toBe(0);
    await new Promise((r) => setTimeout(r, 520));
    expect(p.changes.n).toBe(1);
    expect(p.changes.submits).toBe(1);
    expect(isOpen(p)).toBe(true);
    key(p, "Escape");
    await settle();
    expect(p.changes.n).toBe(1);
  });

  test("closing with a change not yet sent sends it at once; closing with none sends nothing", async () => {
    const p = await started();
    await open(p);
    key(p, "Escape");
    await settle();
    expect(p.changes.n).toBe(0);
    await open(p);
    click(p, optionFor(p, "orders"));
    key(p, "Escape");
    await settle();
    expect(p.changes.n).toBe(1);
  });

  test("the clear x on the button clears every choice, asks the server and keeps focus on the button", async () => {
    const p = await started(["orders", "pick-lists"]);
    click(p, q(p, "[data-clear]"));
    await settle();
    expect(chosen(p).size).toBe(0);
    expect(value(p)).toBe("All topics");
    expect(p.changes.n).toBe(1);
    expect(p.document.activeElement).toBe(trigger(p));
    await settle();
    expect(q(p, "[data-clear]").style.display).toBe("none");
  });

  test("the chosen options are listed first when it opens again (and never move while it is open)", async () => {
    const p = await started(["pick-lists"]);
    await open(p);
    expect(qa(p, "[role=option]").map((o) => o.getAttribute("data-label"))).toEqual(["pick-lists", "orders", "orders.dlq", "payments.settled"]);
    expect(active(p)).toBe("pick-lists");
    click(p, optionFor(p, "orders"));
    expect(qa(p, "[role=option]").map((o) => o.getAttribute("data-label"))).toEqual(["pick-lists", "orders", "orders.dlq", "payments.settled"]);
  });

  test("with a search and a selection, Enter toggles the option the search left active", async () => {
    const p = await started();
    await open(p);
    await typeInto(p, "settled");
    key(p, "Enter");
    expect(chosen(p)).toEqual(new Set(["payments.settled"]));
  });

  test("a hostile label is text in the options, the chips and the button, and the checkbox value is the raw string", async () => {
    const p = await boot(multiHtml([HOSTILE], [[HOSTILE, HOSTILE, 1], ["b", "b", 2]]), { alpine: true, plain: true });
    await settle();
    await open(p);
    expect(p.document.querySelectorAll("img").length).toBe(0);
    expect(q(p, ".dz-tag-t").textContent).toBe(HOSTILE);
    expect(value(p)).toBe(HOSTILE);
    expect(q(p, ".dz-tag-x").getAttribute("aria-label")).toBe("Remove " + HOSTILE);
    expect(chosen(p)).toEqual(new Set([HOSTILE]));
  });

  test("without data-autosubmit a change is left for the form's own button", async () => {
    const p = await boot(multiHtml([], TOPICS, false), { alpine: true, plain: true });
    await settle();
    let submitted = 0;
    q(p, "#form").addEventListener("submit", (e) => { e.preventDefault(); submitted++; });
    await open(p);
    click(p, optionFor(p, "orders"));
    key(p, "Escape");
    await settle();
    expect(submitted).toBe(0);
    expect(chosen(p)).toEqual(new Set(["orders"]));
  });
});

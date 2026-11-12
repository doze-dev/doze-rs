// The components of crates/kafka-console/assets/components.js on the markup src/ui/components/ renders, under Alpine's CSP build in
// a DOM (happy-dom): the keyboard, the ARIA state, the value in the native control, the events. Positioning (x-anchor) needs layout and
// is asserted in a real browser (tools/ux/scenarios/components.ts). Run: `cd tools/dom && bun install && bun test`.

import { describe, expect, test } from "bun:test";
import { boot, type Page } from "./harness.ts";

const EV = `x-on:keydown="key" x-on:click="click" x-on:mousemove="hover" x-on:focusout="focusout" x-on:click.outside="away" :data-open="open"`;
const opt = (id: string, n: number, value: string, label: string, sel: boolean) =>
  `<div class="dz-opt" role="option" id="${id}-o${n}" data-value="${value}" data-label="${label}" aria-selected="${sel}"><span class="dz-opt-t"><span class="dz-opt-label">${label}</span></span></div>`;
const COLOURS: Array<[string, string]> = [["r", "Red"], ["g", "Green"], ["b", "Blue"]];

const selectHtml = (value = "g", extra = "") =>
  `<form id="form"><div class="dz dz-select" x-data="dzSelect" ${EV} data-ui="select">` +
  `<select class="dz-native" id="colour" name="colour" aria-label="Colour" ${extra}>${COLOURS.map(([v, l]) => `<option value="${v}"${v === value ? " selected" : ""}>${l}</option>`).join("")}</select>` +
  `<button type="button" class="dz-trigger" x-ref="trigger" role="combobox" aria-haspopup="listbox" aria-expanded="false" :aria-expanded="open" aria-controls="colour-list" data-placeholder="Choose" hidden><span class="dz-value">x</span></button>` +
  `<div class="dz-pop" role="listbox" id="colour-list" aria-label="Colour" x-ref="list" x-show="open" x-anchor.bottom-start.offset.4="$refs.trigger" style="display:none">${COLOURS.map(([v, l], n) => opt("colour", n, v, l, v === value)).join("")}</div>` +
  `</div></form>`;

const comboHtml = (value = "", create = false) =>
  `<div class="dz dz-combobox" x-data="dzCombobox" ${EV}${create ? " data-create" : ""} data-ui="combobox">` +
  (create
    ? `<input class="dz-native" type="text" id="c" name="client" value="${value}" autocomplete="off" aria-label="Client">`
    : `<select class="dz-native" id="c" name="client" aria-label="Client"><option value="" ${value === "" ? "selected" : ""}></option>${COLOURS.map(([v, l]) => `<option value="${v}"${v === value ? " selected" : ""}>${l}</option>`).join("")}</select>`) +
  `<div class="dz-field" x-ref="field" hidden><input class="dz-input" x-ref="input" type="text" role="combobox" aria-autocomplete="list" aria-haspopup="listbox" aria-expanded="false" :aria-expanded="open" aria-controls="c-list" value="" x-on:input="input"><button type="button" class="dz-chev" tabindex="-1" aria-hidden="true"></button></div>` +
  `<div class="dz-pop" role="listbox" id="c-list" aria-label="Client" x-ref="list" x-show="open" x-anchor.bottom-start.offset.4="$refs.field" style="display:none">` +
  COLOURS.map(([v, l], n) => opt("c", n, v, l, v === value)).join("") +
  `<div class="dz-opt" role="option" id="c-create" data-create-row data-value="" hidden><span class="dz-opt-label">Use <span class="dz-create-text"></span></span></div><div class="dz-empty" hidden>No match</div></div></div>`;

const menuHtml = () =>
  `<div class="dz dz-menu" x-data="dzMenu" x-on:keydown="key" x-on:click="click" x-on:click.outside="away" x-on:focusout="focusout" :data-open="open">` +
  `<button type="button" class="dz-menu-btn" x-ref="trigger" aria-haspopup="menu" aria-expanded="false" :aria-expanded="open" aria-controls="m-menu" aria-label="Actions" hidden>...</button>` +
  `<div class="dz-pop" role="menu" id="m-menu" aria-label="Actions" x-ref="list" x-show="open" x-anchor.bottom-end.offset.4="$refs.trigger">` +
  `<a class="dz-item" role="menuitem" id="m-i0" href="/a">Open</a><div class="dz-sep" role="separator"></div><a class="dz-item danger" role="menuitem" id="m-i2" href="/b">Delete</a></div></div>`;

const settle = async (p: Page) => {
  for (let i = 0; i < 4; i++) await new Promise((r) => setImmediate(r));
  p.runTimers();
  for (let i = 0; i < 4; i++) await new Promise((r) => setImmediate(r));
};
const q = (p: Page, sel: string) => p.document.querySelector(sel) as HTMLElement;
const key = (p: Page, k: string, extra: Record<string, unknown> = {}) => {
  const target = (p.document.activeElement ?? p.document.body) as HTMLElement;
  const e = new p.window.KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...extra });
  target.dispatchEvent(e);
  return e;
};
const started = async (html: string) => {
  const p = await boot(html, { alpine: true, plain: true });
  await settle(p);
  return p;
};
const trigger = (p: Page) => q(p, ".dz-trigger");
const expanded = (p: Page, sel = ".dz-trigger") => q(p, sel).getAttribute("aria-expanded");
const active = (p: Page) => p.document.querySelector("[data-active]")?.textContent;

describe("Select", () => {
  test("Alpine turns the plain select into a combobox button and a listbox, and the button shows the chosen label", async () => {
    const p = await started(selectHtml("g"));
    expect(q(p, ".dz").classList.contains("is-on")).toBe(true);
    expect(trigger(p).hidden).toBe(false);
    expect(q(p, ".dz-value").textContent).toBe("Green");
    expect(q(p, "#colour").getAttribute("aria-hidden")).toBe("true");
    expect((q(p, "#colour") as HTMLSelectElement).tabIndex).toBe(-1);
    expect(trigger(p).getAttribute("aria-controls")).toBe("colour-list");
    expect(trigger(p).getAttribute("aria-label")).toBe("Colour");
  });

  test("ArrowDown, Enter and Space open it with the chosen option active; the state is in the ARIA attributes", async () => {
    for (const k of ["ArrowDown", "Enter", " ", "ArrowUp"]) {
      const p = await started(selectHtml("g"));
      trigger(p).focus();
      key(p, k);
      await settle(p);
      expect(expanded(p)).toBe("true");
      expect(q(p, "[role=listbox]").style.display).not.toBe("none");
      expect(active(p)).toBe("Green");
      expect(trigger(p).getAttribute("aria-activedescendant")).toBe("colour-o1");
      expect(q(p, ".dz").hasAttribute("data-open")).toBe(true);
    }
  });

  test("arrows, Home and End move; Enter chooses: the native value, the label, aria-selected and a change event follow", async () => {
    const p = await started(selectHtml("g"));
    let changes = 0;
    q(p, "#colour").addEventListener("change", () => void changes++);
    trigger(p).focus();
    key(p, "ArrowDown");
    await settle(p);
    key(p, "ArrowDown");
    expect(active(p)).toBe("Blue");
    key(p, "ArrowDown");
    expect(active(p)).toBe("Blue");
    key(p, "Home");
    expect(active(p)).toBe("Red");
    key(p, "End");
    expect(active(p)).toBe("Blue");
    key(p, "Enter");
    await settle(p);
    expect((q(p, "#colour") as HTMLSelectElement).value).toBe("b");
    expect(q(p, ".dz-value").textContent).toBe("Blue");
    expect(expanded(p)).toBe("false");
    expect(changes).toBe(1);
    expect(q(p, "#colour-o2").getAttribute("aria-selected")).toBe("true");
    expect(q(p, "#colour-o1").getAttribute("aria-selected")).toBe("false");
    expect(p.document.activeElement).toBe(trigger(p));
  });

  test("choosing what is already chosen fires no change", async () => {
    const p = await started(selectHtml("g"));
    let changes = 0;
    q(p, "#colour").addEventListener("change", () => void changes++);
    trigger(p).focus();
    key(p, "Enter");
    await settle(p);
    key(p, "Enter");
    await settle(p);
    expect(changes).toBe(0);
  });

  test("type-ahead goes to the option that starts with what was typed; the same letter again goes to the next", async () => {
    const p = await started(selectHtml("r"));
    trigger(p).focus();
    key(p, "ArrowDown");
    await settle(p);
    key(p, "b");
    expect(active(p)).toBe("Blue");
    key(p, "g");
    expect(active(p)).toBe("Green");
  });

  test("Esc closes it, focus stays on the button, and the key is not seen by anything outside (an overlay behind would not close)", async () => {
    const p = await started(selectHtml("g"));
    let outside = 0;
    p.document.addEventListener("keydown", (e) => { if (e.key === "Escape" && !e.defaultPrevented) outside++; });
    trigger(p).focus();
    key(p, "ArrowDown");
    await settle(p);
    const e = key(p, "Escape");
    await settle(p);
    expect(e.defaultPrevented).toBe(true);
    expect(expanded(p)).toBe("false");
    expect(p.document.activeElement).toBe(trigger(p));
    expect(outside).toBe(0);
  });

  test("a click on an option chooses it; a click outside closes without choosing", async () => {
    const p = await started(selectHtml("g"));
    p.click(trigger(p));
    await settle(p);
    expect(expanded(p)).toBe("true");
    p.click(q(p, "#colour-o0"));
    await settle(p);
    expect((q(p, "#colour") as HTMLSelectElement).value).toBe("r");
    p.click(trigger(p));
    await settle(p);
    p.document.body.dispatchEvent(new p.window.MouseEvent("click", { bubbles: true }));
    await settle(p);
    expect(expanded(p)).toBe("false");
    expect((q(p, "#colour") as HTMLSelectElement).value).toBe("r");
  });

  test("a disabled select cannot be opened", async () => {
    const p = await started(selectHtml("g", "disabled"));
    expect(trigger(p).disabled).toBe(true);
  });

  test("the label of the select goes to the button, and a click on it focuses the button", async () => {
    const p = await started(`<label for="colour" id="lab">Colour</label>` + selectHtml("g"));
    expect(trigger(p).getAttribute("aria-labelledby")).toBe("lab");
    p.click(q(p, "#lab"));
    expect(p.document.activeElement).toBe(trigger(p));
  });

  test("an option's text is text: a hostile label is never markup", async () => {
    const hostile = `<img src=x onerror=alert(1)>`;
    const p = await started(selectHtml("g").replace(">Green<", `>${hostile.replace(/</g, "&lt;").replace(/>/g, "&gt;")}<`));
    expect(p.document.querySelectorAll("img").length).toBe(0);
  });
});

describe("Combobox", () => {
  test("typing filters the options, says so when none is left, and Enter chooses the active one", async () => {
    const p = await started(comboHtml(""));
    const input = q(p, ".dz-input") as HTMLInputElement;
    input.focus();
    input.value = "re";
    input.dispatchEvent(new p.window.Event("input", { bubbles: true }));
    await settle(p);
    const shown = [...p.document.querySelectorAll("[role=option]")].filter((o) => !(o as HTMLElement).hidden && !o.hasAttribute("data-create-row")).map((o) => o.textContent);
    expect(shown).toEqual(["Red"]);
    expect(active(p)).toBe("Red");
    input.value = "zzz";
    input.dispatchEvent(new p.window.Event("input", { bubbles: true }));
    await settle(p);
    expect((q(p, ".dz-empty") as HTMLElement).hidden).toBe(false);
    input.value = "gr";
    input.dispatchEvent(new p.window.Event("input", { bubbles: true }));
    await settle(p);
    expect((q(p, ".dz-empty") as HTMLElement).hidden).toBe(true);
    key(p, "Enter");
    await settle(p);
    expect((q(p, "#c") as HTMLSelectElement).value).toBe("g");
    expect(input.value).toBe("Green");
    expect(expanded(p, ".dz-input")).toBe("false");
  });

  test("Esc closes the list and puts the chosen label back in the field", async () => {
    const p = await started(comboHtml("g"));
    const input = q(p, ".dz-input") as HTMLInputElement;
    input.focus();
    key(p, "ArrowDown");
    await settle(p);
    input.value = "xx";
    input.dispatchEvent(new p.window.Event("input", { bubbles: true }));
    await settle(p);
    const e = key(p, "Escape");
    await settle(p);
    expect(e.defaultPrevented).toBe(true);
    expect(input.value).toBe("Green");
    expect((q(p, "#c") as HTMLSelectElement).value).toBe("g");
  });

  test("with create, a name that is not in the list offers 'Use ...' and Enter keeps it as the value", async () => {
    const p = await started(comboHtml("", true));
    const input = q(p, ".dz-input") as HTMLInputElement;
    input.focus();
    input.value = "my-app";
    input.dispatchEvent(new p.window.Event("input", { bubbles: true }));
    await settle(p);
    const row = q(p, "[data-create-row]");
    expect(row.hidden).toBe(false);
    expect(row.textContent).toContain("my-app");
    let changes = 0;
    q(p, "#c").addEventListener("change", () => void changes++);
    key(p, "ArrowDown"); // the first option is Red? no: nothing matches 'my-app', the create row is the only one
    key(p, "Enter");
    await settle(p);
    expect((q(p, "#c") as HTMLInputElement).value).toBe("my-app");
    expect(changes).toBe(1);
  });

  test("with create, a name that IS in the list does not offer to create it", async () => {
    const p = await started(comboHtml("", true));
    const input = q(p, ".dz-input") as HTMLInputElement;
    input.focus();
    input.value = "red";
    input.dispatchEvent(new p.window.Event("input", { bubbles: true }));
    await settle(p);
    expect(q(p, "[data-create-row]").hidden).toBe(true);
  });
});

describe("Menu", () => {
  test("opening moves focus into the menu; arrows wrap and skip the separator; Esc returns focus to the button", async () => {
    const p = await started(menuHtml());
    const btn = q(p, ".dz-menu-btn");
    expect(btn.hidden).toBe(false);
    btn.focus();
    key(p, "ArrowDown");
    await settle(p);
    expect(expanded(p, ".dz-menu-btn")).toBe("true");
    expect(p.document.activeElement?.textContent).toBe("Open");
    key(p, "ArrowDown");
    expect(p.document.activeElement?.textContent).toBe("Delete");
    key(p, "ArrowDown");
    expect(p.document.activeElement?.textContent).toBe("Open");
    key(p, "ArrowUp");
    expect(p.document.activeElement?.textContent).toBe("Delete");
    const e = key(p, "Escape");
    await settle(p);
    expect(e.defaultPrevented).toBe(true);
    expect(expanded(p, ".dz-menu-btn")).toBe("false");
    expect(p.document.activeElement).toBe(btn);
  });

  test("ArrowUp on the button opens it on the last item; a click on an item closes it", async () => {
    const p = await started(menuHtml());
    q(p, ".dz-menu-btn").focus();
    key(p, "ArrowUp");
    await settle(p);
    expect(p.document.activeElement?.textContent).toBe("Delete");
    p.click(q(p, "#m-i0"));
    await settle(p);
    expect(expanded(p, ".dz-menu-btn")).toBe("false");
  });
});

describe("Tooltip", () => {
  const tipHtml = `<span class="dz dz-tip" x-data="dzTip" x-on:mouseenter="later" x-on:mouseleave="off" x-on:focusin="now" x-on:focusout="off" x-on:keydown="key">` +
    `<span class="dz-tip-t" x-ref="trigger" title="Copy it" aria-describedby="t1"><button id="b">Copy</button></span>` +
    `<span class="dz-pop dz-tip-pop" role="tooltip" id="t1" x-show="shown" style="display:none" x-anchor.top.offset.6="$refs.trigger">Copy it</span></span>`;

  test("it waits 400 ms of hover, shows at once on focus, and Esc, leaving and blur hide it", async () => {
    const p = await started(tipHtml);
    const tip = q(p, "#t1");
    expect(q(p, ".dz-tip-t").hasAttribute("title")).toBe(false);
    q(p, ".dz-tip").dispatchEvent(new p.window.MouseEvent("mouseenter"));
    await settle(p);
    expect(p.timers.some((t) => t.ms === 400 && !t.cleared)).toBe(true);
    await settle(p); // runs the 400 ms timer
    expect(tip.style.display).not.toBe("none");
    q(p, ".dz-tip").dispatchEvent(new p.window.MouseEvent("mouseleave"));
    await settle(p);
    expect(tip.style.display).toBe("none");
    q(p, "#b").focus();
    q(p, ".dz-tip").dispatchEvent(new p.window.FocusEvent("focusin", { bubbles: true }));
    await settle(p);
    expect(tip.style.display).not.toBe("none");
    key(p, "Escape");
    await settle(p);
    expect(tip.style.display).toBe("none");
  });
});

describe("Segmented control", () => {
  const segHtml = `<div class="seg dz-seg" role="group" aria-label="Partitions" x-data="dzSeg" x-on:keydown="key" x-on:focusin="focus">` +
    `<a class="dz-seg-i on" href="?p=all" data-seg aria-current="true">All</a><a class="dz-seg-i" href="?p=0" data-seg>0</a><a class="dz-seg-i" href="?p=1" data-seg>1</a></div>`;

  test("one Tab stop that travels with focus; arrows, Home and End move it and wrap", async () => {
    const p = await started(segHtml);
    const items = [...p.document.querySelectorAll("[data-seg]")] as HTMLElement[];
    expect(items.map((i) => i.getAttribute("tabindex"))).toEqual(["0", "-1", "-1"]);
    items[0].focus();
    key(p, "ArrowRight");
    expect(p.document.activeElement).toBe(items[1]);
    expect(items.map((i) => i.getAttribute("tabindex"))).toEqual(["-1", "0", "-1"]);
    key(p, "End");
    expect(p.document.activeElement).toBe(items[2]);
    key(p, "ArrowRight");
    expect(p.document.activeElement).toBe(items[0]);
    key(p, "ArrowLeft");
    expect(p.document.activeElement).toBe(items[2]);
    key(p, "Home");
    expect(p.document.activeElement).toBe(items[0]);
  });
});

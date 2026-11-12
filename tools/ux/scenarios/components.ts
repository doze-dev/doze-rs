// The components in the gallery (/gallery, examples build): keyboard, focus, ARIA state, in-place updates.
import { join } from "node:path";
import type { T } from "../drive.ts";
import type { Page } from "../page.ts";

const trig = (id: string) => `#${id}`;
const open = (id: string) => `.dz:has(#${id}) .dz-trigger`;
const list = (id: string) => `.dz:has(#${id}) [role=listbox]`;
const visible = (p: Page, sel: string) => p.eval<boolean>(`(() => { const e = document.querySelector(${JSON.stringify(sel)}); if (!e) return false; const s = getComputedStyle(e); const r = e.getBoundingClientRect(); return s.display !== 'none' && s.visibility !== 'hidden' && r.width > 0 && r.height > 0; })()`);

export const scenarios = {
  async "gallery-screenshots"(t: T) {
    const p = t.page;
    await p.goto("/gallery");
    await p.shot(join(t.shots, "gallery.png"));
    t.ok(await visible(p, open("s-plain")), "the enhanced select trigger is shown");
    t.ok(await p.eval("getComputedStyle(document.querySelector('#s-plain')).opacity === '0'"), "the native select is not seen");
    t.ok(await p.eval("document.querySelector('.dz:has(#s-plain) .dz-value').textContent === 'Green'"), "the trigger shows the chosen label");
    // Hostile labels are text, never markup.
    t.ok(await p.eval("document.querySelectorAll('.gal img').length === 0"), "a hostile label created no element");
  },

  async "select-keyboard"(t: T) {
    const p = t.page;
    await p.goto("/gallery");
    await p.focus(open("s-plain"));
    t.ok((await p.eval<string>(`document.querySelector(${JSON.stringify(open("s-plain"))}).getAttribute('aria-expanded')`)) === "false", "closed at first");
    await p.key("ArrowDown");
    await p.waitFor(`document.querySelector(${JSON.stringify(open("s-plain"))}).getAttribute('aria-expanded') === 'true'`);
    await p.waitFor(`!!document.querySelector(${JSON.stringify(open("s-plain"))}).getAttribute('aria-activedescendant')`);
    t.ok(await visible(p, list("s-plain")), "ArrowDown opens the list");
    t.ok(await p.eval(`document.querySelector(${JSON.stringify(list("s-plain"))}).getAttribute('role') === 'listbox'`), "role=listbox");
    t.ok(await p.eval("document.activeElement.getAttribute('role') === 'combobox'"), "focus stays on the combobox");
    const act0 = await p.eval<string>(`document.querySelector(${JSON.stringify(open("s-plain"))}).getAttribute('aria-activedescendant')`);
    t.ok(act0.endsWith("-o1"), `the chosen option (Green) is active first: ${act0}`);
    await p.key("ArrowUp");
    await p.key("Home");
    t.ok((await p.eval<string>(`document.querySelector(${JSON.stringify(open("s-plain"))}).getAttribute('aria-activedescendant')`)).endsWith("-o0"), "Home goes to the first");
    await p.key("End");
    t.ok((await p.eval<string>(`document.querySelector(${JSON.stringify(open("s-plain"))}).getAttribute('aria-activedescendant')`)).endsWith("-o3"), "End goes to the last");
    await p.key("Home");
    await p.key("g");
    t.ok((await p.eval<string>(`document.querySelector(${JSON.stringify(open("s-plain"))}).getAttribute('aria-activedescendant')`)).endsWith("-o1"), "type-ahead: g goes to Green");
    await p.key("Escape");
    await p.waitFor(`document.querySelector(${JSON.stringify(open("s-plain"))}).getAttribute('aria-expanded') === 'false'`);
    t.ok(await p.eval("document.activeElement.getAttribute('role') === 'combobox'"), "Esc closes and focus stays on the trigger");
    t.ok(await p.eval("!document.querySelector('.ovl')"), "Esc did not close anything else");
    await p.key("Enter");
    await p.waitFor(`document.querySelector(${JSON.stringify(open("s-plain"))}).getAttribute('aria-expanded') === 'true'`);
    await p.key("Home");
    await p.key("Enter");
    await p.waitFor(`document.querySelector(${JSON.stringify(open("s-plain"))}).getAttribute('aria-expanded') === 'false'`);
    t.ok(await p.eval("document.querySelector('#s-plain').value === 'r'"), "Enter chooses: the native select holds r");
    t.ok(await p.eval("document.querySelector('.dz:has(#s-plain) .dz-value').textContent === 'Red'"), "the trigger shows Red");
    t.ok(await p.eval("document.querySelector('.dz:has(#s-plain) [aria-selected=true]').textContent.includes('Red')"), "aria-selected follows");
    // A disabled option is skipped by choosing it.
    await p.key("ArrowDown");
    await p.key("End");
    await p.key("Enter");
    t.ok(await p.eval("document.querySelector('#s-plain').value === 'r'"), "a disabled option cannot be chosen");
    await p.key("Escape");
    // Opening near the bottom flips above.
    await p.eval("window.scrollTo(0, document.body.scrollHeight)");
  },

  async "select-mouse-and-outside"(t: T) {
    const p = t.page;
    await p.goto("/gallery");
    await p.click(open("s-plain"));
    await p.waitFor(`document.querySelector(${JSON.stringify(open("s-plain"))}).getAttribute('aria-expanded') === 'true'`);
    await p.shot(join(t.shots, "select-open.png"));
    const r = await p.rect(list("s-plain"));
    const tr = await p.rect(open("s-plain"));
    const gap = r.top >= tr.bottom - 1 ? r.top - tr.bottom : tr.top - r.bottom;
    t.num("pop.gap", gap);
    t.ok(gap >= 0 && gap < 12, "the list sits against the trigger (under it, or above when there is no room)");
    t.ok(r.width >= tr.width - 1, "the list is at least as wide as the trigger");
    await p.click(`${list("s-plain")} [role=option]:nth-child(1)`);
    await p.waitFor(`document.querySelector(${JSON.stringify(open("s-plain"))}).getAttribute('aria-expanded') === 'false'`);
    t.ok(await p.eval("document.querySelector('#s-plain').value === 'r'"), "a click on an option chooses it");
    // click outside closes
    await p.click(open("s-plain"));
    await p.waitFor(`document.querySelector(${JSON.stringify(open("s-plain"))}).getAttribute('aria-expanded') === 'true'`);
    await p.click("h1");
    await p.waitFor(`document.querySelector(${JSON.stringify(open("s-plain"))}).getAttribute('aria-expanded') === 'false'`);
    t.ok(true, "a click outside closes it");
  },

  async "select-flips-near-the-bottom"(t: T) {
    const p = t.page;
    await p.goto("/gallery");
    // Put the trigger low in the window (once the page has its full height: the scroll is clamped before that).
    await p.waitFor("document.documentElement.scrollHeight > window.innerHeight + 300");
    await Bun.sleep(150);
    await p.eval(`(() => { const e = document.querySelector(${JSON.stringify(open("s-plain"))}); const y = e.getBoundingClientRect().top + window.scrollY - (window.innerHeight - 70); window.scrollTo({ top: y, behavior: "instant" }); })()`);
    await Bun.sleep(150);
    await p.click(open("s-plain"), { scroll: false });
    await p.waitFor(`document.querySelector(${JSON.stringify(open("s-plain"))}).getAttribute('aria-expanded') === 'true'`);
    await Bun.sleep(100);
    const r = await p.rect(list("s-plain"));
    const tr = await p.rect(open("s-plain"));
    t.num("list.bottom", r.bottom); t.num("trigger.top", tr.top);
    t.ok(r.bottom <= tr.top + 1 || r.bottom <= (await p.eval<number>("window.innerHeight")) + 1, "the list does not run past the window: it flips or fits");
    await p.shot(join(t.shots, "select-flip.png"));
    await p.key("Escape");
  },

  async "combobox-filters-and-creates"(t: T) {
    const p = t.page;
    await p.goto("/gallery");
    const input = ".dz:has(#c-client) .dz-input";
    await p.focus(input);
    await p.type("ad");
    await p.waitFor(`document.querySelector(${JSON.stringify(list("c-client"))}) && getComputedStyle(document.querySelector(${JSON.stringify(list("c-client"))})).display !== 'none'`);
    const shown = await p.eval<number>(`[...document.querySelectorAll(${JSON.stringify(list("c-client") + " [role=option]")})].filter((o) => !o.hidden && !o.hasAttribute('data-create-row')).length`);
    t.num("filtered", shown);
    t.ok(shown === 1, "typing 'ad' leaves one option (admin-cli)");
    await p.shot(join(t.shots, "combobox-open.png"));
    await p.key("Enter");
    await p.waitFor("document.querySelector('#c-client').value === 'admin-cli'");
    t.ok(await p.eval(`document.querySelector(${JSON.stringify(input)}).value === 'admin-cli'`), "the input shows the chosen name");
    // empty state
    await p.eval(`(() => { const i = document.querySelector(${JSON.stringify(input)}); i.select(); })()`);
    await p.type("zzz");
    await Bun.sleep(80);
    t.ok(await visible(p, `.dz:has(#c-client) .dz-empty`), "no match says so");
    await p.key("Escape");
    t.ok(await p.eval(`document.querySelector(${JSON.stringify(input)}).value === 'admin-cli'`), "Esc puts the chosen name back");
    // create
    const free = ".dz:has(#c-free) .dz-input";
    await p.focus(free);
    await p.type("my-new-app");
    await Bun.sleep(80);
    t.ok(await p.eval(`(() => { const r = document.querySelector('.dz:has(#c-free) [data-create-row]'); return !!r && !r.hidden && r.textContent.includes('my-new-app'); })()`), "a name that is not in the list offers 'Use my-new-app'");
    await p.key("Enter");
    await Bun.sleep(80);
    t.ok(await p.eval("document.querySelector('#c-free').value === 'my-new-app'"), "Enter keeps the free text as the value");
  },

  async "menu-keyboard"(t: T) {
    const p = t.page;
    await p.goto("/gallery");
    const btn = ".dz-menu .dz-menu-btn";
    await p.focus(btn);
    await p.key("Enter");
    await p.waitFor("document.querySelector('.dz-menu-btn').getAttribute('aria-expanded') === 'true'");
    await Bun.sleep(100);
    t.ok(await p.eval("document.activeElement.getAttribute('role') === 'menuitem'"), "opening moves focus into the menu");
    await p.shot(join(t.shots, "menu-open.png"));
    t.ok(await p.eval("document.querySelector('.dz-item.danger') !== null"), "a destructive item is marked");
    await p.key("ArrowDown");
    await p.key("ArrowDown");
    t.ok(await p.eval("document.activeElement.textContent === 'Delete topic'"), "arrows skip the separator");
    await p.key("ArrowDown");
    t.ok(await p.eval("document.activeElement.textContent === 'Open the topic'"), "arrows wrap");
    await p.key("Escape");
    await p.waitFor("document.querySelector('.dz-menu-btn').getAttribute('aria-expanded') === 'false'");
    t.ok(await p.eval("document.activeElement.classList.contains('dz-menu-btn')"), "Esc returns focus to the button");
  },

  async "tooltip-delay"(t: T) {
    const p = t.page;
    await p.goto("/gallery");
    const trig = ".dz-tip:first-of-type .btn";
    await p.hover(".dz-tip .btn");
    await Bun.sleep(150);
    t.ok(!(await visible(p, "#t1")), "not yet at 150 ms");
    await Bun.sleep(400);
    t.ok(await visible(p, "#t1"), "shown after 400 ms of hover");
    t.ok((await p.text("#t1")) === "Copy the address", "the tooltip's text");
    await p.move(5, 5);
    await Bun.sleep(100);
    t.ok(!(await visible(p, "#t1")), "gone when the pointer leaves");
    await p.focus(".dz-tip .btn");
    await Bun.sleep(100);
    t.ok(await visible(p, "#t1"), "focus shows it at once");
    await p.key("Escape");
    await Bun.sleep(80);
    t.ok(!(await visible(p, "#t1")), "Esc hides it");
    void trig;
  },

  async "segmented-roving"(t: T) {
    const p = t.page;
    await p.goto("/gallery");
    const seg = ".dz-seg[aria-label=Partitions]";
    t.ok(await p.eval(`document.querySelectorAll(${JSON.stringify(seg + " [tabindex='0']")}).length === 1`), "one stop in the Tab order");
    await p.focus(`${seg} [data-seg][tabindex='0']`);
    await p.key("ArrowRight");
    t.ok(await p.eval("document.activeElement.textContent === '0'"), "ArrowRight moves to the next");
    t.ok(await p.eval(`document.querySelectorAll(${JSON.stringify(seg + " [tabindex='0']")}).length === 1 && document.activeElement.getAttribute('tabindex') === '0'`), "the stop travels with focus");
    await p.key("End");
    t.ok(await p.eval("document.activeElement.textContent === '2'"), "End goes to the last");
    await p.key("ArrowRight");
    t.ok(await p.eval("document.activeElement.textContent === 'All partitions'"), "it wraps");
  },

  // A filter changes only its own region: no scroll jump, no shift above, focus and the address.
  async "filter-in-place"(t: T) {
    const p = t.page;
    await p.goto("/gallery");
    await p.waitFor("!!document.querySelector('#g-partition')");
    await p.eval("window.__marker = 'same'");
    await p.scrollTo(150);
    const y0 = await p.scrollY();
    const tool0 = await p.rect("form.toolbar");
    await p.resetShift();
    await p.click(open("g-partition"), { scroll: false });
    await p.waitFor(`document.querySelector(${JSON.stringify(open("g-partition"))}).getAttribute('aria-expanded') === 'true'`);
    await p.waitFor(`!!document.querySelector(${JSON.stringify(open("g-partition"))}).getAttribute('aria-activedescendant')`);
    await p.key("ArrowDown");
    await p.key("Enter");
    await p.waitFor("location.search.includes('partition=0')");
    await p.waitFor("document.querySelector('[data-test=chosen] b').textContent === '0'");
    await Bun.sleep(300);
    const y1 = await p.scrollY();
    const tool1 = await p.rect("form.toolbar");
    t.num("scrollY.before", y0); t.num("scrollY.after", y1); t.num("toolbar.top.before", tool0.top); t.num("toolbar.top.after", tool1.top); t.num("shift", await p.shift());
    t.near(y1, y0, 0.5, "scrollY is unchanged by a filter");
    t.near(tool1.top, tool0.top, 0.5, "the toolbar did not move");
    t.ok(await p.eval("window.__marker === 'same'"), "no navigation: the document is the same");
    t.ok((await p.shift()) < 0.02, "layout shift under 0.02");
    t.ok(await p.eval("document.activeElement.getAttribute('role') === 'combobox' && document.activeElement.closest('.dz-select') !== null"), "focus stays on the control that was used");
    t.ok(await p.eval("location.search.includes('partition=0')"), "the address records the filter (pushed)");
    await p.back();
    await p.waitFor("!location.search.includes('partition=0')");
    await p.waitFor("document.querySelector('[data-test=chosen] b').textContent === 'all'");
    t.ok(true, "Back undoes the filter");
  },
};

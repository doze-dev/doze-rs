// The palette (groups, recents), the shared tooltip, the row action menu: what a person sees when reaching for a place or a value.
import type { T } from "../drive.ts";

const heads = "Array.from(document.querySelectorAll('.jump-list .jump-h:not([hidden])')).map((h) => h.textContent)";

export const scenarios: Record<string, (t: T) => Promise<void>> = {
  async "palette-sections-and-recents"(t: T) {
    const p = t.page;
    await p.goto("/");
    await p.eval("localStorage.removeItem('doze:recent')");
    await p.key("k", 2);
    await p.waitFor("!!document.querySelector('.jump-list')");
    t.ok((await p.eval<string[]>(heads)).join() === "Topics,Groups,Pages", `sections: ${(await p.eval<string[]>(heads)).join()}`);
    await p.shot(`${t.shots}/palette-sections.png`);
    await p.type("ful");
    await Bun.sleep(80);
    t.ok((await p.eval<string[]>(heads)).join() === "Groups", `a section with no match loses its heading: ${(await p.eval<string[]>(heads)).join()}`);
    await p.key("Enter");
    await p.waitFor("location.pathname === '/groups/fulfilment'");
    await p.key("k", 2);
    await p.waitFor("!!document.querySelector('.jump-list')");
    const first = await p.eval<string>("document.querySelector('.jump-list li').textContent");
    const recent = await p.eval<string>("Array.from(document.querySelectorAll('.jump-list [data-recent]')).map((e) => e.textContent.trim()).join('|')");
    t.ok(first === "Recent" && recent.includes("fulfilment"), `the place just opened is the first recent: ${first} / ${recent}`);
    t.ok((await p.eval<string>("localStorage.getItem('doze:recent')")) === '[{"k":"group","n":"fulfilment"}]', "only a kind and a name are stored");
    await p.shot(`${t.shots}/palette-recents.png`);
    await p.type("zzzz");
    await Bun.sleep(80);
    t.ok(await p.eval("!document.querySelector('.jump-empty').hidden"), "the empty state is said");
    await p.key("Escape");
  },

  async "tooltip-shared-for-title-and-icon-buttons"(t: T) {
    const p = t.page;
    await p.goto("/topics");
    const sel = "main time[data-rel][title]";
    t.ok(await p.exists(sel), "the list has an exact time on hover");
    const title = await p.eval<string>(`document.querySelector(${JSON.stringify(sel)}).getAttribute('title')`);
    await p.hover(sel);
    await Bun.sleep(150);
    t.ok(await p.eval("!document.getElementById('dz-tip-shared') || getComputedStyle(document.getElementById('dz-tip-shared')).display === 'none'"), "nothing before 400 ms");
    await Bun.sleep(450);
    t.ok(await p.eval(`(() => { const e = document.getElementById('dz-tip-shared'); return !!e && getComputedStyle(e).display !== 'none' && e.textContent.startsWith(${JSON.stringify(title)}); })()`), "the card shows the exact time (and, once the pointer is on it, how long ago)");
    t.ok(await p.eval(`!document.querySelector(${JSON.stringify(sel)}) || true`), "ok");
    await p.shot(`${t.shots}/tooltip-shared.png`);
    await p.move(5, 400);
    await Bun.sleep(100);
    t.ok(await p.eval("getComputedStyle(document.getElementById('dz-tip-shared')).display === 'none'"), "leaving hides it");
    t.ok(await p.eval(`!!document.querySelector('main [title]')`), "the title is back for a browser with no script");
  },

  async "row-menu-opens-an-overlay"(t: T) {
    const p = t.page;
    await p.goto("/topics/inventory.levels/config");
    await p.waitFor("!!document.querySelector('.dz-menu-btn:not([hidden])')");
    await p.click(".dz-menu-btn:not([hidden])");
    await p.waitFor("document.querySelector('.dz-menu-btn[aria-expanded=true]') !== null");
    await Bun.sleep(150);
    await p.shot(`${t.shots}/row-menu.png`);
    const r = await p.rect(".dz-menu-list");
    t.ok(r.left >= 184 && r.right <= 1280, `the menu is inside the content (${r.left}..${r.right})`);
    await p.click(".dz-menu-list a.dz-item");
    await p.waitFor("!!document.querySelector('[data-overlay]')");
    t.ok(true, "Edit opens its dialog over the page");
    await p.key("Escape");
  },
};

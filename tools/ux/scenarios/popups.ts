// No menu, select or popover overlaps the rail, goes under the top bar or leaves the window (Floating UI knows the window, not the rail).
import type { T } from "../drive.ts";

const rect = (sel: string) => `(() => { const r = document.querySelector(${JSON.stringify(sel)}).getBoundingClientRect(); return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width }; })()`;

export const scenarios: Record<string, (t: T) => Promise<void>> = {
  async "popups-stay-clear-of-the-rail"(t: T) {
    const p = t.page;
    await p.goto("/gallery");
    const railRight = await p.eval<number>("document.getElementById('rail').getBoundingClientRect().right");
    t.num("rail.right", railRight);
    // Put the menu's button at the far left of the content, where a bottom-end card would run over the rail, and open it.
    await p.eval("(() => { const m = document.querySelector('.dz-menu'); m.style.position = 'fixed'; m.style.left = (document.getElementById('rail').getBoundingClientRect().right + 6) + 'px'; m.style.top = '300px'; m.style.zIndex = 5; })()");
    await p.click(".dz-menu .dz-menu-btn", { scroll: false });
    await p.waitFor("document.querySelector('.dz-menu-btn').getAttribute('aria-expanded') === 'true'");
    await Bun.sleep(250);
    const m = await p.eval<{ left: number; right: number; top: number; bottom: number }>(rect(".dz-menu-list"));
    t.num("menu.left", m.left);
    t.ok(m.left >= railRight, `the menu starts at ${m.left}, right of the rail (${railRight})`);
    t.ok(m.right <= 1280 && m.top >= 52 && m.bottom <= 800, "the menu is inside the window and under the top bar");
    await p.shot(`${t.shots}/menu-by-the-rail.png`);
    await p.key("Escape");
    // The same at the right edge and near the top bar.
    await p.eval("(() => { const m = document.querySelector('.dz-menu'); m.style.left = 'auto'; m.style.right = '4px'; m.style.top = '58px'; })()");
    await p.click(".dz-menu .dz-menu-btn", { scroll: false });
    await p.waitFor("document.querySelector('.dz-menu-btn').getAttribute('aria-expanded') === 'true'");
    await Bun.sleep(250);
    const r = await p.eval<{ left: number; right: number; top: number; bottom: number }>(rect(".dz-menu-list"));
    t.ok(r.right <= 1280 - 7 && r.top >= 52, `at the right edge: right ${r.right}, top ${r.top}`);
    await p.key("Escape");
  },
};

// Drawer and dialog geometry and motion (owner finding 2): the drawer is as tall as the window, on the right edge, above the top bar;
// it slides in from the right edge; the page behind does not move; Esc, the scrim and Back close it and give focus back.
import { join } from "node:path";
import type { T } from "../drive.ts";

const OPENER = "main table.t a[data-dialog]";

async function openDrawer(t: T, scrolled: number) {
  const p = t.page;
  await p.goto("/topics/orders/messages");
  await p.waitFor(`!!document.querySelector(${JSON.stringify(OPENER)})`);
  await p.scrollTo(scrolled);
  const y = await p.scrollY();
  (globalThis as { __pageUrl?: string }).__pageUrl = await p.eval<string>("location.pathname + location.search");
  await p.click(OPENER, { scroll: false });
  await p.waitFor("!!document.querySelector('.ovl-drawer .drawer-panel')");
  return y;
}

export const scenarios = {
  async "drawer-geometry"(t: T) {
    const p = t.page;
    const y0 = await openDrawer(t, 0);
    await Bun.sleep(400);
    const vw = await p.eval<number>("document.documentElement.clientWidth");
    const vh = await p.eval<number>("window.innerHeight");
    const r = await p.rect(".ovl-drawer .drawer-panel");
    t.num("drawer.top", r.top); t.num("drawer.height", r.height); t.num("drawer.right", r.right); t.num("drawer.width", r.width); t.num("viewport.h", vh); t.num("viewport.w", vw);
    t.near(r.top, 0, 0.5, "the drawer starts at y = 0");
    t.near(r.height, vh, 0.5, "the drawer is as tall as the window");
    t.near(r.right, vw, 1, "the drawer's right edge is the window's right edge");
    t.near(r.width, 480, 1, "the drawer is 480 px wide");
    const above = await p.eval<boolean>(`(() => { const r = document.querySelector('.ovl-drawer .drawer-panel').getBoundingClientRect(); const e = document.elementFromPoint(r.left + 40, 20); return !!e && !!e.closest('.drawer-panel'); })()`);
    t.ok(above, "the drawer is above the sticky top bar (the element at the top-bar height inside the drawer's column is the drawer's)");
    const z = await p.eval<number>("Number(getComputedStyle(document.querySelector('.ovl')).zIndex)");
    const zb = await p.eval<number>("Number(getComputedStyle(document.querySelector('.topbar')).zIndex)");
    t.ok(z > zb, `overlay z-index ${z} above the top bar ${zb}`);
    t.ok(await p.eval("getComputedStyle(document.querySelector('.drawer-panel')).boxShadow.includes('-16px')"), "a shadow falls to the left of the drawer");
    await p.shot(join(t.shots, "drawer-open.png"));
    t.num("y.before", y0); t.num("y.after", await p.scrollY());
    t.near(await p.scrollY(), y0, 0.5, "the page behind did not move");
  },

  async "drawer-motion"(t: T) {
    const p = t.page;
    await p.goto("/topics/orders/messages");
    await p.waitFor(`!!document.querySelector(${JSON.stringify(OPENER)})`);
    await p.animationRate(0.05);
    await p.click(OPENER);
    await p.waitFor("!!document.querySelector('.ovl-drawer .drawer-panel')");
    const m = await p.eval<{ tx: number; w: number; dur: number; easing: string; scrim: number; kind: string }>(`(() => {
      const panel = document.querySelector('.drawer-panel');
      const a = panel.getAnimations().find((x) => x.animationName === 'ovl-slide');
      const s = document.querySelector('.ovl .scrim').getAnimations()[0];
      a.pause(); a.currentTime = 0;
      const tx = new DOMMatrix(getComputedStyle(panel).transform).m41;
      const t = a.effect.getTiming(); const cs = a.effect.getComputedTiming();
      return { tx, w: panel.getBoundingClientRect().width, dur: cs.duration, easing: (getComputedStyle(panel).animationTimingFunction.match(/cubic-bezier\\([^)]*\\)/) || [''])[0], scrim: s ? s.effect.getComputedTiming().duration : -1, kind: a.animationName };
    })()`);
    await p.animationRate(1);
    t.num("slide.startX", m.tx); t.num("slide.width", m.w); t.num("slide.ms", m.dur); t.num("scrim.ms", m.scrim);
    t.near(m.tx, m.w, 1, "the slide starts one panel-width to the right (translateX(100%)), at the window's edge");
    t.near(m.dur, 240, 1, "slide-in lasts 240 ms");
    t.near(m.scrim, 180, 1, "the scrim fades in 180 ms");
    t.ok(m.easing.replace(/ /g, "") === "cubic-bezier(0.22,1,0.36,1)", `ease-out cubic-bezier(.22,1,.36,1), got ${m.easing}`);
    // Closing slides out over 180 ms.
    await p.eval("document.querySelectorAll('.ovl *').forEach((e) => e.getAnimations().forEach((a) => a.finish()))");
    await p.animationRate(0.05);
    await p.key("Escape");
    await p.waitFor("!!document.querySelector('.ovl-out')", 1500);
    const out = await p.eval<number>("document.querySelector('.ovl-out .drawer-panel').getAnimations()[0].effect.getComputedTiming().duration");
    t.num("slide.out.ms", out);
    t.near(out, 180, 1, "slide-out lasts 180 ms");
    await p.animationRate(1);
    await p.waitFor("!document.querySelector('.ovl')", 3000);
  },

  async "drawer-closes-three-ways-and-keeps-the-page"(t: T) {
    const p = t.page;
    const y0 = await openDrawer(t, 240);
    const url0 = (globalThis as { __pageUrl?: string }).__pageUrl as string;
    const opener = await p.eval<string>("document.querySelector('[data-dialog]') && 'x'");
    t.ok(!!opener, "there is an opener");
    t.ok((await p.eval<string>("location.pathname")).includes("/messages/"), "the address is the record's");
    await Bun.sleep(300);
    t.ok(await p.eval("document.querySelector('.root').hasAttribute('inert')"), "the page behind is inert");
    // Esc
    await p.key("Escape");
    await p.waitFor("!document.querySelector('.ovl')", 2000);
    t.near(await p.scrollY(), y0, 1, "Esc: the page is where it was");
    t.ok((await p.eval<string>("location.pathname + location.search")) === url0, "Esc: the address is the page's again");
    t.ok(await p.eval("document.activeElement && document.activeElement.matches('a[data-dialog]')"), "Esc: focus is back on the opener");
    t.ok(!(await p.eval("document.querySelector('.root').hasAttribute('inert')")), "Esc: the page is live again");
    // scrim
    await p.click(OPENER, { scroll: false });
    await p.waitFor("!!document.querySelector('.ovl-drawer .drawer-panel')");
    await Bun.sleep(300);
    await p.move(100, 400);
    await p.click(".ovl .scrim", { scroll: false });
    await p.waitFor("!document.querySelector('.ovl')", 2000);
    t.near(await p.scrollY(), y0, 1, "scrim: the page is where it was");
    // Back
    await p.click(OPENER, { scroll: false });
    await p.waitFor("!!document.querySelector('.ovl-drawer .drawer-panel')");
    await Bun.sleep(300);
    await p.back();
    await p.waitFor("!document.querySelector('.ovl')", 2000);
    t.ok((await p.eval<string>("location.pathname + location.search")) === url0, "Back: the address is the page's again");
    t.near(await p.scrollY(), y0, 1, "Back: the page is where it was");
    t.ok(await p.eval("!!document.querySelector('main table.t')"), "Back: the page behind was not reloaded or lost (its table is there)");
  },

  async "dialog-geometry"(t: T) {
    const p = t.page;
    await p.goto("/topics/orders");
    await p.waitFor("!!document.querySelector('a[data-dialog][href$=\"/produce\"]')");
    await p.animationRate(0.05);
    await p.click("a[data-dialog][href$=\"/produce\"]");
    await p.waitFor("!!document.querySelector('.ovl-dialog .modal')");
    const m = await p.eval<{ from: string; dur: number }>(`(() => {
      const modal = document.querySelector('.modal');
      const a = modal.getAnimations().find((x) => x.animationName === 'ovl-pop');
      a.pause(); a.currentTime = 0;
      return { from: getComputedStyle(modal).transform, dur: a.effect.getComputedTiming().duration };
    })()`);
    await p.animationRate(1);
    t.num("dialog.ms", m.dur);
    t.near(m.dur, 180, 1, "the dialog comes in over 180 ms");
    t.ok(/matrix\(0\.98/.test(m.from), `the dialog starts at scale .98, got ${m.from}`);
    await p.eval("document.querySelectorAll('.ovl *').forEach((e) => e.getAnimations().forEach((a) => a.finish()))");
    await Bun.sleep(100);
    const r = await p.rect(".ovl-dialog .modal");
    const vw = await p.eval<number>("document.documentElement.clientWidth");
    t.near(r.left + r.width / 2, vw / 2, 2, "the dialog is centred");
    await p.shot(join(t.shots, "dialog-open.png"));
    await p.key("Escape");
    await p.waitFor("!document.querySelector('.ovl')", 2000);
  },

  async "scroll-lock-does-not-shift-the-page"(t: T) {
    const p = t.page;
    await p.goto("/topics/orders/messages");
    await p.waitFor(`!!document.querySelector(${JSON.stringify(OPENER)})`);
    const before = await p.rect("main");
    await p.resetShift();
    await p.click(OPENER);
    await p.waitFor("!!document.querySelector('.ovl-drawer .drawer-panel')");
    await Bun.sleep(350);
    const during = await p.rect("main");
    t.num("main.left.before", before.left); t.num("main.left.during", during.left);
    t.near(during.left, before.left, 0.5, "the page's left edge does not move when the scroll lock engages");
    t.near(during.width, before.width, 0.5, "the page's width does not change when the scroll lock engages");
    t.num("shift", await p.shift());
    t.ok((await p.shift()) < 0.02, "no layout shift on open");
    await p.key("Escape");
    await p.waitFor("!document.querySelector('.ovl')", 2000);
  },
};

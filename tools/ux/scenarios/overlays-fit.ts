// Every overlay of the fixtures server fits the window at three sizes and scrolls inside itself (the owner: the Produce dialog was cut
// off at the bottom with no way to scroll, its buttons out of reach). Dialog: inside the window, the foot with its buttons in view
// WITHOUT scrolling, the body scrolls when it is longer. Drawer: from the top to the bottom of the window on the right edge, scrolls
// inside itself. The page behind does not scroll.
import { join } from "node:path";
import type { T } from "../drive.ts";

const SIZES: [number, number][] = [[1280, 720], [1024, 640], [800, 600]];
export const OVERLAYS: string[] = [
  "/topics/orders/produce", "/topics/inventory.levels/produce", "/topics/orders/delete", "/topics/orders/partitions/add", "/topics/orders/delete-records?partition=0",
  "/topics/inventory.levels/config/edit?key=cleanup.policy", "/topics/inventory.levels/config/edit?key=retention.ms", "/topics/inventory.levels/config/edit?key=compression.type",
  "/topics/inventory.levels/config/edit?key=segment.bytes", "/topics/inventory.levels/config/reset?key=cleanup.policy",
  "/groups/fulfilment/reset", "/groups/analytics/reset", "/groups/fulfilment/delete", "/groups/fulfilment/offsets/delete",
  "/groups/fulfilment/members/fulfilment-py-1", "/groups/fulfilment/story/31", "/topics/orders/messages/0/16103", "/live/12", "/live/clients",
];

export const scenarios: Record<string, (t: T) => Promise<void>> = {
  async "overlays-fit-and-scroll-at-three-sizes"(t: T) {
    const p = t.page;
    let worst = 0;
    try {
      for (const [w, h] of SIZES) {
        await p.viewport(w, h);
        for (const path of OVERLAYS) {
          await p.goto(path);
          await p.waitFor("!!document.querySelector('.ovl .modal, .ovl .drawer-panel')", 4000);
          await Bun.sleep(320);
          const tag = `${path} @${w}x${h}`;
          const m = await p.eval<{ kind: string; top: number; bottom: number; left: number; right: number; foot: { bottom: number; btns: number; hidden: number } | null; scrolls: boolean; overflow: boolean; bodyOverflow: string; panelOverflow: string; htmlOverflow: string; scrolled: boolean; buttonsOut: number }>(`(() => {
            const modal = document.querySelector('.ovl .modal'), panel = modal || document.querySelector('.ovl .drawer-panel');
            const r = panel.getBoundingClientRect();
            const body = modal && modal.querySelector('.body'), foot = modal && modal.querySelector('.foot');
            const scroller = body || panel;
            const before = scroller.scrollTop;
            scroller.scrollTop = scroller.scrollHeight;
            const scrolled = scroller.scrollTop !== before;
            scroller.scrollTop = before;
            const fr = foot && foot.getBoundingClientRect();
            const btns = foot ? [...foot.querySelectorAll('button,a.btn')] : [];
            return { kind: modal ? 'dialog' : 'drawer', top: r.top, bottom: r.bottom, left: r.left, right: r.right,
              foot: foot ? { bottom: fr.bottom, btns: btns.length, hidden: btns.filter((b) => { const q = b.getBoundingClientRect(); return q.bottom > innerHeight || q.top < 0 || q.height === 0; }).length } : null,
              scrolls: scroller.scrollHeight > scroller.clientHeight + 1, overflow: scroller.scrollHeight > scroller.clientHeight + 1, scrolled,
              bodyOverflow: body ? getComputedStyle(body).overflowY : '', panelOverflow: getComputedStyle(panel).overflowY, htmlOverflow: getComputedStyle(document.documentElement).overflow, buttonsOut: 0 };
          })()`);
          t.ok(m.top >= -0.5 && m.bottom <= h + 0.5 && m.left >= -0.5 && m.right <= w + 0.5, `${tag}: the panel is inside the window (${Math.round(m.left)},${Math.round(m.top)} to ${Math.round(m.right)},${Math.round(m.bottom)})`);
          if (m.kind === "drawer") {
            t.ok(Math.abs(m.top) < 1 && Math.abs(m.bottom - h) < 1 && Math.abs(m.right - w) < 1, `${tag}: the drawer runs from the top to the bottom of the window on the right edge`);
            t.ok(m.panelOverflow === "auto" || m.panelOverflow === "scroll", `${tag}: the drawer scrolls inside itself`);
          } else {
            t.ok(!!m.foot && m.foot.bottom <= h + 0.5, `${tag}: the foot is in view without scrolling (bottom ${Math.round(m.foot?.bottom ?? -1)})`);
            t.ok(!!m.foot && m.foot.hidden === 0 && m.foot.btns > 0, `${tag}: its ${m.foot?.btns} button(s) are all visible`);
            t.ok(m.bodyOverflow === "auto", `${tag}: the body scrolls inside the dialog`);
          }
          if (m.overflow) t.ok(m.scrolled, `${tag}: the content is longer than the window and it scrolls`);
          t.ok(m.htmlOverflow === "hidden", `${tag}: the page behind does not scroll`);
          worst = Math.max(worst, m.bottom);
          if (path.endsWith("/produce") && path.includes("orders")) await p.shot(join(t.shots, `overlay-produce-${w}x${h}.png`));
        }
      }
    } finally {
      await p.viewport(1280, 800);
    }
    t.num("overlays", OVERLAYS.length);
    t.num("sizes", SIZES.length);
  },
};

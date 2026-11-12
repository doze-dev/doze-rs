// One focus system: every kind of control, focused, shows exactly ONE ring, 2 px, the primary colour, on its outermost shape (the
// owner: a purple rectangle inside the search pill, "might be there in other places too"). A control that is a box around an input
// draws the ring on the box and its input draws none; a control that is its own shape draws it on itself.
import { join } from "node:path";
import type { T } from "../drive.ts";

const CASES: [string, string, string][] = [
  ["/topics", ".search input", "top bar search (a box around an input)"],
  ["/topics", "form.toolbar .finput input", "filter field"],
  ["/topics", "form.toolbar button[name=internal]", "toggle chip"],
  ["/topics", "form.toolbar button[data-ui=refresh]", "button"],
  ["/topics", ".tabs a.tab:not(.on)", "tab"],
  ["/topics", "main table.t a.mono", "row link"],
  ["/topics", "nav.crumbs a", "crumb"],
  ["/topics", ".rail a.ni", "rail item"],
  ["/groups", "[aria-label=State] a", "segmented control"],
  ["/live", ".dz-chip .dz-trigger", "select trigger (chip)"],
  ["/live", "form.toolbar button[name=refused]", "filter chip"],
  ["/topics/orders/messages", ".dz-pov-btn", "popover button"],
  ["/topics/orders/messages", "#topic-explained > summary", "disclosure"],
  ["/topics/orders/messages", "a.btn.pri", "primary button"],
  ["/topics/inventory.levels/config", ".dz-menu-btn", "menu button"],
  ["/gallery", ".dz-combobox .dz-input", "combobox field (a box around an input)"],
  ["/gallery", ".dz-select .dz-trigger", "select trigger"],
  ["/topics/orders/produce", "textarea", "textarea in a field"],
  ["/topics/orders/produce", ".modal input[type=text], .ovl input[type=text]", "text field in a dialog"],
];

export const scenarios: Record<string, (t: T) => Promise<void>> = {
  async "focus-one-ring-on-the-outer-shape"(t: T) {
    const p = t.page;
    let n = 0;
    for (const [path, sel, what] of CASES) {
      await p.goto(path);
      await p.waitFor(`!!document.querySelector(${JSON.stringify(sel)})`, 4000);
      await p.eval(`(() => { const e = [...document.querySelectorAll(${JSON.stringify(sel)})].find((e) => e.getBoundingClientRect().width > 0 && !e.hidden && getComputedStyle(e).visibility !== 'hidden'); e.scrollIntoView({ block: 'center', behavior: 'instant' }); e.focus(); window.__f = e; })()`);
      await Bun.sleep(80);
      const r = await p.eval<{ rings: string[]; own: boolean; focused: boolean; colour: string }>(`(() => {
        const rings = []; let own = false, colour = '';
        for (let e = window.__f, i = 0; e && i < 4; e = e.parentElement, i++) {
          const s = getComputedStyle(e);
          if (s.outlineStyle !== 'none' && parseFloat(s.outlineWidth) >= 2 && s.outlineColor !== 'rgba(0, 0, 0, 0)') { rings.push(e.tagName + '.' + e.className); if (i === 0) own = true; colour = s.outlineColor; }
        }
        return { rings, own, focused: document.activeElement === window.__f, colour };
      })()`);
      t.ok(r.focused, `${what}: it took the focus`);
      t.ok(r.rings.length === 1, `${what}: exactly one ring (${r.rings.join(" | ") || "none"})`);
      t.ok(/rgba\(122, 79, 224, 0\.45\)|rgb\(122, 79, 224\)/.test(r.colour), `${what}: the ring is the primary colour at 45% (${r.colour})`);
      // A field that is a box around an input: the ring is on the box, never on the input.
      const inner = await p.eval<boolean>(`window.__f.matches('input,textarea') && window.__f.closest('.search,.finput,.input,.dz-field') !== null`);
      if (inner) t.ok(!r.own, `${what}: the input inside its box draws no ring of its own`);
      const box = await p.eval<{ x: number; y: number; width: number; height: number }>(`(() => { const b = (window.__f.closest('.search,.finput,.input,.dz-field') || window.__f).getBoundingClientRect(); return { x: Math.max(0, b.x - 14), y: Math.max(0, b.y - 14), width: b.width + 28, height: b.height + 28 }; })()`);
      await p.shot(join(t.shots, `focus-${n++}.png`), box);
    }
    t.num("controls", CASES.length);
  },
};

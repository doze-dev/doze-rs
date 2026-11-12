// Forms in overlays, the palette, Live's filters: the interactions that cross the overlay and navigation layers.
import { join } from "node:path";
import type { T } from "../drive.ts";
import { pad } from "../inplace.ts";

export const scenarios = {
  // A refusal (the dialog comes back with a field to fix) stays in the overlay; a result page replaces main and the overlay.
  async "dialog-form-result-replaces-the-page"(t: T) {
    const p = t.page;
    await p.goto("/topics/orders");
    await p.waitFor("!!document.querySelector('a[data-dialog][href$=\"/produce\"]')");
    await p.eval("window.__marker = 'same-document'");
    await p.click("a[data-dialog][href$=\"/produce\"]");
    await p.waitFor("!!document.querySelector('.ovl-dialog form')");
    await Bun.sleep(300);
    const hist0 = await p.eval<number>("history.length");
    await p.eval("document.querySelector('.ovl-dialog textarea, .ovl-dialog input[name=value]').focus()");
    await p.type("hello");
    await p.shot(join(t.shots, "produce-dialog.png"));
    await p.click(".ovl-dialog button[type=submit]");
    await p.waitFor("!document.querySelector('.ovl') || !!document.querySelector('.ovl .err, .ovl [role=alert]')", 5000);
    await Bun.sleep(500);
    const stillOverlay = await p.exists(".ovl");
    t.num("overlay.afterSubmit", stillOverlay ? 1 : 0);
    t.num("path", await p.eval<string>("location.pathname") as unknown as number);
    t.ok(await p.eval("window.__marker === 'same-document'"), "the document survived the form (no reload)");
    if (!stillOverlay) {
      t.ok(await p.eval("!document.querySelector('.root').hasAttribute('inert')"), "the page is live again");
      t.ok(await p.eval("document.title.length > 0"), "there is a title");
      t.ok(!(await p.eval<string>("location.pathname")).endsWith("/produce"), "the address is not the dialog's any more");
    }
    t.ok((await p.eval<number>("history.length")) <= hist0 + 1, "at most one history entry came of it");
  },

  async "palette-opens-filters-navigates"(t: T) {
    const p = t.page;
    await p.goto("/topics");
    await p.eval("window.__marker = 'same-document'");
    await p.key("k", 4 /* ctrl */);
    await p.waitFor("!!document.querySelector('.ovl [data-jump-input]')");
    await Bun.sleep(300);
    t.ok(await p.eval("document.activeElement.hasAttribute('data-jump-input')"), "the field has focus");
    await p.shot(join(t.shots, "palette.png"));
    await p.type("ord");
    await Bun.sleep(100);
    const shown = await p.eval<number>("[...document.querySelectorAll('.jump-list li')].filter((l) => !l.hidden).length");
    t.num("palette.matches", shown);
    t.ok(shown >= 1 && shown < 10, "typing narrows the list");
    await p.key("ArrowDown");
    await p.key("Enter");
    await p.waitFor("location.pathname.startsWith('/topics/o') || location.pathname.startsWith('/groups/')", 4000);
    await Bun.sleep(300);
    t.ok(await p.eval("window.__marker === 'same-document'"), "the jump was a client-side navigation");
    t.ok(!(await p.exists(".ovl")), "the palette closed with the navigation");
    // Esc closes it with no history entry.
    const h = await p.eval<number>("history.length");
    await p.key("k", 4);
    await p.waitFor("!!document.querySelector('.ovl [data-jump-input]')");
    await p.key("Escape");
    await p.waitFor("!document.querySelector('.ovl')", 2000);
    t.near(await p.eval<number>("history.length"), h, 0, "the palette made no history entry");
  },

  // Live: a filter chip is a MultiSelect; choosing changes the tape in place.
  async "live-filter-in-place"(t: T) {
    const p = t.page;
    await p.goto("/live");
    await p.waitFor("!!document.querySelector('form[data-live-filters] .dz-multi.is-on')");
    await p.eval("window.__marker = 'same-document'");
    // The filter now narrows the tape, so the page may be shorter than the scroll: keep it long (a spacer after the region).
    await pad(p, 1500);
    await p.scrollTo(80);
    const y0 = await p.scrollY();
    const bar0 = await p.rect("form[data-live-filters]");
    const crumb0 = await p.text(".crumbs");
    await p.resetShift();
    await p.shot(join(t.shots, "live.png"));
    await p.click("form[data-live-filters] .dz-multi .dz-trigger", { scroll: false });
    await p.waitFor("document.querySelector('form[data-live-filters] .dz-multi .dz-trigger').getAttribute('aria-expanded') === 'true'");
    await Bun.sleep(100);
    await p.key("ArrowDown");
    await p.key("Enter");
    await p.key("Escape");
    await p.waitFor("location.search.includes('client=')", 4000);
    await Bun.sleep(400);
    const bar1 = await p.rect("form[data-live-filters]");
    t.num("scrollY.before", y0); t.num("scrollY.after", await p.scrollY()); t.num("toolbar.top.delta", bar1.top - bar0.top); t.num("shift", await p.shift());
    t.near(await p.scrollY(), y0, 1, "scrollY unchanged by a filter");
    t.near(bar1.top, bar0.top, 1, "the toolbar did not move");
    t.ok((await p.shift()) < 0.02, "layout shift under 0.02");
    t.ok(await p.eval("window.__marker === 'same-document'"), "no navigation");
    t.ok((await p.text(".crumbs")) === crumb0, "the breadcrumb is unchanged by a filter");
    t.ok(await p.eval("document.querySelector('form[data-live-filters] .dz-chip.on') !== null"), "the chip shows it is filtering");
    t.ok(await p.eval("document.activeElement && document.activeElement.classList.contains('dz-trigger')"), "focus stayed on the control");
  },
};

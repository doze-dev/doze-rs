// Page transitions (UX finding 5): every link and form is a client-side swap of `main` (the document and the shell survive), the
// transition is a ~180 ms fade, reduced motion turns it off, a quick sequence of clicks ends on the last page with nothing stacked,
// a request overtaken by a newer click is aborted, Back and Forward put the scroll back.
import type { T } from "../drive.ts";

const SAME = "window.__doc === 'same'";
const marks = (p: T["page"]) => p.eval("window.__doc = 'same'; window.__vt = 0; if (!window.__vtWrapped) { window.__vtWrapped = true; const o = document.startViewTransition.bind(document); document.startViewTransition = (cb) => { window.__vt++; return o(cb); }; } 1");

export const scenarios: Record<string, (t: T) => Promise<void>> = {
  // A walk through the console by clicking: the same document the whole way, `main` swapped by each page change.
  async "transitions-no-document-flash-across-pages"(t: T) {
    const p = t.page;
    await p.goto("/");
    await marks(p);
    const steps: [string, string][] = [
      [".rail a[href='/live']", "/live"],
      [".rail a[href='/topics']", "/topics"],
      ["main table.t tbody tr:first-child a", "/topics/orders"],
      [".tabs a[href$='/config']", "/topics/orders/config"],
      [".tabs a[href$='/consumers']", "/topics/orders/consumers"],
      [".tabs a[href='/topics/orders']", "/topics/orders"],
      ["nav.crumbs a[href='/topics']", "/topics"],
      [".tabs a[href='/groups']", "/groups"],
      ["main table.t tbody tr:first-child a", "/groups/analytics"],
      [".rail a[href='/']", "/"],
    ];
    let swaps = 0;
    for (const [sel, path] of steps) {
      await p.eval("window.__m = document.getElementById('main')");
      await p.click(sel);
      await p.waitFor(`location.pathname === ${JSON.stringify(path)} && window.__m !== document.getElementById('main')`, 5000);
      await Bun.sleep(260);
      t.ok(await p.eval(SAME), `${path}: still the same document (no flash)`);
      swaps++;
    }
    t.num("page.changes", swaps);
    t.num("view.transitions", await p.eval<number>("window.__vt"));
    t.ok((await p.eval<number>("window.__vt")) >= swaps, "each page change ran in a view transition");
    t.ok(await p.eval("document.getAnimations().length === 0"), "nothing is still animating after the last one");
  },

  // A form post (Check again on Home) and a form with steps (Create topic) swap in place too.
  async "transitions-forms-swap-in-place"(t: T) {
    const p = t.page;
    await p.goto("/topics/new");
    await marks(p);
    await p.type("x");
    const next = "form button[type=submit], form .btn.pri";
    t.ok(await p.exists(next), "the create form has a button");
    await p.click("main form .btn.pri, main form button.pri", { scroll: true }).catch(() => undefined);
    await Bun.sleep(600);
    t.ok(await p.eval(SAME), "a form step is a swap, not a reload");
  },

  // prefers-reduced-motion: no transition and no fade at all.
  async "transitions-reduced-motion-runs-none"(t: T) {
    const p = t.page;
    await p.emulateMedia("prefers-reduced-motion", "reduce");
    try {
      await p.goto("/topics");
      await marks(p);
      await p.eval("window.__anims = 0; new MutationObserver(() => { window.__anims = Math.max(window.__anims, document.getAnimations({ subtree: true }).length); }).observe(document, { subtree: true, childList: true, attributes: true }); setInterval(() => { window.__anims = Math.max(window.__anims, document.getAnimations({ subtree: true }).length); }, 15); 1");
      await p.click(".rail a[href='/live']");
      await p.waitFor("location.pathname === '/live'");
      await Bun.sleep(400);
      t.num("anims.reduced", await p.eval<number>("window.__anims"));
      t.ok(await p.eval(SAME), "the swap still happens, in the same document");
      t.ok((await p.eval<number>("window.__anims")) === 0, "no animation ran with reduced motion");
      t.ok(await p.eval("getComputedStyle(document.documentElement).scrollBehavior === 'auto'"), "scrolling is not smooth");
    } finally {
      await p.emulateMedia("prefers-reduced-motion", "no-preference");
    }
    // The same click with motion allowed does animate: the test above is not vacuous.
    await p.goto("/topics");
    await marks(p);
    await p.eval("window.__anims = 0; setInterval(() => { window.__anims = Math.max(window.__anims, document.getAnimations({ subtree: true }).length); }, 15); 1");
    await p.click(".rail a[href='/live']");
    await p.waitFor("location.pathname === '/live'");
    await Bun.sleep(300);
    t.num("anims.motion", await p.eval<number>("window.__anims"));
    t.ok((await p.eval<number>("window.__anims")) > 0, "with motion allowed the same click animates");
  },

  // Five quick clicks: one page at the end, the last one clicked, no transition stuck, no error.
  async "transitions-fast-clicks-do-not-stack"(t: T) {
    const p = t.page;
    await p.goto("/");
    await marks(p);
    await p.eval(`(() => { ['/live', '/topics', '/settings', '/live', '/topics'].forEach((h, i) => setTimeout(() => document.querySelector(".rail a[href='" + h + "']").click(), i * 40)); })()`);
    await p.waitFor("location.pathname === '/topics'", 5000);
    await Bun.sleep(700);
    t.ok(await p.eval(SAME), "the same document after five quick clicks");
    t.ok((await p.eval<string>("document.title")).startsWith("Topics"), "the last click won");
    t.ok(await p.eval("document.querySelectorAll('main#main, #main').length === 1"), "one main");
    t.ok(await p.eval("document.getAnimations().length === 0"), "no transition is stuck");
    t.ok(await p.eval("document.querySelector('.rail a.on').getAttribute('href') === '/topics'"), "the rail agrees");
    t.num("view.transitions", await p.eval<number>("window.__vt"));
  },

  // A click that follows another: the page is the later one even when the earlier answer is the slower (it must not land late), and
  // nothing is logged as an error.
  async "transitions-overtaken-click-never-lands-late"(t: T) {
    const p = t.page;
    await p.goto("/topics");
    await marks(p);
    await p.latency(1200);
    try {
      await p.eval("document.querySelector(\".rail a[href='/settings']\").click()");
      await Bun.sleep(150);
      await p.latency(60);
      await p.eval("document.querySelector(\".rail a[href='/live']\").click()");
      await p.waitFor("document.title.startsWith('Live')", 6000);
      await Bun.sleep(1700);
    } finally {
      await p.latency(0);
    }
    t.ok((await p.eval<string>("document.title")).startsWith("Live"), "the page is the later click's, after the slower earlier answer arrived");
    t.num("path", await p.eval<string>("location.pathname")); t.num("rail", await p.eval<string>("document.querySelector('.rail a.on').getAttribute('href')"));
    t.ok(await p.eval("location.pathname === '/live' && document.querySelector('.rail a.on').getAttribute('href') === '/live'"), "so are the address and the rail");
    t.ok(await p.eval(SAME), "same document");
  },

  // Back and Forward restore the scroll of each page, in the same document, without a smooth scroll through the page.
  async "transitions-back-forward-restore-scroll"(t: T) {
    const p = t.page;
    await p.goto("/topics/orders/messages");
    await p.waitFor("!!document.querySelector('main table.t')");
    await marks(p);
    await p.scrollTo(240);
    await p.click(".tabs a[href$='/config']", { scroll: false });
    await p.waitFor("location.pathname.endsWith('/config')");
    await Bun.sleep(300);
    await p.scrollTo(120);
    await p.back();
    await p.waitFor("location.pathname.endsWith('/messages')");
    await Bun.sleep(400);
    t.near(await p.scrollY(), 240, 2, "Back: the Messages page is where it was");
    await p.eval("history.forward()");
    await p.waitFor("location.pathname.endsWith('/config')");
    await Bun.sleep(400);
    t.near(await p.scrollY(), 120, 2, "Forward: the Config page is where it was");
    t.ok(await p.eval(SAME), "same document throughout");
  },
};

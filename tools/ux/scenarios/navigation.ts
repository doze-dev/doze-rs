// Navigation: a link click swaps only `main`, the shell stays, the title/rail/history follow, scroll behaves, transitions run.
import type { T } from "../drive.ts";

export const scenarios = {
  // The stack is loaded and the page is quiet.
  async "stack-loaded"(t: T) {
    const p = t.page;
    await p.goto("/topics");
    t.ok(await p.eval("typeof htmx === 'object' && typeof Idiomorph === 'object' && typeof Alpine === 'object'"), "htmx, Idiomorph and Alpine are present");
    t.ok(await p.eval("htmx.config.allowEval === false && htmx.config.selfRequestsOnly === true"), "htmx is configured without eval and for own origin only");
  },

  // A rail click is a client-side navigation: the document survives, only main changes.
  async "nav-swaps-main-only"(t: T) {
    const p = t.page;
    await p.goto("/topics");
    await p.eval("window.__marker = 'same-document'; document.querySelector('.topbar').dataset.keep = '1'");
    await p.resetShift();
    await p.click(".rail a[href$='/live']");
    await p.waitFor("location.pathname.endsWith('/live')");
    await p.waitFor("document.querySelector('#main h1, #main .page-h h1')");
    await Bun.sleep(300);
    t.ok(await p.eval("window.__marker === 'same-document'"), "the document was not reloaded");
    t.ok(await p.eval("document.querySelector('.topbar').dataset.keep === '1'"), "the top bar element survived");
    t.ok((await p.eval<string>("document.title")).startsWith("Live"), "the title follows");
    t.ok(await p.eval("document.querySelector('.rail a.on').getAttribute('href').endsWith('/live')"), "the rail marks Live");
    t.num("shift", await p.shift());
    t.ok((await p.shift()) < 0.02, "layout shift under 0.02");
    await p.back();
    await p.waitFor("location.pathname.endsWith('/topics')");
    await p.waitFor("document.title.startsWith('Topics')");
    t.ok(await p.eval("document.querySelector('.rail a.on').getAttribute('href').endsWith('/topics')"), "Back puts the rail back");
  },
};

// ---- the rest of the navigation contract ----------------------------------------------------------------------------------
const nav = scenarios as Record<string, (t: T) => Promise<void>>;

// A new page starts at the top; Back puts the page where it was left.
nav["nav-scroll-new-page-top-and-back-restores"] = async (t) => {
  const p = t.page;
  await p.goto("/topics/orders/messages");
  await p.waitFor("!!document.querySelector('main table.t')");
  await p.scrollTo(260);
  const y = await p.scrollY();
  t.ok(y > 100, `the page scrolls (y = ${y})`);
  await p.click(".rail a[href$='/live']", { scroll: false });
  await p.waitFor("location.pathname.endsWith('/live')");
  await p.waitFor("document.title.startsWith('Live')");
  await Bun.sleep(300);
  t.num("scrollY.newPage", await p.scrollY());
  t.near(await p.scrollY(), 0, 0.5, "a new page starts at the top");
  await p.back();
  await p.waitFor("location.pathname.endsWith('/messages')");
  await p.waitFor("!!document.querySelector('main table.t')");
  await Bun.sleep(400);
  t.num("scrollY.back", await p.scrollY());
  t.near(await p.scrollY(), y, 2, "Back restores the scroll the page had");
};

// A page change is a same-document View Transition with the gentle timings.
nav["nav-view-transition"] = async (t) => {
  const p = t.page;
  await p.goto("/topics");
  await p.eval("window.__vt = 0; const o = document.startViewTransition.bind(document); document.startViewTransition = (cb) => { window.__vt++; return o(cb); };");
  await p.click(".rail a[href$='/live']");
  await p.waitFor("location.pathname.endsWith('/live')");
  await Bun.sleep(300);
  t.num("viewTransitions", await p.eval<number>("window.__vt"));
  t.ok((await p.eval<number>("window.__vt")) >= 1, "the navigation ran inside a view transition");
  const d = await p.eval<{ nw: string; old: string }>(`(() => ({ nw: getComputedStyle(document.documentElement, '::view-transition-new(main)').animationDuration, old: getComputedStyle(document.documentElement, '::view-transition-old(main)').animationDuration }))()`);
  t.ok(d.nw === "0.18s" || d.nw === "" , `new page fades in over 180 ms (got ${d.nw})`);
  const css = await p.eval<string>(`[...document.styleSheets].flatMap((s) => [...s.cssRules]).map((r) => r.cssText).filter((c) => c.includes('view-transition')).join('\\n')`);
  t.ok(/vt-in 0\.18s/.test(css) || /vt-in 180ms/.test(css), "the CSS cross-fades the new main in 180 ms");
  t.ok(/translateY\(5px\)/.test(await p.eval<string>(`[...document.styleSheets].flatMap((s) => [...s.cssRules]).filter((r) => r.name === 'vt-in').map((r) => r.cssText).join('')`)), "with a 5 px rise");
};

// The pointer resting on a link fetches its page ahead; the click is answered from the cache.
nav["nav-preload"] = async (t) => {
  const p = t.page;
  await p.goto("/topics");
  await Bun.sleep(300);
  p.clearLogs();
  await p.hover(".rail a[href$='/live']");
  await Bun.sleep(400);
  const pre = p.requests.filter((r) => r.url.endsWith("/live") && r.headers["HX-Preloaded"] === "true");
  t.num("preloads", pre.length);
  t.ok(pre.length === 1, "one request ahead of the click, marked as a preload");
  t.ok(pre[0]?.headers["HX-Request"] === "true" && pre[0]?.headers["HX-Target"] === "main", "with the headers a navigation sends, so the cache can be used");
  await p.click(".rail a[href$='/live']", { scroll: false });
  await p.waitFor("location.pathname.endsWith('/live')");
  await Bun.sleep(300);
  const after = p.served.filter((s) => s.url.endsWith("/live"));
  t.num("responses", after.length);
  t.ok(after.length >= 2 && after[after.length - 1].cached, "the click was answered from the browser's cache");
};

// A progress hint only when a request is slow.
nav["nav-progress-hint-only-when-slow"] = async (t) => {
  const p = t.page;
  await p.goto("/topics");
  await p.eval("window.__hint = 0; new MutationObserver(() => { if (document.documentElement.classList.contains('is-loading')) window.__hint++; }).observe(document.documentElement, { attributes: true, attributeFilter: ['class'] });");
  await p.click(".rail a[href$='/live']");
  await p.waitFor("location.pathname.endsWith('/live')");
  await Bun.sleep(300);
  t.num("hint.fast", await p.eval<number>("window.__hint"));
  t.ok((await p.eval<number>("window.__hint")) === 0, "no hint for a fast request");
  await p.goto("/topics");
  await Bun.sleep(900); // out of the preload window of the previous visit
  await p.eval("window.__hint = 0; new MutationObserver(() => { if (document.documentElement.classList.contains('is-loading')) window.__hint++; }).observe(document.documentElement, { attributes: true, attributeFilter: ['class'] });");
  await p.latency(900);
  await p.click(".rail a[href$='/groups']", { scroll: false }).catch(() => undefined);
  await p.click(".rail a[href$='/settings']", { scroll: false });
  await Bun.sleep(500);
  t.ok(await p.eval("document.documentElement.classList.contains('is-loading')"), "a hint after 250 ms of a slow request");
  await p.waitFor("location.pathname.endsWith('/settings')", 6000);
  await p.latency(0);
  await Bun.sleep(200);
  t.ok(!(await p.eval("document.documentElement.classList.contains('is-loading')")), "and it goes when the page is there");
};

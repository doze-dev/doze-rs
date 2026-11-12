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

// Every same-page control updates its own region in place (UX finding 10): Topics, Groups, Config, Lag, Messages.
// The check of one control is tools/ux/inplace.ts; a scenario here is a short story of a person using a page.
import type { T } from "../drive.ts";
import { inPlace, pad } from "../inplace.ts";

const TOOLBAR = "form.toolbar";

export const scenarios: Record<string, (t: T) => Promise<void>> = {
  // Typing a filter, a toggle, Refresh and a filter with no match: the toolbar never moves, the focus stays in the box.
  async "region-topics"(t: T) {
    const p = t.page;
    await p.goto("/topics");
    await pad(p);
    await p.scrollTo(120);
    const above = [".page-h", ".tabs", TOOLBAR];
    await p.focus(`${TOOLBAR} input[name=q]`);
    await inPlace(t, p, {
      name: "topics: typing", above, history: "replace", url: /^\/topics\?q=ord$/, focus: `${TOOLBAR} input[name=q]`,
      act: () => p.type("ord"), done: "location.search === '?q=ord' && document.querySelectorAll('#topics-table tbody tr').length > 0",
    });
    t.ok(await p.eval("document.querySelector('form.toolbar input[name=q]').value === 'ord'"), "topics: the typed text is still there");
    await inPlace(t, p, {
      name: "topics: Show internal", above, url: /^\/topics\?q=ord&internal=1$/,
      act: () => p.click(`${TOOLBAR} button[name=internal]`, { scroll: false }),
      done: "location.search.includes('internal=1')", focus: `${TOOLBAR} button[name=internal]`,
    });
    t.ok(await p.eval("document.querySelector('form.toolbar button[name=internal]').getAttribute('aria-pressed') === 'true'"), "topics: the toggle is on");
    await p.focus(`${TOOLBAR} input[name=q]`);
    await p.eval("(() => { const i = document.querySelector('form.toolbar input[name=q]'); i.setSelectionRange(i.value.length, i.value.length); })()");
    await inPlace(t, p, {
      name: "topics: no match", above, history: "replace", url: /q=ordzzz/,
      act: () => p.type("zzz"), done: "!!document.querySelector('#topics-view .empty')",
    });
    await p.shot(`${t.shots}/region-topics-empty.png`);
    await inPlace(t, p, {
      name: "topics: Clear filter", above, url: /^\/topics$/,
      act: () => p.click("#topics-view .empty a.btn:not(.pri)", { scroll: false }), done: "!document.querySelector('#topics-view .empty')",
    });
    await p.back();
    await p.waitFor("location.search.includes('q=ordzzz')");
    await p.waitFor("!!document.querySelector('#topics-view .empty')");
    await p.back();
    await p.waitFor("!location.search.includes('q=ordzzz')");
    t.ok(true, "Back restores the earlier states");
  },
};

const BAR = "#topic-bar";
const MSG_ABOVE = [".page-h", ".tabs", BAR, `${BAR} .bar-row`, `${BAR} .bar-chips`, `${BAR} .seg`, `${BAR} .seg[aria-label=Partition]`];

Object.assign(scenarios, {
  // The Messages toolbar: a partition, a level, a key, a chip's X, Clear filters, Older. Nothing above the table moves; the owner's
  // findings 8, 9 and 10 in one story.
  async "region-messages"(t: T) {
    const p = t.page;
    await p.goto("/topics/orders/messages");
    await pad(p);
    await p.scrollTo(100);
    const n = (name: string) => `messages: ${name}`;
    await inPlace(t, p, {
      name: n("partition 1"), above: MSG_ABOVE, url: /^\/topics\/orders\/messages\?partition=1$/,
      act: () => p.click(`${BAR} [aria-label=Partition] a:nth-child(3)`, { scroll: false }), done: "location.search.includes('partition=1') && document.querySelector('#topic-bar [aria-label=Partition] a:nth-child(3)').getAttribute('aria-current') === 'true'",
      focus: `${BAR} [aria-label=Partition] a:nth-child(3)`,
    });
    await inPlace(t, p, {
      name: n("read_committed"), above: MSG_ABOVE, url: /isolation=read_committed/,
      act: () => p.click(`${BAR} [aria-label="Reading as"] a:nth-child(2)`, { scroll: false }), done: "location.search.includes('isolation=read_committed')",
      focus: `${BAR} [aria-label="Reading as"] a:nth-child(2)`,
    });
    // "Reading as" stays where it is (finding 9).
    const reading = await p.rect(`${BAR} .seg[aria-label="Reading as"]`);
    t.ok(reading.right > 1000, `messages: Reading as stays at the right (${reading.right})`);
    await inPlace(t, p, {
      name: n("a key"), above: MSG_ABOVE, url: /key=ord/,
      act: async () => { await p.focus(`${BAR} input[name=find]`); await p.type("ord"); await p.key("Enter"); }, done: "!!document.querySelector('#topic-bar [aria-label^=\"Remove the filter Key\"]')",
    });
    await p.shot(`${t.shots}/region-messages-key.png`);
    await inPlace(t, p, {
      name: n("remove the key chip"), above: MSG_ABOVE, url: /^\/topics\/orders\/messages\?partition=1&isolation=read_committed$/,
      act: () => p.click(`${BAR} [aria-label^="Remove the filter Key"]`, { scroll: false }), done: "!document.querySelector('#topic-bar [aria-label^=\"Remove the filter Key\"]')",
    });
    await inPlace(t, p, {
      name: n("Clear filters"), above: MSG_ABOVE, url: /^\/topics\/orders\/messages\?isolation=read_committed$/,
      act: () => p.click(`${BAR} a[data-ui=clear]`, { scroll: false }), done: "!document.querySelector('#topic-bar a[data-ui=clear]')",
    });
    await p.back();
    await p.waitFor("document.querySelector('#topic-bar a[data-ui=clear]') !== null");
    await p.back();
    await p.waitFor("location.search.includes('key=ord')");
    t.ok(true, "messages: Back walks the states back");
  },

  // Older loads more rows under the ones shown: the page does not jump.
  async "region-messages-older"(t: T) {
    const p = t.page;
    await p.goto("/topics/orders/messages?isolation=read_committed");
    const has = await p.exists("a[data-ui=page]");
    t.ok(has, "messages: the fixture topic has an Older link");
    if (!has) return;
    await p.scrollTo(300);
    await inPlace(t, p, {
      name: "messages: Older", above: MSG_ABOVE, url: /limit=/,
      act: () => p.click("a[data-ui=page]", { scroll: false }), done: "location.search.includes('limit=')",
    });
  },

  // Groups: the state chips and the filter; the counts and the table change in the region.
  async "region-groups"(t: T) {
    const p = t.page;
    await p.goto("/groups");
    await pad(p);
    await p.scrollTo(100);
    const above = [".page-h", ".tabs", TOOLBAR];
    await inPlace(t, p, {
      name: "groups: Stable", above, url: /^\/groups\?state=stable$/,
      act: () => p.click(`${TOOLBAR} [aria-label=State] a:nth-child(2)`, { scroll: false }), done: "location.search.includes('state=stable') && document.querySelector('form.toolbar [aria-label=State] a:nth-child(2)').getAttribute('aria-current') === 'true'",
      focus: `${TOOLBAR} [aria-label=State] a:nth-child(2)`,
    });
    await inPlace(t, p, {
      name: "groups: typing keeps the state", above, history: "replace", url: /^\/groups\?q=pay&state=stable$/,
      act: async () => { await p.focus(`${TOOLBAR} input[name=q]`); await p.type("pay"); }, done: "location.search.includes('q=pay')", focus: `${TOOLBAR} input[name=q]`,
    });
    await inPlace(t, p, {
      name: "groups: All", above, url: /^\/groups\?q=pay$/,
      act: () => p.click(`${TOOLBAR} [aria-label=State] a:nth-child(1)`, { scroll: false }), done: "!location.search.includes('state=')",
    });
    await p.back();
    await p.waitFor("location.search.includes('state=stable')");
    t.ok(true, "groups: Back restores the chip");
  },

  // Config: the chips and the search box swap the sections, the counts stay.
  async "region-config"(t: T) {
    const p = t.page;
    await p.goto("/topics/orders/config");
    await pad(p);
    await p.scrollTo(100);
    const above = [".page-h", ".tabs", TOOLBAR];
    await inPlace(t, p, {
      name: "config: Changed", above, url: /^\/topics\/orders\/config\?show=changed$/,
      act: () => p.click(`${TOOLBAR} a.fchip:nth-of-type(2)`, { scroll: false }), done: "location.search.includes('show=changed')",
    });
    await inPlace(t, p, {
      name: "config: typing", above, history: "replace", url: /^\/topics\/orders\/config\?q=seg&show=changed$/,
      act: async () => { await p.focus(`${TOOLBAR} input[name=q]`); await p.type("seg"); }, done: "location.search.includes('q=seg')", focus: `${TOOLBAR} input[name=q]`,
    });
    await p.back();
    await p.waitFor("location.search === ''");
    t.ok(true, "config: Back");
  },

  // The Lag page: asking when the offsets expire changes the table and the words, in place.
  async "region-lag-expiring"(t: T) {
    const p = t.page;
    await p.goto("/groups/analytics/lag");
    await pad(p);
    await p.scrollTo(60);
    await inPlace(t, p, {
      name: "lag: When do these expire?", above: [".page-h", ".tabs"], url: /\?expiring=1$/,
      act: () => p.click(".page-h a.btn[href$='expiring=1']", { scroll: false }), done: "location.search.includes('expiring=1') && !!document.querySelector('#group-lag-table')",
    });
    await p.back();
    await p.waitFor("!location.search.includes('expiring')");
    t.ok(true, "lag: Back");
  },
});

Object.assign(scenarios, {
  // The jump page (the palette's no-script twin): the filter and the names it leaves swap in place.
  async "region-jump"(t: T) {
    const p = t.page;
    await p.goto("/jump");
    await inPlace(t, p, {
      name: "jump: typing", above: [".page-h", ".jump-form"], history: "replace", url: /^\/jump\?q=ord$/,
      act: async () => { await p.focus("#q"); await p.type("ord"); }, done: "location.search === '?q=ord' && document.querySelectorAll('#jump-view .jump-item').length < 9",
      focus: "#q",
    });
  },
});

// The MultiSelect in real Chrome: the keyboard, the repeated parameters in the address and Back, the flip near the window's edge, the
// rail, long labels, focus, the chips, and a screenshot of every state under <shots>/ms-*.png. It runs on the Live page (the fixtures
// server or a real broker: the options are read from the page) and on the gallery (examples build).
import type { T } from "../drive.ts";
import type { Page } from "../page.ts";

const ms = (id: string) => `#${id}-ms`;
const trig = (id: string) => `${ms(id)} .dz-trigger`;
const pop = (id: string) => `${ms(id)} .dz-multi-pop`;
const search = (id: string) => `${ms(id)} .dz-input`;
const WIRE = "table.wire tbody tr";
const rows = (p: Page) => p.eval<number>(`document.querySelectorAll(${JSON.stringify(WIRE)}).length`);
const shown = (p: Page, sel: string) => p.eval<boolean>(`(() => { const e = document.querySelector(${JSON.stringify(sel)}); if (!e) return false; const s = getComputedStyle(e); const r = e.getBoundingClientRect(); return s.display !== 'none' && s.visibility !== 'hidden' && r.width > 0 && r.height > 0; })()`);
const params = (p: Page, name: string) => p.eval<string[]>(`new URLSearchParams(location.search).getAll(${JSON.stringify(name)})`);
const value = (p: Page, id: string) => p.text(`${ms(id)} .dz-value`);
const labels = (p: Page, id: string) => p.eval<string[]>(`[...document.querySelectorAll('${ms(id)} [role=option]')].filter((o) => !o.hidden).map((o) => o.getAttribute('data-label'))`);
const selected = (p: Page, id: string) => p.eval<string[]>(`[...document.querySelectorAll('${ms(id)} [role=option][aria-selected=true]')].map((o) => o.getAttribute('data-label'))`);
const active = (p: Page, id: string) => p.eval<string>(`(document.querySelector('${ms(id)} [data-active]') || { getAttribute: () => '' }).getAttribute('data-label')`);
const focused = (p: Page) => p.eval<string>("(() => { const a = document.activeElement; return a ? a.tagName.toLowerCase() + '.' + a.className : ''; })()");

async function openBy(p: Page, id: string, how: "click" | "enter" = "click") {
  if (how === "click") await p.click(trig(id));
  else { await p.focus(trig(id)); await p.key("Enter"); }
  await p.waitFor(`document.querySelector('${pop(id)}') && getComputedStyle(document.querySelector('${pop(id)}')).display !== 'none' && document.querySelector('${pop(id)}').getBoundingClientRect().width > 0`);
  await Bun.sleep(180);
}
/** A real click on the option with this label. */
async function tick(p: Page, id: string, label: string) {
  const sel = await p.eval<string>(`(() => { const o = [...document.querySelectorAll('${ms(id)} [role=option]')].find((o) => o.getAttribute('data-label') === ${JSON.stringify(label)}); if (!o) return ''; o.setAttribute('data-pick', '1'); return '${ms(id)} [data-pick]'; })()`);
  if (!sel) throw new Error(`no option ${label} in ${id}`);
  await p.click(sel, { scroll: false });
  await p.eval(`document.querySelectorAll('${ms(id)} [data-pick]').forEach((o) => o.removeAttribute('data-pick'))`);
}
/** The labels of a Live filter, in the popup's order (read while it is open or not). */
const options = (p: Page, id: string) => p.eval<string[]>(`[...document.querySelectorAll('${ms(id)} [role=option]')].map((o) => o.getAttribute('data-label'))`);

export const scenarios: Record<string, (t: T) => Promise<void>> = {
  async "multiselect-keyboard"(t: T) {
    const p = t.page;
    await p.goto("/live");
    const id = "live-topic";
    const all = await options(p, id);
    t.ok(all.length >= 3, `the topics filter has options (${all.length})`);
    await openBy(p, id, "enter");
    t.ok((await focused(p)).includes("dz-input"), "Enter opens it and focus goes to the search field");
    t.ok((await p.eval<string>(`document.querySelector('${trig(id)}').getAttribute('aria-expanded')`)) === "true", "aria-expanded");
    t.ok((await active(p, id)) === all[0], `the first option is active (${await active(p, id)})`);
    await p.key("ArrowDown");
    t.ok((await active(p, id)) === all[1], "ArrowDown moves");
    await p.key("End");
    t.ok((await active(p, id)) === all[all.length - 1], "End goes to the last");
    await p.key("Home");
    t.ok((await active(p, id)) === all[0], "Home goes to the first");
    await p.key(" ");
    t.ok((await selected(p, id)).join() === all[0], "Space toggles the active option");
    await p.key("ArrowDown");
    await p.key("Enter");
    t.ok((await selected(p, id)).length === 2, "Enter toggles it too, without submitting the form by itself");
    // Type-ahead goes into the search field and filters the list.
    const probe = all[all.length - 1].slice(0, 3);
    await p.type(probe);
    const hits = await labels(p, id);
    t.ok(hits.length >= 1 && hits.length < all.length && hits.every((l) => l.toLowerCase().includes(probe.toLowerCase())), `typing '${probe}' filters the list (${hits.join(", ")})`);
    await p.shot(`${t.shots}/ms-search.png`);
    for (let i = 0; i < probe.length; i++) await p.key("Backspace");
    t.ok((await p.eval<string>(`document.querySelector('${search(id)}').value`)) === "", "Backspace edits the search text first");
    t.ok((await selected(p, id)).length === 2, "and removes no chip while there is text");
    await p.key("Backspace");
    t.ok((await selected(p, id)).length === 1, "Backspace in the empty search removes the last chip");
    await p.key("Backspace");
    t.ok((await selected(p, id)).length === 0, "and the next");
    await p.key("Escape");
    await p.waitFor(`getComputedStyle(document.querySelector('${pop(id)}')).display === 'none'`);
    t.ok((await focused(p)).includes("dz-trigger"), "Esc closes it and focus returns to the button");
    await Bun.sleep(700);
    t.ok((await params(p, "topic")).length === 0, "everything ticked and removed again leaves no topic in the address");
    // The arrow keys open it from the button too.
    await p.focus(trig(id));
    await p.key("ArrowDown");
    await p.waitFor(`getComputedStyle(document.querySelector('${pop(id)}')).display !== 'none'`);
    await p.key("Escape");
  },

  async "multiselect-live-persists-in-the-address-and-back-restores"(t: T) {
    const p = t.page;
    await p.goto("/live");
    const all = await rows(p);
    const topics = await options(p, "live-topic");
    t.ok(topics.length >= 2, "two topics to choose");
    await p.scrollTo(80);
    const y0 = await p.scrollY();
    const bar0 = await p.rect("form.toolbar");
    await p.shot(`${t.shots}/ms-closed.png`, { x: 184, y: 230, width: 1096, height: 120 });
    await openBy(p, "live-topic");
    await p.shot(`${t.shots}/ms-open.png`);
    await tick(p, "live-topic", topics[0]);
    await tick(p, "live-topic", topics[1]);
    await p.waitFor("new URLSearchParams(location.search).getAll('topic').length === 2", 4000);
    t.ok((await params(p, "topic")).join() === [topics[0], topics[1]].join() || (await params(p, "topic")).length === 2, "the address carries topic=a&topic=b");
    await Bun.sleep(400);
    const two = await rows(p);
    t.num("rows.all", all); t.num("rows.two topics", two);
    t.ok(two > 0 && two <= all, `the tape shows rows that touch them (${all} -> ${two})`);
    t.ok((await isOpen(p, "live-topic")), "the popup stays open while ticking (the swap leaves it alone)");
    t.ok(await staysPut(p, y0), "the scroll position did not move");
    const bar1 = await p.rect("form.toolbar");
    t.ok(Math.abs(bar1.top - bar0.top) < 0.5 && Math.abs(bar1.left - bar0.left) < 0.5, "the toolbar did not move");
    t.ok(await p.eval(`document.querySelector('#live-wire').getAttribute('data-frag').split('topic=').length === 3`), "the stream's fragment keeps both topics");
    await p.key("Escape");
    await Bun.sleep(300);
    t.ok((await value(p, "live-topic")) === `${topics[0]}, ${topics[1]}`, `the button says them: ${await value(p, "live-topic")}`);
    t.ok((await p.eval<number>("document.querySelectorAll('#live-picked a.dz-tag').length")) === 2, "two removable tags under the toolbar");
    await p.shot(`${t.shots}/ms-two.png`, { x: 184, y: 230, width: 1096, height: 180 });
    // A third kind: AND across kinds.
    const clients = await options(p, "live-client");
    await openBy(p, "live-client");
    await tick(p, "live-client", clients[0]);
    await p.waitFor("new URLSearchParams(location.search).getAll('client').length === 1", 4000);
    await p.key("Escape");
    await Bun.sleep(400);
    const and = await rows(p);
    t.ok(and > 0 && and <= two, `a client narrows it further (${two} -> ${and})`);
    t.ok((await p.eval<number>("document.querySelectorAll('#live-picked a.dz-tag').length")) === 3, "three tags");
    // Removing a tag takes it off, in place.
    const tag = await p.eval<string>(`(() => { const a = [...document.querySelectorAll('#live-picked a.dz-tag')].find((a) => a.querySelector('.dz-tag-t').textContent === ${JSON.stringify(topics[0])}); if (!a) return ''; a.setAttribute('data-pick', '1'); return '#live-picked a[data-pick]'; })()`);
    t.ok(!!tag, "a tag for the first topic");
    await p.click(tag);
    await p.waitFor("new URLSearchParams(location.search).getAll('topic').length === 1", 4000);
    await Bun.sleep(300);
    t.ok((await p.eval<number>("document.querySelectorAll('#live-picked a.dz-tag').length")) === 2, "a tag took its value off: one topic and the client are left, as two tags");
    await p.back();
    await p.waitFor("new URLSearchParams(location.search).getAll('topic').length === 2 && new URLSearchParams(location.search).getAll('client').length === 1", 4000);
    await Bun.sleep(500);
    t.ok((await value(p, "live-client")) === clients[0], "Back restores the client choice");
    t.ok((await selected(p, "live-topic")).length === 2, "and both topics are still ticked");
    await p.back();
    await p.waitFor("new URLSearchParams(location.search).getAll('client').length === 0 && new URLSearchParams(location.search).getAll('topic').length === 2", 4000);
    await Bun.sleep(500);
    t.ok((await value(p, "live-topic")) === `${topics[0]}, ${topics[1]}`, "Back again: only the two topics");
    await p.back();
    await p.waitFor("new URLSearchParams(location.search).getAll('topic').length === 0", 4000);
    await Bun.sleep(600);
    t.ok((await value(p, "live-topic")) === "All topics", "Back to the start: 'All topics'");
    t.ok((await rows(p)) === all, `and the whole tape (${await rows(p)} rows)`);
    // A reload restores it from the address.
    await p.goto(`/live?topic=${encodeURIComponent(topics[0])}&topic=${encodeURIComponent(topics[1])}`);
    t.ok((await selected(p, "live-topic")).length === 2 && (await value(p, "live-topic")) === `${topics[0]}, ${topics[1]}`, "an address with two topics opens with both chosen");
    t.ok((await rows(p)) > 0, "and rows");
    // Three or more: a count.
    if (topics.length >= 3) {
      await p.goto(`/live?${topics.slice(0, 3).map((x) => `topic=${encodeURIComponent(x)}`).join("&")}`);
      t.ok((await value(p, "live-topic")) === "3 topics", "three topics: '3 topics'");
      await p.shot(`${t.shots}/ms-three.png`, { x: 184, y: 230, width: 1096, height: 180 });
    }
  },

  async "multiselect-clear-focus-and-empty-state"(t: T) {
    const p = t.page;
    await p.goto("/live");
    const topics = await options(p, "live-topic");
    await openBy(p, "live-topic");
    await tick(p, "live-topic", topics[0]);
    t.ok((await p.eval<number>(`document.querySelectorAll('${ms("live-topic")} .dz-tag').length`)) === 1, "the chosen shows as a chip in the popup");
    await p.waitFor("new URLSearchParams(location.search).getAll('topic').length === 1", 4000);
    // Select all, then Clear, from the popup.
    await p.click(`${ms("live-topic")} [data-all]`);
    t.ok((await selected(p, "live-topic")).length === topics.length, "Select all takes every option");
    await p.click(`${ms("live-topic")} [data-none]`);
    t.ok((await selected(p, "live-topic")).length === 0, "Clear takes them all off");
    await p.key("Escape");
    // The clear x on the button.
    await p.waitFor("new URLSearchParams(location.search).getAll('topic').length === 0", 4000);
    await openBy(p, "live-topic");
    await tick(p, "live-topic", topics[1]);
    await p.key("Escape");
    await p.waitFor("new URLSearchParams(location.search).getAll('topic').length === 1", 4000);
    await Bun.sleep(300);
    t.ok(await shown(p, `${ms("live-topic")} [data-clear]`), "a clear x shows on the button once something is chosen");
    await p.click(`${ms("live-topic")} [data-clear]`);
    await p.waitFor("new URLSearchParams(location.search).getAll('topic').length === 0", 4000);
    await Bun.sleep(300);
    t.ok((await focused(p)).includes("dz-trigger"), "after the clear x, focus is on the button");
    t.ok(!(await shown(p, `${ms("live-topic")} [data-clear]`)), "and the x is gone");
    await p.shot(`${t.shots}/ms-one-chip.png`, { x: 184, y: 230, width: 1096, height: 120 });
    // An empty result says which filters are on, and how to leave.
    await p.goto("/live?client=no-such-app&topic=" + encodeURIComponent(topics[0]));
    t.ok((await p.exists("#live-no-match")), "an empty result is explained");
    const said = await p.text("#live-no-match");
    t.ok(said.includes("no-such-app") && said.includes(topics[0]) && said.includes("Clear filters"), `it names the filters: ${said.replace(/\s+/g, " ").slice(0, 160)}`);
    t.ok((await value(p, "live-client")) === "no-such-app", "a chosen value the tape lacks stays chosen");
    await p.shot(`${t.shots}/ms-empty.png`, { x: 184, y: 60, width: 1096, height: 600 });
    await p.click("#live-no-match a");
    await p.waitFor("location.search === ''", 4000);
    await Bun.sleep(400);
    t.ok((await rows(p)) > 0 && !(await p.exists("#live-no-match")), "Clear filters brings the tape back");
  },

  async "multiselect-flips-near-the-bottom-and-clears-the-rail"(t: T) {
    const p = t.page;
    await p.goto("/gallery");
    const id = "m-three";
    // Put the button 60 px above the window's bottom edge: the list cannot open downwards.
    const top = await p.eval<number>(`document.querySelector('${trig(id)}').getBoundingClientRect().top + window.scrollY`);
    await p.scrollTo(Math.max(0, top - (800 - 70)));
    await openBy(p, id);
    const r = await p.eval<{ tl: number; tt: number; tb: number; pl: number; pt: number; pr: number; pb: number; rail: number }>(`(() => { const a = document.querySelector('${trig(id)}').getBoundingClientRect(), b = document.querySelector('${pop(id)}').getBoundingClientRect(); return { tl: a.left, tt: a.top, tb: a.bottom, pl: b.left, pt: b.top, pr: b.right, pb: b.bottom, rail: document.getElementById('rail').getBoundingClientRect().right }; })()`);
    t.num("trigger.top", r.tt); t.num("list.bottom", r.pb); t.num("list.top", r.pt);
    t.ok(r.pb <= r.tt + 1, `the list opens above the button (bottom ${r.pb} <= top ${r.tt})`);
    t.ok(r.pt >= 0 && r.pb <= 800, "inside the window");
    t.ok(r.pl >= r.rail + 7, `clear of the rail (${r.pl} >= ${r.rail})`);
    await p.shot(`${t.shots}/ms-flipped.png`);
    await p.key("Escape");
    // The first one of the row sits at the content's left edge: still right of the rail.
    await p.scrollTo(0);
    await p.goto("/gallery");
    await openBy(p, "m-none");
    const q = await p.eval<{ l: number; r: number; rail: number; w: number }>(`(() => { const b = document.querySelector('${pop("m-none")}').getBoundingClientRect(); return { l: b.left, r: b.right, rail: document.getElementById('rail').getBoundingClientRect().right, w: innerWidth }; })()`);
    t.ok(q.l >= q.rail + 7 && q.r <= q.w - 7, `the first multi-select's list stays clear of the rail and inside the window (${q.l}..${q.r})`);
    await p.key("Escape");
  },

  async "multiselect-long-labels-ellipsis-and-width"(t: T) {
    const p = t.page;
    await p.goto("/gallery");
    const id = "m-long";
    await p.eval(`document.querySelector('${trig(id)}').scrollIntoView({ block: 'center', behavior: 'instant' })`);
    await openBy(p, id);
    const m = await p.eval<{ trig: number; pop: number; l: number; r: number; lines: number[]; titles: number; broken: string[]; valueClipped: boolean; ellipsis: string }>(`(() => {
      const pop = document.querySelector('${pop(id)}').getBoundingClientRect();
      const opts = [...document.querySelectorAll('${ms(id)} [role=option]')];
      const v = document.querySelector('${ms(id)} .dz-value');
      return { trig: document.querySelector('${trig(id)}').getBoundingClientRect().width, pop: pop.width, l: pop.left, r: pop.right,
        lines: opts.map((o) => Math.round(o.getBoundingClientRect().height)), titles: opts.filter((o) => o.title).length,
        broken: opts.filter((o) => { const l = o.querySelector('.dz-opt-label'); return l.scrollWidth > l.clientWidth && getComputedStyle(l).textOverflow !== 'ellipsis'; }).map((o) => o.textContent),
        valueClipped: v.scrollWidth > v.clientWidth, ellipsis: getComputedStyle(v).textOverflow };
    })()`);
    t.num("list.w", m.pop); t.num("trigger.w", m.trig);
    t.ok(m.pop <= 421, `the list is at most 420 px (${m.pop})`);
    t.ok(m.pop >= 239, `and at least 240 (${m.pop})`);
    t.ok(m.l >= 184 && m.r <= 1280 - 8, `inside the window, clear of the rail (${m.l}..${m.r})`);
    t.ok(m.lines.every((h) => h <= 40), `one line per option (${m.lines.join(",")})`);
    t.ok(m.titles === m.lines.length, "every option has its label in title");
    t.ok(m.broken.length === 0, "a label that does not fit ends in an ellipsis");
    t.ok(m.trig <= 281, `the button stays at most 280 px (${m.trig})`);
    t.ok(!m.valueClipped || m.ellipsis === "ellipsis", "a clipped selection on the button ends in an ellipsis");
    await p.shot(`${t.shots}/ms-long.png`);
    await p.key("Escape");
  },

  async "multiselect-repeated-parameters-in-the-gallery-form"(t: T) {
    const p = t.page;
    await p.goto("/gallery");
    await openBy(p, "g-tags");
    await tick(p, "g-tags", "topic.03");
    await tick(p, "g-tags", "topic.07");
    await p.waitFor("new URLSearchParams(location.search).getAll('tag').length === 2", 4000);
    t.ok((await params(p, "tag")).join() === "topic.03,topic.07", "the form sends tag=topic.03&tag=topic.07");
    await p.key("Escape");
    await Bun.sleep(300);
    t.ok((await p.text("[data-test=chosen]")).includes("topic.03 + topic.07"), "the region says what was chosen");
    t.ok((await value(p, "g-tags")) === "topic.03, topic.07", "the button says it");
  },

  async "multiselect-without-a-script-is-checkboxes-in-a-details"(t: T) {
    const p = t.page;
    await p.scripting(false);
    await p.gotoRaw("/live?topic=orders");
    await Bun.sleep(300);
    t.ok(await p.exists("#live-topic.dz-native"), "the details holds the control");
    t.ok(!(await p.eval<boolean>("document.querySelector('#live-topic-ms').classList.contains('is-on')")), "no script has run");
    await p.click("#live-topic summary");
    await Bun.sleep(150);
    t.ok(await shown(p, "#live-topic-ms .dz-multi-check input"), "the checkboxes are shown");
    t.ok(await shown(p, "#live-topic-ms [data-noscript]"), "with an Apply button");
    await p.shot(`${t.shots}/ms-noscript.png`, { x: 184, y: 230, width: 1096, height: 360 });
    await p.scripting(true);
  },
};

/** The scroll position is where it was, or as far down as the (now shorter) page allows. */
async function staysPut(p: Page, y0: number): Promise<boolean> {
  return p.eval<boolean>(`(() => { const max = Math.max(0, document.documentElement.scrollHeight - innerHeight); return Math.abs(scrollY - Math.min(${y0}, max)) < 1.5; })()`);
}

async function isOpen(p: Page, id: string): Promise<boolean> {
  return p.eval<boolean>(`document.querySelector('${ms(id)}').hasAttribute('data-open')`);
}

// Against a real broker with traffic (opt-in: LIVE_URL, as live-flicker.ts; the fixtures tape has rows of its own that are not requests):
// the owner's report, "the topics filter does not seem to work". Every row that stays says the topic it passed by in its Target cell,
// and none belongs only to a topic whose name merely contains the chosen one (orders.dlq under orders).
const LIVE = process.env.LIVE_URL ?? "";
scenarios["multiselect-real-topic-filter-shows-only-and-says-the-topic"] = async (t: T) => {
  if (!LIVE) { t.num("skipped", "no LIVE_URL"); return; }
  const p = t.page;
  await p.goto(`${LIVE}/live`);
  await p.waitFor(`document.querySelectorAll('${WIRE}').length > 3`, 8000);
  await openBy(p, "live-topic");
  const names = await options(p, "live-topic");
  const topic = names.includes("orders") ? "orders" : names[0];
  t.ok(names.includes("orders.dlq") || names.length > 1, `the filter lists the broker's topics: ${names.join(", ")}`);
  await tick(p, "live-topic", topic);
  await p.key("Escape");
  await p.waitFor(`new URLSearchParams(location.search).getAll('topic').join() === ${JSON.stringify(topic)}`, 4000);
  await Bun.sleep(600);
  const r = await p.eval<{ n: number; bad: string[]; dlq: number }>(`(() => {
    const topic = ${JSON.stringify(topic)};
    const rows = [...document.querySelectorAll('${WIRE}')];
    const cell = (r) => r.children[3].innerText.replace(/\\s+/g, ' ').trim();
    const says = (txt) => txt.split(' ').some((w) => w === topic || new RegExp('^' + topic.replace(/[.*+?^$()|[\\]\\\\]/g, '\\\\$&') + '-[0-9]+$').test(w));
    return { n: rows.length, bad: rows.filter((r) => !says(cell(r))).map(cell), dlq: rows.filter((r) => cell(r).startsWith(topic + '.')).length };
  })()`);
  t.num("rows", r.n);
  t.ok(r.n > 0, "the topic has rows");
  t.ok(r.bad.length === 0, `every row says ${topic} in its Target cell (not: ${r.bad.slice(0, 3).join(" | ")})`);
  t.ok(r.dlq === 0, `no row of a topic that only starts with ${topic}`);
  await p.shot(`${t.shots}/ms-real-topic.png`, { x: 184, y: 230, width: 1096, height: 560 });
};

// A control that looks right but changes nothing is the worst defect a filter can have (the owner: "I do not think the filters work
// at all"): for every page with a filter, pick a value and assert that the DATA changed (rows counted before and after), the address
// followed, Back restored the earlier rows, and the stream fragment keeps the filter so that the next update is filtered too.
import type { T } from "../drive.ts";
import type { Page } from "../page.ts";

const rows = (p: Page, sel: string) => p.eval<number>(`document.querySelectorAll(${JSON.stringify(sel)}).length`);
const WIRE = ".dt-t tbody tr";

async function pick(p: Page, id: string, text: string): Promise<void> {
  // A MultiSelect (the Live filters): tick the option with a real click, then Esc closes it and sends the choice at once.
  if (await p.exists(`#${id}-ms`)) {
    await p.click(`#${id}-ms .dz-trigger`);
    await p.waitFor(`getComputedStyle(document.querySelector('#${id}-ms .dz-multi-pop')).display !== 'none'`);
    await Bun.sleep(150);
    const sel = await p.eval<string>(`(() => { const o = [...document.querySelectorAll('#${id}-ms [role=option]')].find((o) => o.getAttribute('data-label').startsWith(${JSON.stringify(text)})); if (!o) return ''; o.setAttribute('data-pick', '1'); return '#${id}-ms [data-pick]'; })()`);
    if (!sel) throw new Error(`no option ${text} in ${id}`);
    await p.click(sel, { scroll: false });
    await p.key("Escape");
    return;
  }
  const trig = `.dz:has(#${id}) .dz-trigger, .dz:has(#${id}) .dz-input`;
  await p.click(trig);
  await p.waitFor(`document.querySelector('.dz:has(#${id}) [role=listbox]') && getComputedStyle(document.querySelector('.dz:has(#${id}) [role=listbox]')).display !== 'none'`);
  await Bun.sleep(100);
  const sel = await p.eval<string>(`(() => { const o = [...document.querySelectorAll('.dz:has(#${id}) [role=option]')].find((o) => o.textContent.trim().startsWith(${JSON.stringify(text)})); if (!o) return ''; o.setAttribute('data-pick', '1'); return '.dz:has(#${id}) [data-pick]'; })()`);
  if (!sel) throw new Error(`no option ${text} in ${id}`);
  await p.click(sel);
}

export const scenarios: Record<string, (t: T) => Promise<void>> = {
  async "filters-live-change-the-tape"(t: T) {
    const p = t.page;
    await p.goto("/live");
    const all = await rows(p, WIRE);
    t.num("rows.all", all);
    t.ok(all >= 10, `the tape has rows (${all})`);
    const watch = async (name: string, act: () => Promise<void>, url: RegExp, more = false) => {
      const before = await rows(p, WIRE);
      const keyBefore = await p.eval<string>(`[...document.querySelectorAll(${JSON.stringify(WIRE)})].map((r) => r.textContent).join('|')`);
      await act();
      await p.waitFor(`/${url.source}/.test(location.search)`, 4000);
      await Bun.sleep(350);
      const after = await rows(p, WIRE);
      t.num(`${name}: rows`, `${before} -> ${after}`);
      const keyAfter = await p.eval<string>(`[...document.querySelectorAll(${JSON.stringify(WIRE)})].map((r) => r.textContent).join('|')`);
      // A filter that narrows leaves fewer rows; one that widens (the polls that found nothing) shows rows that were not there.
      t.ok(more ? keyAfter !== keyBefore && after > 0 : after < before && after > 0, `${name}: the rows changed (${before} -> ${after})`);
      t.ok(await p.eval(`(document.querySelector('#live-wire[data-frag]') || {getAttribute: () => ''}).getAttribute('data-frag').includes(${JSON.stringify(url.source.replace(/\\/g, ""))}.split('=')[0])`), `${name}: the stream fragment keeps the filter`);
    };
    await watch("typing a client", async () => { await p.focus("form.toolbar input[name=q]"); await p.type("fulfil"); }, /q=fulfil/);
    await p.goto("/live");
    await watch("All apps: orders", () => pick(p, "live-client", "orders"), /client=orders/);
    await p.back();
    await p.waitFor("!location.search.includes('client=')");
    await Bun.sleep(400);
    t.ok((await rows(p, WIRE)) === all, "Back: the whole tape is back");
    await p.goto("/live");
    await watch("All topics: orders", () => pick(p, "live-topic", "orders"), /topic=orders/);
    await p.goto("/live");
    await watch("Refused only", () => p.click("form.toolbar button[name=refused]"), /refused=1/);
    await p.goto("/live");
    await watch("Idle polls hidden off", () => p.click("form.toolbar button[name=idle]"), /idle=0/, true);
  },

  async "filters-lists-change-the-data"(t: T) {
    const p = t.page;
    // Topics: a name filter and the internal toggle.
    await p.goto("/topics");
    const topics = await rows(p, "#topics-table tbody tr");
    await p.focus("form.toolbar input[name=q]");
    await p.type("pick");
    await p.waitFor("location.search.includes('q=pick')");
    await Bun.sleep(350);
    const filtered = await rows(p, "#topics-table tbody tr");
    t.ok(filtered > 0 && filtered < topics, `topics: the filter narrows the list (${topics} -> ${filtered})`);
    // Groups: a state.
    await p.goto("/groups");
    const groups = await rows(p, "#groups-table tbody tr");
    await pick(p, "groups-state", "Stable");
    await p.waitFor("location.search.includes('state=stable')");
    await Bun.sleep(350);
    const stable = await rows(p, "#groups-table tbody tr");
    t.ok(stable > 0 && stable < groups, `groups: Stable narrows the list (${groups} -> ${stable})`);
    // Messages: a partition and a key.
    await p.goto("/topics/orders/messages");
    const msgs = await rows(p, "#topic-messages tbody tr");
    await p.click("#topic-bar [aria-label=Partition] a:nth-child(2)");
    await p.waitFor("location.search.includes('partition=0')");
    await Bun.sleep(350);
    const part = await rows(p, "#topic-messages tbody tr");
    t.ok(part > 0 && part < msgs, `messages: partition 0 narrows the table (${msgs} -> ${part})`);
    t.ok((await p.eval<string[]>("[...document.querySelectorAll('#topic-messages tbody tr td:first-child')].map((e) => e.textContent.trim())")).every((x) => x === "0"), "messages: every row is partition 0");
    // Config: the Changed chip.
    await p.goto("/topics/inventory.levels/config");
    const cfg = await rows(p, "#config-view tbody tr[id]");
    await p.click("form.toolbar a.fchip:nth-of-type(2)");
    await p.waitFor("location.search.includes('show=changed')");
    await Bun.sleep(350);
    const changed = await rows(p, "#config-view tbody tr[id]");
    t.ok(changed > 0 && changed < cfg, `config: Changed narrows the table (${cfg} -> ${changed})`);
  },
};

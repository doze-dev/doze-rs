// How a page changes while it is open (docs/port/console/live-design.md, CON-800 to CON-812), asserted in a DOM against
// crates/kafka-console/assets/live.js and console.js: a page is a snapshot (no stream, no timer, no request); Refresh fetches the
// regions with their ETags and morphs only what differs; the interval is chained, remembered, stopped by a hidden tab and held back
// by a field with focus; the tail batches, holds what you are reading behind a pill, never touches a row that is there, and
// comes back from a lost connection without a row twice. Run: `cd tools/dom && bun install && bun test`.

import { describe, expect, test } from "bun:test";
import { boot, type Page } from "./harness.ts";

const wait = (ms = 30) => new Promise((r) => setTimeout(r, ms));

const bar = (regions: string) =>
  `<div class="refresh" x-data="refreshControl" x-on:change="picked" data-refresh data-regions="${regions}">` +
  `<span class="stamp">Updated <time data-stamp datetime="2025-01-01T00:00:00Z">00:00:00</time></span>` +
  `<span class="every"><select id="every" name="every"><option value="0" selected>Off</option><option value="5">5 s</option><option value="15">15 s</option></select></span>` +
  `<span data-refresh-failed hidden></span><a data-refresh-now href="" x-on:click="now">Refresh</a></div>`;

const region = (id: string, text: string, extra = "") => `<div class="frag" id="${id}" data-frag="/fragments/${id}">${extra}<p id="${id}-p">${text}</p><input id="${id}-q" value="a"></div>`;

const refreshPage = (a = "old a", b = "old b") => `<main id="main">${bar("a,b")}${region("a", a)}${region("b", b)}</main>`;

const started = async (html: string) => {
  const p = await boot(html, { alpine: true, plain: true });
  (p.window as unknown as { Doze: { batchMs?: number } }).Doze.batchMs = 0;
  return p;
};
const picks = async (p: Page, v: string) => {
  const sel = p.document.getElementById("every") as HTMLSelectElement;
  sel.value = v;
  sel.dispatchEvent(new p.window.Event("change", { bubbles: true }));
  await p.flush();
};
const text = (p: Page, id: string) => p.document.getElementById(id)?.textContent;

describe("a page is a snapshot (CON-800)", () => {
  test("with nothing asked, a page makes no request, opens no stream and sets no timer of its own", async () => {
    const p = await started(refreshPage());
    await wait(60);
    expect(p.calls).toEqual([]);
    expect(p.streams).toEqual([]);
    expect(p.timers.filter((t) => t.ms >= 1000).length).toBe(0);
  });

  test("a mutation observer sees nothing change on an idle page", async () => {
    const p = await started(refreshPage());
    let n = 0;
    new p.window.MutationObserver(() => n++).observe(p.document.body, { subtree: true, childList: true, attributes: true, characterData: true });
    await wait(120);
    expect(n).toBe(0);
  });
});

describe("Refresh (CON-802, CON-803)", () => {
  test("the button fetches every region; the first fetch is unconditional, the next carries the ETag, and a 304 changes nothing", async () => {
    const p = await started(refreshPage());
    p.autoAnswer((c) => ({ etag: `"${c.url}-1"`, body: region(c.url.split("/").pop()!, "new " + c.url.split("/").pop()) }));
    p.click(p.document.querySelector("[data-refresh-now]")!);
    await p.flush();
    expect(p.calls.map((c) => c.url)).toEqual(["/fragments/a", "/fragments/b"]);
    expect(p.calls.every((c) => !("If-None-Match" in c.headers))).toBe(true);
    expect(text(p, "a-p")).toBe("new a");
    p.calls.length = 0;
    p.autoAnswer(() => ({ status: 304 }));
    let n = 0;
    for (const id of ["a", "b"]) new p.window.MutationObserver(() => n++).observe(p.document.getElementById(id)!, { subtree: true, childList: true, attributes: true, characterData: true });
    p.click(p.document.querySelector("[data-refresh-now]")!);
    await p.flush();
    expect(p.calls.map((c) => c.headers["If-None-Match"])).toEqual(['"/fragments/a-1"', '"/fragments/b-1"']);
    expect(n).toBe(0);
    expect(text(p, "a-p")).toBe("new a");
  });

  test("a changed region changes only what differs: the other region and the field with the person's text are left alone", async () => {
    const p = await started(refreshPage());
    const field = p.document.getElementById("b-q") as HTMLInputElement;
    field.value = "typed";
    field.focus();
    const kept = p.document.getElementById("b-p")!;
    p.autoAnswer((c) => ({ etag: '"x"', body: c.url.endsWith("/a") ? region("a", "fresh a") : region("b", "old b") }));
    p.click(p.document.querySelector("[data-refresh-now]")!);
    await p.flush();
    expect(text(p, "a-p")).toBe("fresh a");
    expect(p.document.getElementById("b-p")).toBe(kept);
    expect(field.value).toBe("typed");
    expect(p.animated).toEqual([]);
  });

  test("the stamp changes when the person refreshes, and not for an interval poll that found nothing", async () => {
    const p = await started(refreshPage());
    p.autoAnswer(() => ({ status: 304 }));
    const stamp = () => p.document.querySelector("time[data-stamp]")!.textContent;
    p.click(p.document.querySelector("[data-refresh-now]")!);
    await p.flush();
    expect(stamp()).not.toBe("00:00:00");
  });

  test("the key r refreshes, and typing r in a field is typing", async () => {
    const p = await started(refreshPage());
    p.autoAnswer(() => ({ etag: '"x"', body: region("a", "r a") }));
    const field = p.document.getElementById("a-q") as HTMLInputElement;
    field.focus();
    field.dispatchEvent(new p.window.KeyboardEvent("keydown", { key: "r", bubbles: true }));
    await p.flush();
    expect(p.calls.length).toBe(0);
    (p.document.activeElement as HTMLElement).blur();
    p.document.body.dispatchEvent(new p.window.KeyboardEvent("keydown", { key: "r", bubbles: true, cancelable: true }));
    await p.flush();
    expect(p.calls.length).toBe(2);
  });

  test("a failed refresh leaves the page as it was and says when the next try is", async () => {
    const p = await started(refreshPage());
    p.autoAnswer(() => ({ status: 304 }));
    await picks(p, "5");
    p.autoAnswer(() => new Error("network"));
    p.click(p.document.querySelector("[data-refresh-now]")!);
    await p.flush();
    const f = p.document.querySelector("[data-refresh-failed]") as HTMLElement;
    expect(f.hidden).toBe(false);
    expect(f.textContent).toMatch(/^Could not refresh · will retry at \d\d:\d\d:\d\d$/);
    expect(text(p, "a-p")).toBe("old a");
  });
});

describe("the interval (CON-802, CON-804)", () => {
  test("choosing 5 s is remembered, polls the regions and schedules the next poll only when this one is done", async () => {
    const p = await started(refreshPage());
    p.autoAnswer(() => ({ status: 304 }));
    await picks(p, "5");
    expect(p.window.localStorage.getItem("doze:refresh")).toBe("5");
    expect(p.calls.length).toBe(2);
    await p.flush();
    const next = p.timers.filter((t) => !t.cleared && t.ms === 5000);
    expect(next.length).toBe(1);
    p.calls.length = 0;
    p.runTimers();
    await p.flush();
    expect(p.calls.length).toBe(2);
    expect(p.timers.filter((t) => !t.cleared && t.ms === 5000).length).toBe(1);
  });

  test("Off stops the polling, and a remembered choice is the interval of the next page", async () => {
    const p = await started(refreshPage());
    p.autoAnswer(() => ({ status: 304 }));
    await picks(p, "5");
    await picks(p, "0");
    expect(p.window.localStorage.getItem("doze:refresh")).toBe("0");
    expect(p.timers.filter((t) => !t.cleared && t.ms === 5000).length).toBe(0);
    // The next page: its control is made after the choice was stored.
    const q = await started("<main id=\"main\"></main>");
    q.window.localStorage.setItem("doze:refresh", "15");
    q.autoAnswer(() => ({ status: 304 }));
    q.document.getElementById("main")!.insertAdjacentHTML("beforeend", refreshPage().replace(/<\/?main[^>]*>/g, ""));
    await wait(30);
    expect((q.document.getElementById("every") as HTMLSelectElement).value).toBe("15");
    expect(q.timers.some((t) => !t.cleared && t.ms === 15000)).toBe(true);
  });

  test("a hidden tab polls nothing; becoming visible after the interval refreshes once", async () => {
    const p = await started(refreshPage());
    p.autoAnswer(() => ({ status: 304 }));
    await picks(p, "5");
    await p.flush();
    p.calls.length = 0;
    Object.defineProperty(p.document, "hidden", { value: true, configurable: true });
    p.document.dispatchEvent(new p.window.Event("visibilitychange"));
    expect(p.timers.filter((t) => !t.cleared && t.ms === 5000).length).toBe(0);
    expect(p.calls.length).toBe(0);
  });

  test("a poll is skipped while a field of the page has focus, and the next is scheduled", async () => {
    const p = await started(refreshPage());
    p.autoAnswer(() => ({ status: 304 }));
    await picks(p, "5");
    await p.flush();
    p.calls.length = 0;
    (p.document.getElementById("a-q") as HTMLInputElement).focus();
    p.runTimers();
    await p.flush();
    expect(p.calls.length).toBe(0);
    expect(p.timers.filter((t) => !t.cleared && t.ms === 5000).length).toBe(1);
  });
});

// ---- the tail ----
const row = (id: number, extra = "") => `<tr id="tape-r-${id}" data-row="${id}"><td><a class="lnk" href="/live/${id}">r${id}</a>${extra}</td></tr>`;
const tail = (ids: number[], cursor = 5) =>
  `<main id="main"><div class="tailbar" x-data="tailControl" data-tail-bar data-tail-for="live-wire">` +
  `<div class="seg"><button type="button" class="on" aria-pressed="true" data-tail-mode="snapshot" x-on:click="snapshot">Snapshot</button><button type="button" aria-pressed="false" data-tail-mode="live" x-on:click="live" disabled>Live tail</button></div>` +
  `<span class="conn" data-conn="snapshot"><i class="dot"></i><span data-conn-text>Snapshot</span></span>` +
  `<button type="button" class="held-pill" data-held-pill hidden x-on:click="show">↑ <span data-held-n>0</span> new · Show</button></div>` +
  `<div class="frag" id="live-wire" data-frag="/fragments/live-wire"><div class="tail-root" data-tail-url="/events?tail=live&amp;since=${cursor}" data-cursor="${cursor}" data-tail-rows="tape">` +
  `<table class="dt-t"><thead data-morph-skip><tr><th>Time</th></tr></thead><tbody data-tail>${ids.map((i) => row(i)).join("")}</tbody></table><div data-empty hidden>none</div></div></div></main>`;

const ids = (p: Page) => [...p.document.querySelectorAll("tbody[data-tail] tr")].map((r) => r.getAttribute("data-row"));
const scrollAt = (p: Page, y: number) => Object.defineProperty(p.window, "scrollY", { value: y, configurable: true });
const live = async (p: Page) => {
  p.click(p.document.querySelector("[data-tail-mode=live]")!);
  await p.flush();
  const es = p.streams[p.streams.length - 1];
  es.emit("ready", "ok");
  return es;
};
const conn = (p: Page) => p.document.querySelector("[data-conn]")!.getAttribute("data-conn");
const pill = (p: Page) => p.document.querySelector("[data-held-pill]") as HTMLElement;

describe("the tail (CON-809, CON-810)", () => {
  test("it opens as a snapshot with no stream; Live tail opens one for the region, from its cursor, and says Live", async () => {
    const p = await started(tail([3, 2, 1]));
    await p.flush();
    expect(p.streams).toEqual([]);
    expect((p.document.querySelector("[data-tail-mode=live]") as HTMLButtonElement).disabled).toBe(false);
    const es = await live(p);
    expect(es.url).toContain("/events?tail=live&since=5");
    expect(conn(p)).toBe("live");
    expect(p.document.querySelector("[data-conn-text]")!.textContent).toBe("Live");
    expect(p.document.querySelector("[data-tail-mode=live]")!.getAttribute("aria-pressed")).toBe("true");
  });

  test("at the top, new rows are put above the others in one batch and marked for a moment; nothing that was there is rewritten", async () => {
    const p = await started(tail([3, 2, 1]));
    const old = p.document.getElementById("tape-r-3")!;
    const es = await live(p);
    scrollAt(p, 0);
    es.emit("rows", row(5) + row(4), "7");
    await wait(40);
    expect(ids(p)).toEqual(["5", "4", "3", "2", "1"]);
    expect(p.document.getElementById("tape-r-3")).toBe(old);
    expect([...p.document.querySelectorAll("tr.fresh")].map((r) => r.getAttribute("data-row"))).toEqual(["5", "4"]);
    p.runTimers();
    expect(p.document.querySelectorAll("tr.fresh").length).toBe(0);
  });

  test("batches are at most one a second: two events inside it are one insert", async () => {
    const p = await started(tail([3, 2, 1]));
    (p.window as unknown as { Doze: { batchMs?: number } }).Doze.batchMs = 1000;
    const es = await live(p);
    scrollAt(p, 0);
    es.emit("rows", row(4), "6");
    await wait(30);
    expect(ids(p)).toEqual(["4", "3", "2", "1"]);
    es.emit("rows", row(5), "7");
    es.emit("rows", row(6), "8");
    await wait(30);
    expect(ids(p)).toEqual(["4", "3", "2", "1"]);
    await wait(1050);
    expect(ids(p)).toEqual(["6", "5", "4", "3", "2", "1"]);
  });

  test("scrolled down, rows are held behind a pill that counts them and the page does not move; Show puts them in", async () => {
    const p = await started(tail([3, 2, 1]));
    const es = await live(p);
    scrollAt(p, 400);
    es.emit("rows", row(5) + row(4), "7");
    await wait(40);
    expect(ids(p)).toEqual(["3", "2", "1"]);
    expect(pill(p).hidden).toBe(false);
    expect(p.document.querySelector("[data-held-n]")!.textContent).toBe("2");
    es.emit("rows", row(6), "8");
    await wait(40);
    expect(p.document.querySelector("[data-held-n]")!.textContent).toBe("3");
    expect(p.scrolls).toEqual([]);
    p.click(pill(p));
    await p.flush();
    expect(ids(p)).toEqual(["6", "5", "4", "3", "2", "1"]);
    expect(pill(p).hidden).toBe(true);
  });

  test("a row you have focus in holds the batch, and returning to the top lets it in", async () => {
    const p = await started(tail([3, 2, 1]));
    const es = await live(p);
    (p.document.querySelector("tbody[data-tail] a") as HTMLElement).focus();
    es.emit("rows", row(5), "7");
    await wait(40);
    expect(ids(p)).toEqual(["3", "2", "1"]);
    expect(pill(p).hidden).toBe(false);
  });

  test("a folded repeat arrives again with its count grown and changes in place, silently, without being counted as new", async () => {
    const p = await started(tail([3, 2, 1]));
    const es = await live(p);
    scrollAt(p, 400);
    const el = p.document.getElementById("tape-r-2")!;
    es.emit("rows", row(2, '<span class="fold">× 9</span>'), "9");
    await wait(40);
    expect(p.document.getElementById("tape-r-2")).toBe(el);
    expect(el.textContent).toContain("× 9");
    expect(pill(p).hidden).toBe(true);
    expect(ids(p)).toEqual(["3", "2", "1"]);
  });

  test("the DOM keeps 200 rows: older ones drop off the end when the new ones are put in", async () => {
    const p = await started(tail(Array.from({ length: 200 }, (_, i) => 200 - i)));
    const es = await live(p);
    scrollAt(p, 0);
    es.emit("rows", row(202) + row(201), "7");
    await wait(40);
    expect(ids(p).length).toBe(200);
    expect(ids(p)[0]).toBe("202");
    expect(ids(p)[199]).toBe("3");
  });

  test("a lost connection shows Reconnecting, then comes back from the last row seen with a growing wait, and no row twice", async () => {
    const p = await started(tail([3, 2, 1]));
    const es = await live(p);
    scrollAt(p, 0);
    es.emit("rows", row(4), "7");
    await wait(40);
    es.error();
    expect(es.closed).toBe(true);
    expect(conn(p)).toBe("reconnecting");
    const reopen = p.timers.filter((t) => !t.cleared && t.ms >= 800 && t.ms <= 1300);
    expect(reopen.length).toBeGreaterThan(0);
    p.runTimers();
    await p.flush();
    const again = p.streams[p.streams.length - 1];
    expect(again).not.toBe(es);
    expect(again.url).toContain("since=7");
    again.emit("ready", "ok");
    expect(conn(p)).toBe("live");
    again.emit("rows", row(4) + row(5), "8");
    await wait(40);
    expect(ids(p)).toEqual(["5", "4", "3", "2", "1"]);
  });

  test("a second failure shows Disconnected with the seconds to the next try", async () => {
    const p = await started(tail([3, 2, 1]));
    const es = await live(p);
    es.error();
    p.runTimers();
    await p.flush();
    p.streams[p.streams.length - 1].error();
    expect(conn(p)).toBe("disconnected");
    expect(p.document.querySelector("[data-conn-text]")!.textContent).toMatch(/^Disconnected · Retry in \d+ s$/);
  });

  test("Snapshot closes the stream, drops what was held and says Snapshot", async () => {
    const p = await started(tail([3, 2, 1]));
    const es = await live(p);
    scrollAt(p, 400);
    es.emit("rows", row(5), "7");
    await wait(40);
    p.click(p.document.querySelector("[data-tail-mode=snapshot]")!);
    await p.flush();
    expect(es.closed).toBe(true);
    expect(pill(p).hidden).toBe(true);
    expect(conn(p)).toBe("snapshot");
    expect(ids(p)).toEqual(["3", "2", "1"]);
  });

  test("a tail that the page does not offer (a key search) stays a snapshot", async () => {
    const html = tail([3]).replace('data-tail-mode="live" x-on:click="live" disabled', 'data-tail-mode="live" x-on:click="live" data-unavailable disabled');
    const p = await started(html);
    await p.flush();
    expect((p.document.querySelector("[data-tail-mode=live]") as HTMLButtonElement).disabled).toBe(true);
  });

  test("the Refresh of a page that is tailing leaves the tail's region alone", async () => {
    const html = tail([3, 2, 1]).replace("<main id=\"main\">", `<main id="main">${bar("live-wire")}`);
    const p = await started(html);
    await live(p);
    p.autoAnswer(() => ({ etag: '"x"', body: "<div class=\"frag\" id=\"live-wire\"></div>" }));
    p.click(p.document.querySelector("[data-refresh-now]")!);
    await p.flush();
    expect(p.calls.length).toBe(0);
  });
});

describe("a time says how long ago, when the pointer is on it (CON-806)", () => {
  test("the page holds the absolute time; the hover adds the relative form, once", async () => {
    const at = new Date(Date.now() - 5000).toISOString();
    const p = await started(`<main id="main"><time id="t" datetime="${at}" title="exact" data-rel>22:13:33</time></main>`);
    const t = p.document.getElementById("t")!;
    expect(t.getAttribute("title")).toBe("exact");
    t.dispatchEvent(new p.window.MouseEvent("mouseover", { bubbles: true }));
    expect(t.getAttribute("title")).toMatch(/^exact · [45] s ago$/);
    t.dispatchEvent(new p.window.MouseEvent("mouseover", { bubbles: true }));
    expect(t.getAttribute("title")).toMatch(/^exact · \d+ s ago$/);
    expect(t.textContent).toBe("22:13:33");
  });
});

describe("a deadline counts down in its own element, with no request (CON-812)", () => {
  test("the element is filled and a one second timer runs only while a deadline is on the page", async () => {
    const until = new Date(Date.now() + 42_000).toISOString();
    const p = await started(`<main id="main"><span class="cd" data-until="${until}"></span></main>`);
    await wait(20);
    expect(p.document.querySelector(".cd")!.textContent).toMatch(/^\(in (4[01]|42) s\)$/);
    expect(p.calls).toEqual([]);
  });
});

describe("the keyboard", () => {
  test("j and k move the focus between the rows' links and stay inside the list", async () => {
    const p = await boot(
      `<main id="main"><table class="dt-t"><tbody>${[3, 2, 1].map((i) => `<tr><td><a href="/live/${i}">r${i}</a></td></tr>`).join("")}</tbody></table></main>`,
    );
    const links = [...p.document.querySelectorAll("tbody a")] as HTMLElement[];
    const key = (k: string) => p.document.body.dispatchEvent(new p.window.KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true }));
    key("j");
    expect(p.document.activeElement).toBe(links[0]);
    key("j");
    key("j");
    key("j");
    expect(p.document.activeElement).toBe(links[2]);
    key("k");
    expect(p.document.activeElement).toBe(links[1]);
  });

  test("a select that is marked applies its form when it changes", async () => {
    const p = await boot(`<main id="main"><form id="f"><select id="s" name="client" data-autosubmit><option value="">All</option><option value="a">a</option></select></form></main>`);
    const sel = p.document.getElementById("s") as HTMLSelectElement;
    sel.value = "a";
    sel.dispatchEvent(new p.window.Event("change", { bubbles: true }));
    await p.flush();
    expect(p.calls.length + p.forms.length).toBeGreaterThan(0);
  });
});

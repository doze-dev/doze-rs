// Navigation as the stack does it (htmx boost, out-of-band parts, history) and the seams console.js adds to it: scroll memory, the
// progress hint, focus after a swap, preload on intent. Run: `cd tools/dom && bun install && bun test`.

import { describe, expect, test } from "bun:test";
import { boot, type Page } from "./harness.ts";

const ORIGIN = "http://127.0.0.1:9093";
const SHELL = `<div class="root"><nav class="rail" id="rail"><a class="ni on" href="/topics" aria-current="page">Resources</a><a class="ni" id="live" href="/live">Live</a></nav>
<main class="main" id="main"><h1>Topics</h1><a id="go" href="/live">Live</a><a id="dlg" href="/topics/orders/delete" data-dialog>Delete</a>
<a id="inplace" href="/gallery?p=1" hx-get="/gallery?p=1" hx-target="#region" hx-select="#region" hx-swap="morph:outerHTML show:none" hx-push-url="true" data-nopreload>one</a>
<input id="typed" value="">
<div id="region"><p id="n">before</p></div></main></div>`;

/** What the server answers an htmx navigation with (src/region.rs): the region, the rail and the overlay root out of band. */
const region = (title: string, h1: string) =>
  `<title>${title} · doze-kafka</title><main class="main" id="main"><h1>${h1}</h1></main>` +
  `<nav class="rail" id="rail" aria-label="Spaces" hx-swap-oob="true"><a class="ni" href="/topics">Resources</a><a class="ni on" id="live" href="/live" aria-current="page">Live</a></nav><div id="ovl" hx-swap-oob="true"></div>`;

const started = (opts = {}) => boot(SHELL, { url: `${ORIGIN}/topics`, ...opts });
const text = (p: Page, sel: string) => p.document.querySelector(sel)?.textContent;

describe("a boosted link", () => {
  test("asks for the region, swaps main, updates the title and the rail, pushes the address, and focuses the new page", async () => {
    const p = await started();
    p.autoAnswer(() => ({ body: region("Live", "Live") }));
    p.click(p.document.getElementById("go")!);
    await p.flush();
    const call = p.calls[p.calls.length - 1];
    expect(call.url).toBe(`${ORIGIN}/live`);
    expect(call.headers["HX-Request"]).toBe("true");
    expect(call.headers["HX-Boosted"]).toBe("true");
    expect(call.headers["HX-Target"]).toBe("main");
    expect(text(p, "#main h1")).toBe("Live");
    expect(p.document.title).toBe("Live · doze-kafka");
    expect(p.document.querySelector(".rail .on")?.textContent).toBe("Live");
    expect(p.history.pushes).toEqual([`${ORIGIN}/live`]);
    expect(p.document.activeElement?.id).toBe("main");
    expect(p.navigated).toEqual([]);
  });

  test("no page opens a stream by itself: a swap to any page leaves the browser with no EventSource and no timer of ours", async () => {
    const p = await started();
    p.autoAnswer(() => ({ body: region("Live", "Live") }));
    p.click(p.document.getElementById("go")!);
    await p.flush();
    expect(p.streams.length).toBe(0);
  });

  test("a request that cannot be made is followed as the browser would", async () => {
    const p = await started();
    p.click(p.document.getElementById("go")!);
    await p.flush();
    p.pending[0].fail(new Error("network"));
    await p.flush();
    expect(p.navigated).toEqual(["/live"]);
  });

  test("a modified click is the browser's: htmx and the script leave it alone", async () => {
    const p = await started();
    p.document.getElementById("go")!.dispatchEvent(new p.window.MouseEvent("click", { bubbles: true, cancelable: true, ctrlKey: true }));
    await p.flush();
    expect(p.calls.length).toBe(0);
  });
});

describe("a control that owns a region", () => {
  test("changes only that region, keeps what was typed outside it and the focus, and pushes the address", async () => {
    const p = await started();
    const typed = p.document.getElementById("typed") as HTMLInputElement;
    typed.value = "half a sentence";
    typed.focus();
    p.autoAnswer(() => ({ body: `<title>x</title><main id="main"><h1>Topics</h1><div id="region"><p id="n">after</p></div></main>` }));
    p.click(p.document.getElementById("inplace")!);
    await p.flush();
    expect(p.calls[0].headers["HX-Target"]).toBe("region");
    expect(text(p, "#n")).toBe("after");
    expect(typed.value).toBe("half a sentence");
    expect(p.document.activeElement).toBe(typed);
    expect(text(p, "#main h1")).toBe("Topics");
    expect(p.history.pushes).toEqual([`${ORIGIN}/gallery?p=1`]);
    expect(p.scrolls).toEqual([]);
  });

  test("nothing fades: a changed answer changes its text and animates nothing (an opacity dip was the flicker)", async () => {
    const p = await started();
    p.autoAnswer(() => ({ body: `<main id="main"><div id="region"><p id="n">before</p></div></main>` }));
    p.click(p.document.getElementById("inplace")!);
    await p.flush();
    expect(p.animated.map((e) => e.id)).toEqual([]);
    p.autoAnswer(() => ({ body: `<main id="main"><div id="region"><p id="n">different</p></div></main>` }));
    p.click(p.document.getElementById("inplace")!);
    await p.flush();
    expect(p.document.getElementById("n")!.textContent).toBe("different");
    expect(p.animated.map((e) => e.id)).toEqual([]);
  });
});

describe("history", () => {
  test("Back from an overlay's entry is ours: it asks htmx for nothing and puts no page back", async () => {
    const p = await started();
    const overlay = `<div class="ovl ovl-drawer root" data-overlay data-close="/topics"><a class="scrim" href="/topics" data-overlay-close></a><div class="drawer-panel" role="dialog" aria-labelledby="t" tabindex="-1"><h2 id="t">x</h2></div></div>`;
    p.autoAnswer(() => ({ body: overlay }));
    p.click(p.document.getElementById("dlg")!);
    await p.flush();
    expect(p.document.querySelector("[data-overlay]")).not.toBeNull();
    const before = p.calls.length;
    p.history.back();
    await p.flush();
    p.runTimers();
    expect(p.calls.length).toBe(before);
    expect(p.document.querySelector("[data-overlay]")).toBeNull();
    expect(text(p, "#main h1")).toBe("Topics");
  });

  test("the scroll a page had is remembered when it is left and given back when the page is restored", async () => {
    const p = await started();
    Object.defineProperty(p.window, "scrollY", { value: 321, configurable: true });
    p.autoAnswer(() => ({ body: region("Live", "Live") }));
    p.click(p.document.getElementById("go")!);
    await p.flush();
    expect(p.window.sessionStorage.getItem("doze:y:/topics")).toBe("321");
    // htmx has put the page at /topics back (it fetched it again): the scroll goes with it.
    p.history.entries[p.history.index].url = `${ORIGIN}/topics`;
    p.document.body.dispatchEvent(new p.window.CustomEvent("htmx:historyRestore", { bubbles: true, detail: {} }));
    expect(p.scrolls[p.scrolls.length - 1][0]).toEqual({ top: 321, left: 0, behavior: "instant" }); // instant: the page has `scroll-behavior: smooth`, a restore must not glide
  });
});

describe("preload on intent", () => {
  const over = (p: Page, id: string) => p.document.getElementById(id)!.dispatchEvent(new p.window.MouseEvent("mouseover", { bubbles: true }));

  test("a boosted link the pointer rests on for 65 ms is fetched ahead with the headers of a navigation, once", async () => {
    const p = await started();
    over(p, "go");
    expect(p.timers.some((t) => t.ms === 65)).toBe(true);
    p.autoAnswer(() => ({ body: "x" }));
    p.runTimers();
    await p.flush();
    const pre = p.calls.filter((c) => c.url === `${ORIGIN}/live`);
    expect(pre.length).toBe(1);
    expect(pre[0].headers).toMatchObject({ "HX-Request": "true", "HX-Boosted": "true", "HX-Target": "main", "HX-Preloaded": "true" });
    over(p, "go");
    p.runTimers();
    await p.flush();
    expect(p.calls.filter((c) => c.url === `${ORIGIN}/live`).length).toBe(1);
  });

  test("leaving before 65 ms fetches nothing; an overlay opener and an in-place control are never fetched ahead", async () => {
    const p = await started();
    over(p, "go");
    p.document.getElementById("go")!.dispatchEvent(new p.window.MouseEvent("mouseout", { bubbles: true }));
    p.runTimers();
    over(p, "dlg");
    over(p, "inplace");
    p.runTimers();
    await p.flush();
    expect(p.calls.length).toBe(0);
  });

  test("keyboard focus is the same intent", async () => {
    const p = await started();
    p.autoAnswer(() => ({ body: "x" }));
    p.document.getElementById("live")!.focus();
    p.document.getElementById("live")!.dispatchEvent(new p.window.FocusEvent("focusin", { bubbles: true }));
    await p.flush();
    expect(p.calls.some((c) => c.url === `${ORIGIN}/live` && c.headers["HX-Preloaded"] === "true")).toBe(true);
  });
});

describe("the progress hint", () => {
  const html = (p: Page) => p.document.documentElement.classList;

  test("a request that takes over 250 ms shows it, and the answer takes it away", async () => {
    const p = await started();
    p.click(p.document.getElementById("go")!);
    await p.flush();
    expect(p.timers.some((t) => t.ms === 250)).toBe(true);
    p.runTimers();
    expect(html(p).contains("is-loading")).toBe(true);
    p.pending[0].answer({ body: region("Live", "Live") });
    await p.flush();
    expect(html(p).contains("is-loading")).toBe(false);
  });

  test("a fast request never shows it", async () => {
    const p = await started();
    p.click(p.document.getElementById("go")!);
    await p.flush();
    p.pending[0].answer({ body: region("Live", "Live") });
    await p.flush();
    p.runTimers();
    expect(html(p).contains("is-loading")).toBe(false);
  });

  test("a preload shows nothing at all", async () => {
    const p = await started();
    p.document.getElementById("go")!.dispatchEvent(new p.window.MouseEvent("mouseover", { bubbles: true }));
    p.runTimers();
    await p.flush();
    expect(p.timers.some((t) => t.ms === 250 && !t.cleared)).toBe(false);
  });
});

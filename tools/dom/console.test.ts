// The behaviour of crates/kafka-console/assets/console.js (decision D4a, docs/port/phase9/client-spike.md), asserted in a
// DOM: the rules the spike found, and the audit's A8, A9, A13, A18 and A19. Run: `cd tools/dom && bun install && bun test`.

import { describe, expect, test } from "bun:test";
import { boot, type Options, type Page } from "./harness.ts";

const PAGE = `
<div id="stream-banner" hidden>Live updates are paused.</div>
<div class="frag" id="home-live" data-frag="/fragments/home-live"><p id="marker">old</p><input id="q" name="q" value="a"></div>
<div class="frag" id="other" data-frag="/fragments/other"><p id="o">other-old</p></div>
<div data-address="127.0.0.1:9092">
  <div class="seg">
    <button data-copy-as="code" class="on" data-js-only hidden>Code</button>
    <button data-copy-as="properties" data-js-only hidden>Properties</button>
    <button data-copy-as="environment" data-js-only hidden>Environment</button>
  </div>
  <button id="snippet" data-copy-from="s1" data-copy-forms data-js-only hidden>Copy</button><pre id="s1">producer := code</pre>
  <button id="sasl" data-copy-from="sasl-lines" data-js-only hidden>Copy</button><pre id="sasl-lines">security.protocol=SASL_PLAINTEXT</pre>
  <button id="chip" data-copy="127.0.0.1:9092">chip</button>
</div>
<form id="check" data-ajax data-target="probe-home" action="/connect/advertised/check" method="post"><input type="hidden" name="back" value="home"><button>Check</button></form>
<div class="frag" id="probe-home" data-frag="/fragments/probe-home"><p id="p">before</p></div>`;

const home = (marker: string, q = "x") => `<div class="frag" id="home-live" data-frag="/fragments/home-live"><p id="marker">${marker}</p><input id="q" name="q" value="${q}"></div>`;
const other = (text: string) => `<div class="frag" id="other" data-frag="/fragments/other"><p id="o">${text}</p></div>`;

/** The page with the script run on it, the stream attribute on the body as the server renders it. */
const started = (opts: Options = {}): Promise<Page> => boot(PAGE, { stream: "/events?watch=home-live,other", ...opts });

const text = (p: Page, id: string) => p.document.getElementById(id)?.textContent;
const banner = (p: Page) => p.document.getElementById("stream-banner")!;

describe("the stream", () => {
  test("opens one stream for the page and a tick fetches the fragments it names", async () => {
    const p = await started();
    expect(p.streams.length).toBe(1);
    expect(p.streams[0].url).toBe("/events?watch=home-live,other");
    p.tick("home-live,other");
    await p.flush();
    expect(p.calls.map((c) => c.url)).toEqual(["/fragments/home-live", "/fragments/other"]);
  });

  test("closes on pagehide, reopens on pageshow of a page from the cache and not on the first show", async () => {
    const p = await started();
    const first = p.streams[0];
    p.window.dispatchEvent(new p.window.Event("pageshow"));
    expect(p.streams.length).toBe(1);
    p.window.dispatchEvent(new p.window.Event("pagehide"));
    expect(first.closed).toBe(true);
    const back = new p.window.Event("pageshow") as Event & { persisted?: boolean };
    back.persisted = true;
    p.window.dispatchEvent(back);
    expect(p.streams.length).toBe(2);
    expect(p.streams[1].closed).toBe(false);
  });

  test("a hidden tab holds no stream and a visible one has one", async () => {
    const p = await started();
    let hidden = true;
    Object.defineProperty(p.document, "hidden", { get: () => hidden, configurable: true });
    p.document.dispatchEvent(new p.window.Event("visibilitychange"));
    expect(p.streams[0].closed).toBe(true);
    hidden = false;
    p.document.dispatchEvent(new p.window.Event("visibilitychange"));
    expect(p.streams.length).toBe(2);
    expect(p.streams[1].closed).toBe(false);
  });

  test("reconnects for ever with a wait that doubles to 15 s and starts again after an open", async () => {
    const p = await started();
    const waits: number[] = [];
    for (let i = 0; i < 14; i++) {
      p.streams[p.streams.length - 1].error();
      const t = p.timers.filter((x) => !x.cleared).pop()!;
      waits.push(t.ms);
      p.runTimers();
    }
    expect(waits).toEqual([1000, 2000, 4000, 8000, 15000, 15000, 15000, 15000, 15000, 15000, 15000, 15000, 15000, 15000]);
    // Fourteen failures later it is still trying: a new stream was opened each time.
    expect(p.streams.length).toBe(15);
    expect(p.streams.slice(0, -1).every((s) => s.closed)).toBe(true);
    // A stream that opens resets the wait, and hides the banner the failures showed.
    expect(banner(p).hidden).toBe(false);
    p.streams[p.streams.length - 1].open();
    expect(banner(p).hidden).toBe(true);
    p.streams[p.streams.length - 1].error();
    expect(p.timers.filter((x) => !x.cleared).pop()!.ms).toBe(1000);
  });
});

describe("a fragment", () => {
  test("sends the ETag it was given, and a 304 changes nothing", async () => {
    const p = await started();
    p.tick("home-live");
    await p.flush();
    expect(p.calls[0].headers["If-None-Match"]).toBeUndefined();
    p.pending[0].answer({ etag: '"v1"', body: home("new") });
    await p.flush();
    expect(text(p, "marker")).toBe("new");
    // The page is edited by hand (a person selected text, say): an unchanged answer must not touch it.
    p.document.getElementById("marker")!.textContent = "kept";
    p.tick("home-live");
    await p.flush();
    expect(p.calls[1].headers["If-None-Match"]).toBe('"v1"');
    p.pending[0].answer({ status: 304, etag: '"v1"' });
    await p.flush();
    expect(text(p, "marker")).toBe("kept");
    // And the tag is kept: the next ask carries it again.
    p.tick("home-live");
    await p.flush();
    expect(p.calls[2].headers["If-None-Match"]).toBe('"v1"');
  });

  test("keeps a tag for each fragment and a new one replaces the old", async () => {
    const p = await started();
    p.autoAnswer((c) => (c.url.endsWith("home-live") ? { etag: '"A1"', body: home("h") } : { etag: '"B1"', body: other("o") }));
    p.tick("home-live,other");
    await p.flush();
    p.autoAnswer((c) => ({ status: 304, etag: c.headers["If-None-Match"] }));
    p.tick("home-live,other");
    await p.flush();
    const second = Object.fromEntries(p.calls.slice(2).map((c) => [c.url, c.headers["If-None-Match"]]));
    expect(second).toEqual({ "/fragments/home-live": '"A1"', "/fragments/other": '"B1"' });
    p.autoAnswer((c) => (c.url.endsWith("home-live") ? { etag: '"A2"', body: home("h2") } : { status: 304 }));
    p.tick("home-live,other");
    await p.flush();
    p.autoAnswer((c) => ({ status: 304, etag: c.headers["If-None-Match"] }));
    p.tick("home-live,other");
    await p.flush();
    const third = Object.fromEntries(p.calls.slice(6).map((c) => [c.url, c.headers["If-None-Match"]]));
    expect(third).toEqual({ "/fragments/home-live": '"A2"', "/fragments/other": '"B1"' });
  });

  test("each fragment has an AbortController of its own, and a newer request aborts only the older one of the same fragment", async () => {
    const p = await started();
    p.tick("home-live,other");
    await p.flush();
    const [h1, o1] = p.calls;
    expect(h1.signal).toBeDefined();
    expect(h1.signal).not.toBe(o1.signal);
    p.tick("home-live");
    await p.flush();
    const h2 = p.calls[2];
    expect(h1.signal!.aborted).toBe(true);
    expect(h2.signal!.aborted).toBe(false);
    expect(o1.signal!.aborted).toBe(false);
  });

  test("an answer that arrives late never overwrites a newer one", async () => {
    const p = await started();
    p.tick("home-live");
    await p.flush();
    p.tick("home-live");
    await p.flush();
    expect(p.pending.length).toBe(1); // the first was aborted and is gone
    p.pending[0].answer({ etag: '"new"', body: home("newer") });
    await p.flush();
    expect(text(p, "marker")).toBe("newer");
    expect(banner(p).hidden).toBe(true);
    // The stale one, answered as a browser answers an aborted fetch, changes nothing and does not raise the banner.
    expect(p.calls[0].signal!.aborted).toBe(true);
    expect(text(p, "marker")).toBe("newer");
  });

  test("a failed fetch shows the banner and the next answer hides it, even a 304", async () => {
    const p = await started();
    p.tick("home-live");
    await p.flush();
    p.pending[0].fail(new TypeError("Failed to fetch"));
    await p.flush();
    expect(banner(p).hidden).toBe(false);
    p.tick("home-live");
    await p.flush();
    p.pending[0].answer({ status: 304 });
    await p.flush();
    expect(banner(p).hidden).toBe(true);
    // An error status is a failure too, and a good answer after it clears it.
    p.tick("home-live");
    await p.flush();
    p.pending[0].answer({ status: 503 });
    await p.flush();
    expect(banner(p).hidden).toBe(false);
    p.tick("home-live");
    await p.flush();
    p.pending[0].answer({ etag: '"z"', body: home("z") });
    await p.flush();
    expect(banner(p).hidden).toBe(true);
  });

  test("the input that has the focus keeps its text and its caret through a swap, one that has not takes the server's", async () => {
    const p = await started();
    const input = p.document.getElementById("q") as unknown as HTMLInputElement;
    input.focus();
    input.value = "abc";
    input.setSelectionRange(2, 2);
    p.tick("home-live");
    await p.flush();
    p.pending[0].answer({ etag: '"1"', body: home("swapped", "from-server") });
    await p.flush();
    expect(text(p, "marker")).toBe("swapped");
    const after = p.document.getElementById("q") as unknown as HTMLInputElement;
    expect(after).toBe(input);
    expect(p.document.activeElement).toBe(input);
    expect(after.value).toBe("abc");
    expect(after.selectionStart).toBe(2);
    input.blur();
    p.tick("home-live");
    await p.flush();
    p.pending[0].answer({ etag: '"2"', body: home("again", "from-server") });
    await p.flush();
    expect((p.document.getElementById("q") as unknown as HTMLInputElement).value).toBe("from-server");
  });

  test("a focused input the person has not typed in yet still keeps what it shows", async () => {
    const p = await started();
    const input = p.document.getElementById("q") as unknown as HTMLInputElement;
    input.focus();
    p.tick("home-live");
    await p.flush();
    p.pending[0].answer({ etag: '"1"', body: home("swapped", "from-server") });
    await p.flush();
    expect(text(p, "marker")).toBe("swapped");
    expect(p.document.activeElement).toBe(input);
    expect(input.value).toBe("a");
    expect(input.getAttribute("value")).toBe("a");
  });

  test("a whole region that changes shape is swapped in place (the state-dependent part of Home is one fragment)", async () => {
    const p = await started();
    p.tick("home-live");
    await p.flush();
    p.pending[0].answer({
      etag: '"quiet"',
      body: `<div class="frag" id="home-live" data-frag="/fragments/home-live"><h1>Your broker is up</h1><p id="marker">first</p><section id="first-request">Waiting for your first request</section></div>`,
    });
    await p.flush();
    expect(p.document.querySelector("#home-live h1")?.textContent).toBe("Your broker is up");
    expect(p.document.getElementById("first-request")).not.toBeNull();
    p.tick("home-live");
    await p.flush();
    p.pending[0].answer({
      etag: '"steady"',
      body: `<div class="frag" id="home-live" data-frag="/fragments/home-live"><h1>Home</h1><p id="marker">second</p></div>`,
    });
    await p.flush();
    expect(p.document.querySelector("#home-live h1")?.textContent).toBe("Home");
    expect(p.document.getElementById("first-request")).toBeNull();
    expect(text(p, "marker")).toBe("second");
  });
});

describe("a form", () => {
  test("is posted in place with the fragment header and its answer swapped in", async () => {
    const p = await started();
    const form = p.document.getElementById("check")!;
    form.dispatchEvent(new p.window.Event("submit", { bubbles: true, cancelable: true }));
    await p.flush();
    expect(p.calls[0].method).toBe("POST");
    expect(p.calls[0].url).toContain("/connect/advertised/check");
    expect(p.calls[0].headers["X-Fragment"]).toBe("1");
    p.pending[0].answer({ body: `<div class="frag" id="probe-home" data-frag="/fragments/probe-home"><p id="p">after</p></div>` });
    await p.flush();
    expect(text(p, "p")).toBe("after");
    expect(p.forms).toEqual([]);
  });

  test("is submitted as the browser would when the post fails", async () => {
    const p = await started();
    p.document.getElementById("check")!.dispatchEvent(new p.window.Event("submit", { bubbles: true, cancelable: true }));
    await p.flush();
    p.pending[0].answer({ status: 500 });
    await p.flush();
    expect(p.forms.length).toBe(1);
  });
});

describe("copy", () => {
  test("a snippet follows Copy as, and the card that copies its own lines never does", async () => {
    const p = await started();
    const snippet = p.document.getElementById("snippet")!;
    const sasl = p.document.getElementById("sasl")!;
    const choose = (name: string) => p.click(p.document.querySelector(`[data-copy-as="${name}"]`)!);
    p.click(snippet);
    await p.flush();
    expect(p.copied.at(-1)).toBe("producer := code");
    choose("properties");
    p.click(snippet);
    p.click(sasl);
    await p.flush();
    expect(p.copied.slice(-2)).toEqual(["bootstrap.servers=127.0.0.1:9092", "security.protocol=SASL_PLAINTEXT"]);
    choose("environment");
    p.click(snippet);
    p.click(sasl);
    await p.flush();
    expect(p.copied.slice(-2)).toEqual(["KAFKA_BOOTSTRAP_SERVERS=127.0.0.1:9092", "security.protocol=SASL_PLAINTEXT"]);
    // A button with a text of its own always copies it, and the choice is shown on the control.
    p.click(p.document.getElementById("chip")!);
    await p.flush();
    expect(p.copied.at(-1)).toBe("127.0.0.1:9092");
    const on = [...p.document.querySelectorAll("[data-copy-as]")].filter((b) => b.classList.contains("on")).map((b) => b.getAttribute("data-copy-as"));
    expect(on).toEqual(["environment"]);
  });

  test("without the clipboard API (plain http on a LAN address) it selects a field and asks the document to copy", async () => {
    const p = await started({ clipboard: false });
    p.click(p.document.getElementById("snippet")!);
    await p.flush();
    expect(p.execCopied).toEqual(["producer := code"]);
    // The field it made is gone again, and nothing was put on a clipboard that is not there.
    expect(p.document.querySelectorAll("textarea").length).toBe(0);
    expect(p.copied).toEqual([]);
    p.click(p.document.getElementById("chip")!);
    expect(p.execCopied.at(-1)).toBe("127.0.0.1:9092");
  });

  test("when the clipboard API refuses, the old way is tried", async () => {
    const p = await started();
    Object.defineProperty(p.window.navigator, "clipboard", { value: { writeText: () => Promise.reject(new Error("denied")) }, configurable: true });
    p.click(p.document.getElementById("chip")!);
    await p.flush();
    expect(p.execCopied).toEqual(["127.0.0.1:9092"]);
  });

  test("the controls that need the script are revealed by it", async () => {
    const p = await started();
    expect([...p.document.querySelectorAll("[data-js-only]")].every((e) => !(e as HTMLElement).hidden)).toBe(true);
  });
});

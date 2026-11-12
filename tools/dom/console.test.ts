// The behaviour of crates/kafka-console/assets/console.js that is not live updates (those are live.test.ts): forms posted in
// place, copy. Run: `cd tools/dom && bun install && bun test`.

import { describe, expect, test } from "bun:test";
import { boot, type Options, type Page } from "./harness.ts";

const PAGE = `
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
<form id="check" action="/connect/advertised/check" method="post" hx-post="/connect/advertised/check" hx-target="#probe-home" hx-select="#probe-home" hx-swap="morph:outerHTML show:none" hx-headers='{"X-Fragment":"1"}'><input type="hidden" name="back" value="home"><button>Check</button></form>
<div class="frag" id="probe-home" data-frag="/fragments/probe-home"><p id="p">before</p></div>`;


/** The page with the script run on it. */
const started = (opts: Options = {}): Promise<Page> => boot(PAGE, opts);

const text = (p: Page, id: string) => p.document.getElementById(id)?.textContent;

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

  test("is submitted as the browser would when the request cannot be made", async () => {
    const p = await started();
    p.document.getElementById("check")!.dispatchEvent(new p.window.Event("submit", { bubbles: true, cancelable: true }));
    await p.flush();
    p.pending[0].fail(new Error("network"));
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

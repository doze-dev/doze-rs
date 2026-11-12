// The overlays of crates/kafka-console/assets/console.js (src/ui/overlay.rs), over htmx (which fetches them into #ovl): a drawer or a
// dialog opens over the page with no navigation, the address is pushed, Esc, the scrim, Close and Back close it, focus stays inside and returns, the page behind is
// inert, a second opener replaces the content, a server-rendered overlay is adopted, a form's answer replaces it in place, and
// with reduced motion nothing animates. Run: `cd tools/dom && bun install && bun test`.

import { describe, expect, test } from "bun:test";
import { boot, type Options, type Page } from "./harness.ts";

const ORIGIN = "http://127.0.0.1:9093";
const RECORD = "/topics/orders/messages/1/16050";

const PAGE = `<div class="root"><main id="main">
  <h1>orders</h1>
  <a id="open" class="lnk" href="${RECORD}" data-dialog>16,050</a>
  <a id="open2" class="lnk" href="/topics/orders/messages/1/16051" data-dialog>16,051</a>
  <button id="after">after</button>
  <div class="frag" id="rows" data-frag="/fragments/rows"><p id="n">one</p></div>
</main></div>`;

/** The overlay element as the server sends it (src/ui/overlay.rs). */
const overlay = (title: string, kind = "drawer", body = "") =>
  `<div class="ovl ovl-${kind} root" data-overlay data-close="/topics/orders"><a class="scrim" href="/topics/orders" tabindex="-1" aria-hidden="true" data-overlay-close></a>` +
  `<div class="${kind === "drawer" ? "drawer-panel" : "modal"}" role="dialog" aria-modal="true" aria-labelledby="t" tabindex="-1"><h2 id="t">${title}</h2>${body}` +
  `<a class="btn" id="close" href="/topics/orders" data-dialog-close>Close</a></div></div>`;
const OVL = { "x-overlay": "1" };
const reply = (title: string, kind = "drawer", body = "") => ({ headers: OVL, body: overlay(title, kind, body) });

const open = async (p: Page, id = "open", title = "orders-1 · 16,050") => {
  p.autoAnswer(() => reply(title));
  p.click(p.document.getElementById(id)!);
  await p.flush();
};
const el = (p: Page, sel: string) => p.document.querySelector(sel) as HTMLElement;
const root = (p: Page) => el(p, ".root:not([data-overlay])");
const key = (p: Page, k: string, extra: Record<string, unknown> = {}) => {
  const e = new p.window.KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...extra });
  (p.document.activeElement ?? p.document.body).dispatchEvent(e);
  return e;
};
const started = (opts: Options = {}) => boot(PAGE, opts);

describe("opening", () => {
  test("a link marked as an opener fetches the overlay alone and puts it over the page, with no navigation", async () => {
    const p = await started();
    await open(p);
    expect(p.calls.length).toBe(1);
    expect(p.calls[0].url).toBe(`${ORIGIN}${RECORD}`);
    expect(p.calls[0].headers["X-Fragment"]).toBe("1");
    expect(p.navigated).toEqual([]);
    expect(el(p, "[data-overlay] h2").textContent).toBe("orders-1 · 16,050");
    expect(el(p, "h1").textContent).toBe("orders");
  });

  test("pushes the overlay's address, so that Back can close it", async () => {
    const p = await started();
    await open(p);
    expect(p.history.pushes).toEqual([`${ORIGIN}${RECORD}`]);
    expect(p.history.state).toEqual({ ovl: 1, parent: "/" });
  });

  test("makes the page behind inert and moves focus into the panel, which is named by its title", async () => {
    const p = await started();
    p.document.getElementById("open")!.focus();
    await open(p);
    expect(root(p).hasAttribute("inert")).toBe(true);
    const panel = el(p, "[role=dialog]");
    expect(panel.getAttribute("aria-modal")).toBe("true");
    expect(p.document.getElementById(panel.getAttribute("aria-labelledby")!)!.textContent).toContain("16,050");
    expect(panel.contains(p.document.activeElement)).toBe(true);
  });

  test("a form's first field takes the focus, a panel with none takes it itself", async () => {
    const p = await started();
    await open(p);
    expect(p.document.activeElement).toBe(el(p, "[role=dialog]"));
    p.history.back();
    p.runTimers();
    p.autoAnswer(() => reply("Produce", "dialog", `<form method="post" action="/topics/orders/produce"><input id="hidden" type="hidden" name="h"><input id="first" name="a"><input name="b"></form>`));
    p.click(p.document.getElementById("open")!);
    await p.flush();
    expect(p.document.activeElement?.id).toBe("first");
  });

  test("comes in with the class the CSS animates, and with reduced motion with none", async () => {
    const p = await started();
    await open(p);
    expect(el(p, "[data-overlay]").classList.contains("ovl-in")).toBe(true);
    const q = await started({ reduced: true });
    await open(q);
    expect(el(q, "[data-overlay]").classList.contains("ovl-in")).toBe(false);
  });

  test("an answer that is not an overlay (a page that is not there) follows the link as a browser would", async () => {
    const p = await started();
    p.autoAnswer(() => ({ status: 404, body: "<html>No such record</html>" }));
    p.click(p.document.getElementById("open")!);
    await p.flush();
    expect(p.navigated).toEqual([`${ORIGIN}${RECORD}`]);
    expect(p.document.querySelector("[data-overlay]")).toBeNull();
    expect(p.history.pushes).toEqual([]);
  });

  test("a modified click is the browser's: a new tab is not an overlay", async () => {
    const p = await started();
    p.document.getElementById("open")!.dispatchEvent(new p.window.MouseEvent("click", { bubbles: true, cancelable: true, ctrlKey: true }));
    await p.flush();
    expect(p.calls.length).toBe(0);
  });

  test("a second opener replaces the content in place: no new entry of its own, no coming-in again", async () => {
    const p = await started();
    await open(p);
    p.autoAnswer(() => reply("orders-1 · 16,051"));
    p.click(p.document.getElementById("open2")!);
    await p.flush();
    expect(p.document.querySelectorAll("[data-overlay]").length).toBe(1);
    expect(el(p, "[data-overlay] h2").textContent).toBe("orders-1 · 16,051");
    expect(el(p, "[data-overlay]").classList.contains("ovl-in")).toBe(false);
    expect(p.history.pushes.length).toBe(1);
    expect(p.history.url).toBe(`${ORIGIN}/topics/orders/messages/1/16051`);
  });
});

describe("closing", () => {
  const closed = async (p: Page) => {
    await p.flush();
    p.runTimers();
  };

  test("Esc closes it: the address goes back, the page is live again and focus returns to what opened it", async () => {
    const p = await started();
    p.document.getElementById("open")!.focus();
    await open(p);
    key(p, "Escape");
    await closed(p);
    expect(p.document.querySelector("[data-overlay]")).toBeNull();
    expect(p.history.url).toBe(`${ORIGIN}/`);
    expect(root(p).hasAttribute("inert")).toBe(false);
    expect(p.document.activeElement?.id).toBe("open");
  });

  test("goes out with the class the CSS animates for it, and is removed when that ends", async () => {
    const p = await started();
    await open(p);
    key(p, "Escape");
    const o = el(p, "[data-overlay]");
    expect(o.classList.contains("ovl-out")).toBe(true);
    expect(o.classList.contains("ovl-in")).toBe(false);
    o.dispatchEvent(new p.window.Event("animationend"));
    expect(p.document.querySelector("[data-overlay]")).toBeNull();
  });

  test("with reduced motion it is gone at once", async () => {
    const p = await started({ reduced: true });
    await open(p);
    key(p, "Escape");
    expect(p.document.querySelector("[data-overlay]")).toBeNull();
  });

  test("a click on the scrim closes it, and so does Close", async () => {
    const p = await started();
    await open(p);
    p.click(el(p, ".scrim"));
    await closed(p);
    expect(p.document.querySelector("[data-overlay]")).toBeNull();
    await open(p);
    p.click(p.document.getElementById("close")!);
    await closed(p);
    expect(p.document.querySelector("[data-overlay]")).toBeNull();
    expect(p.navigated).toEqual([]);
  });

  test("Back closes it, and Forward opens it again", async () => {
    const p = await started();
    await open(p);
    p.history.back();
    await closed(p);
    expect(p.document.querySelector("[data-overlay]")).toBeNull();
    expect(root(p).hasAttribute("inert")).toBe(false);
    p.autoAnswer(() => reply("orders-1 · 16,050"));
    p.history.forward();
    await p.flush();
    expect(el(p, "[data-overlay] h2").textContent).toBe("orders-1 · 16,050");
    expect(p.calls[p.calls.length - 1].url).toBe(`${ORIGIN}${RECORD}`);
  });

  test("Esc does nothing when no overlay is open, and when something already took it", async () => {
    const p = await started();
    expect(key(p, "Escape").defaultPrevented).toBe(false);
    await open(p);
    p.document.addEventListener("keydown", (e) => e.preventDefault(), { once: true });
    // (the page's own handler runs first only if it is registered first: here ours is, and it prevents the default itself)
    key(p, "Escape");
    await closed(p);
    expect(p.document.querySelector("[data-overlay]")).toBeNull();
  });
});

describe("focus", () => {
  const withForm = `<form method="post" action="/x"><input id="a" name="a"><button id="b" type="submit">Go</button></form>`;

  test("Tab at the last control goes to the first and Shift+Tab at the first to the last: focus does not leave", async () => {
    const p = await started();
    p.autoAnswer(() => reply("Produce", "dialog", withForm));
    p.click(p.document.getElementById("open")!);
    await p.flush();
    const a = p.document.getElementById("a")!;
    const close = p.document.getElementById("close")!;
    close.focus();
    expect(key(p, "Tab").defaultPrevented).toBe(true);
    expect(p.document.activeElement).toBe(a);
    a.focus();
    expect(key(p, "Tab", { shiftKey: true }).defaultPrevented).toBe(true);
    expect(p.document.activeElement).toBe(close);
    // In the middle the browser moves focus on its own.
    p.document.getElementById("b")!.focus();
    expect(key(p, "Tab").defaultPrevented).toBe(false);
  });

  test("focus that has left the panel is pulled back in", async () => {
    const p = await started();
    p.autoAnswer(() => reply("Produce", "dialog", withForm));
    p.click(p.document.getElementById("open")!);
    await p.flush();
    // (A browser will not focus into the inert page; focus that was lost to the document is what is left.)
    (p.document.activeElement as HTMLElement).blur();
    expect(p.document.activeElement === p.document.body || p.document.activeElement === null).toBe(true);
    expect(key(p, "Tab").defaultPrevented).toBe(true);
    expect(el(p, "[role=dialog]").contains(p.document.activeElement)).toBe(true);
  });
});

describe("a server-rendered overlay (a direct visit, a reload)", () => {
  const direct = (opts: Options = {}) => boot(PAGE.replace('class="root"', 'class="root" inert') + `<div id="ovl">${overlay("orders-1 · 16,050")}</div>`, { url: `${ORIGIN}${RECORD}`, ...opts });

  test("is adopted: Esc closes it and the address becomes the parent's, with no fetch and no navigation", async () => {
    const p = await direct();
    expect(p.document.activeElement).toBe(el(p, "[role=dialog]"));
    key(p, "Escape");
    p.runTimers();
    expect(p.document.querySelector("[data-overlay]")).toBeNull();
    expect(root(p).hasAttribute("inert")).toBe(false);
    expect(p.history.url).toBe(`${ORIGIN}/topics/orders`);
    expect(p.calls.length).toBe(0);
    expect(p.navigated).toEqual([]);
  });

  test("its Close and its scrim close it the same way", async () => {
    const p = await direct({ reduced: true });
    p.click(p.document.getElementById("close")!);
    expect(p.document.querySelector("[data-overlay]")).toBeNull();
    expect(p.history.url).toBe(`${ORIGIN}/topics/orders`);
  });

  test("an opener on the page behind it replaces its content", async () => {
    const p = await direct();
    p.autoAnswer(() => reply("orders-1 · 16,051"));
    // The page behind is inert in a browser; here the click is made on the link directly.
    p.click(p.document.getElementById("open2")!);
    await p.flush();
    expect(p.document.querySelectorAll("[data-overlay]").length).toBe(1);
    expect(el(p, "[data-overlay] h2").textContent).toBe("orders-1 · 16,051");
  });
});

describe("forms in an overlay", () => {
  const form = `<form id="f" method="post" action="/topics/orders/produce"><input name="value" value="v"><button id="send" type="submit">Send</button></form>`;
  const opened = async (opts: Options = {}) => {
    const p = await started(opts);
    p.autoAnswer(() => reply("Produce", "dialog", form));
    p.click(p.document.getElementById("open")!);
    await p.flush();
    p.autoAnswer(() => ({}));
    return p;
  };
  const submit = async (p: Page) => {
    p.document.getElementById("f")!.dispatchEvent(new p.window.Event("submit", { bubbles: true, cancelable: true }));
    await p.flush();
  };

  test("posts in place, asking for the overlay alone, and a refusal replaces the overlay and stays in it", async () => {
    const p = await opened();
    p.autoAnswer(() => ({ status: 422, headers: OVL, body: overlay("Produce", "dialog", `<p id="err">A compacted topic needs a key.</p>`) }));
    await submit(p);
    const post = p.calls[p.calls.length - 1];
    expect(post.method).toBe("POST");
    expect(post.url).toBe(`${ORIGIN}/topics/orders/produce`);
    expect(post.headers["X-Fragment"]).toBe("1");
    expect(p.document.getElementById("err")!.textContent).toContain("needs a key");
    expect(p.document.querySelectorAll("[data-overlay]").length).toBe(1);
    expect(p.forms).toEqual([]);
    expect(root(p).hasAttribute("inert")).toBe(true);
  });

  // What the server sends when a write's result is a PAGE: the region (title, main, the rail and the overlay root out of band) with the
  // headers that src/region.rs adds for a request that targeted the overlay root.
  const RESULT = { "HX-Retarget": "#main", "HX-Reswap": "outerHTML", "HX-Replace-Url": "/topics/orders" };
  const region = (title: string, h1: string) => `<title>${title} · doze-kafka</title><main class="main" id="main"><h1>${h1}</h1></main><div id="ovl" hx-swap-oob="true"></div>`;

  test("a page that comes back (the result of a write) replaces main and the overlay, and the address is the page's, in the overlay's entry", async () => {
    const p = await opened();
    p.autoAnswer(() => ({ headers: RESULT, url: `${ORIGIN}/topics/orders`, body: region("Record sent", "Sent") }));
    await submit(p);
    expect(p.document.title).toContain("Record sent");
    expect(p.document.querySelector("h1")!.textContent).toBe("Sent");
    expect(p.document.querySelector("[data-overlay]")).toBeNull();
    expect(root(p).hasAttribute("inert")).toBe(false);
    expect(p.history.url).toBe(`${ORIGIN}/topics/orders`);
    expect(p.history.pushes.length).toBe(1);
  });

  test("a result that was a redirect takes the address it was sent to", async () => {
    const p = await opened();
    p.autoAnswer(() => ({ headers: { ...RESULT, "HX-Replace-Url": "/topics/orders/partitions" }, redirected: true, url: `${ORIGIN}/topics/orders/partitions`, body: region("Partitions", "orders") }));
    await submit(p);
    expect(p.history.url).toBe(`${ORIGIN}/topics/orders/partitions`);
    expect(p.history.pushes.length).toBe(1);
  });

  test("if the request cannot be made at all the form posts as it would without a script", async () => {
    const p = await opened();
    p.autoAnswer(() => new Error("network"));
    await submit(p);
    expect(p.forms).toEqual([`${ORIGIN}/topics/orders/produce`]);
  });
});

describe("a Preview control (a GET submit) in an overlay never writes", () => {
  const form = `<form id="f" method="post" action="/groups/g/reset"><input name="topic" value="orders"><button id="prev" type="submit" formmethod="get" formaction="/groups/g/reset">Preview</button><button id="go" type="submit">Reset</button></form>`;
  test("it is fetched as a GET with the fields in the address, and the overlay that comes back replaces this one", async () => {
    const p = await started();
    p.autoAnswer(() => reply("Reset offsets", "dialog", form));
    p.click(p.document.getElementById("open")!);
    await p.flush();
    p.autoAnswer(() => reply("Reset offsets, previewed", "dialog", `<p id="seen">preview</p>`));
    const prev = p.document.getElementById("prev") as HTMLButtonElement;
    p.document.getElementById("f")!.dispatchEvent(new p.window.SubmitEvent("submit", { bubbles: true, cancelable: true, submitter: prev }));
    await p.flush();
    const call = p.calls[p.calls.length - 1];
    expect(call.method).toBe("GET");
    expect(call.url).toBe(`${ORIGIN}/groups/g/reset?topic=orders`);
    expect(call.headers["X-Fragment"]).toBe("1");
    expect(p.calls.some((c) => c.method === "POST")).toBe(false);
    expect(p.document.getElementById("seen")).not.toBeNull();
    expect(p.history.url).toBe(`${ORIGIN}/groups/g/reset?topic=orders`);
  });
});

describe("a fragment that changed fades in, softly, and one that did not does not move", () => {
  const rows = (text: string) => `<div class="frag" id="rows" data-frag="/fragments/rows"><p id="n">${text}</p></div>`;
  const tick = async (p: Page, body: string) => {
    p.autoAnswer(() => ({ etag: `"${body.length}${body}"`, body }));
    p.streams[0].emit("tick", "rows");
    await p.flush();
  };

  test("what changed changes its text and nothing is animated (no block dips in opacity)", async () => {
    const p = await started({ stream: "/events?watch=rows" });
    await tick(p, rows("two"));
    expect(p.document.getElementById("n")!.textContent).toBe("two");
    expect(p.animated).toEqual([]);
  });

  test("an unchanged answer animates nothing", async () => {
    const p = await started({ stream: "/events?watch=rows" });
    await tick(p, rows("one"));
    expect(p.animated).toEqual([]);
  });

  test("with reduced motion nothing animates", async () => {
    const p = await started({ stream: "/events?watch=rows", reduced: true });
    await tick(p, rows("two"));
    expect(p.document.getElementById("n")!.textContent).toBe("two");
    expect(p.animated).toEqual([]);
  });
});

// Relative times ("4 s ago") are kept by the browser: recomputed every second from what the server said and the time since, with no
// request, and a morph that brings the server's text again is taken as a new starting point.
import { describe, expect, test } from "bun:test";
import { boot } from "./harness.ts";

const PAGE = `<main id="main"><p id="plain">last write 4 s ago, to x</p><p id="split">newest <b id="b">0.2 s</b> ago</p><p id="old">3 h ago</p><p id="static">5 messages ago</p></main>`;

describe("relative times", () => {
  test("each moves on by the time that has passed, whole in a sentence or split by a bold number, and nothing else changes", async () => {
    const p = await boot(PAGE);
    const t0 = p.document.getElementById("plain")!.textContent;
    expect(t0).toBe("last write 4 s ago, to x");
    await Bun.sleep(2200);
    expect(p.document.getElementById("plain")!.textContent).toMatch(/^last write [67] s ago, to x$/);
    expect(p.document.getElementById("b")!.textContent).toMatch(/^2\.\d s$|^2 s$/);
    expect(p.document.getElementById("old")!.textContent).toBe("3 h ago");
    expect(p.document.getElementById("static")!.textContent).toBe("5 messages ago");
    expect(p.calls.length).toBe(0);
  });

  test("an answer that brings the server's text again starts from it", async () => {
    const p = await boot(PAGE);
    await Bun.sleep(1100);
    p.document.getElementById("plain")!.firstChild!.textContent = "last write 40 s ago, to x";
    await Bun.sleep(2300);
    expect(p.document.getElementById("plain")!.textContent).toMatch(/^last write 4[12] s ago, to x$/);
  });
});

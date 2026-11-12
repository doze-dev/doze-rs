// The Live additions of crates/kafka-console/assets/console.js (slice 9e): Pause holds the tape and counts what arrives, Resume
// brings it in, a drawer holds it too, new rows are marked, j and k move between rows, a select filter applies on change.
// Run: `cd tools/dom && bun install && bun test`.

import { describe, expect, test } from "bun:test";
import { boot, type Page } from "./harness.ts";

const tape = (seen: number, rows: number[]) =>
  `<div class="frag" id="live-wire" data-frag="/fragments/live-wire"><span hidden data-seen="${seen}" data-newest="${rows[0] ?? 0}"></span>` +
  `<a class="btn" id="pause" href="/live?until=9" data-pause data-ui="pause">Pause</a>` +
  `<div data-paused-banner hidden>Paused. <span data-held>0</span> requests are held back.</div>` +
  `<form><select id="f" name="client" data-autosubmit><option value="">All</option><option value="a">a</option></select></form>` +
  `<table class="wire"><tbody>${rows.map((r) => `<tr data-row="${r}"><td class="tm"><a href="/live/${r}">r${r}</a></td></tr>`).join("")}</tbody></table></div>`;

const started = (seen = 10, rows = [3, 2, 1]): Promise<Page> => boot(tape(seen, rows), { stream: "/events?watch=live-wire" });
const answer = (p: Page, seen: number, rows: number[]) => {
  p.tick("live-wire");
  return p.flush().then(() => {
    const q = p.pending.find((x) => x.url === "/fragments/live-wire");
    q?.answer({ body: tape(seen, rows) });
    return p.flush();
  });
};
const text = (p: Page, sel: string) => p.document.querySelector(sel)?.textContent;
const rows = (p: Page) => [...p.document.querySelectorAll("tr[data-row]")].map((r) => r.getAttribute("data-row"));

describe("pause", () => {
  test("a click on Pause holds the tape in place, counts what arrives, and Resume brings it in", async () => {
    const p = await started();
    const pause = p.document.getElementById("pause")!;
    p.click(pause);
    expect(pause.textContent).toBe("Resume");
    expect((p.document.querySelector("[data-paused-banner]") as HTMLElement).hidden).toBe(false);
    await answer(p, 14, [7, 6, 5, 4, 3, 2, 1]);
    expect(rows(p)).toEqual(["3", "2", "1"]);
    expect(text(p, "[data-held]")).toBe("4");
    await answer(p, 1250, [9, 8]);
    expect(text(p, "[data-held]")).toBe("1,240");
    expect(rows(p)).toEqual(["3", "2", "1"]);
    p.click(pause);
    await p.flush();
    expect(pause.textContent).toBe("Pause");
    expect(rows(p)).toEqual(["9", "8"]);
    expect((p.document.querySelector("[data-paused-banner]") as HTMLElement).hidden).toBe(true);
  });

  test("a tape that is not paused is swapped as it comes, and the new rows are marked for a moment", async () => {
    const p = await started();
    await answer(p, 12, [5, 4, 3, 2, 1]);
    expect(rows(p)).toEqual(["5", "4", "3", "2", "1"]);
    const marked = [...p.document.querySelectorAll("tr.new")].map((r) => r.getAttribute("data-row"));
    expect(marked).toEqual(["5", "4"]);
    p.runTimers();
    expect(p.document.querySelectorAll("tr.new").length).toBe(0);
  });

  test("a drawer that is open holds the tape without a banner, and closing it lets the tape catch up", async () => {
    const p = await started();
    const drawer = p.document.createElement("div");
    drawer.setAttribute("data-overlay", "");
    p.document.body.append(drawer);
    await answer(p, 20, [8, 7, 3, 2, 1]);
    expect(rows(p)).toEqual(["3", "2", "1"]);
    expect((p.document.querySelector("[data-paused-banner]") as HTMLElement).hidden).toBe(true);
    drawer.remove();
    p.click(p.document.body);
    p.runTimers();
    await p.flush();
    expect(rows(p)).toEqual(["8", "7", "3", "2", "1"]);
  });

  test("Resume on a page that was paused by the server (a plain link) is followed, not intercepted", async () => {
    const p = await boot(`<a id="r" href="/live" data-resume>Resume</a>`);
    const e = new p.window.MouseEvent("click", { bubbles: true, cancelable: true });
    p.document.getElementById("r")!.dispatchEvent(e);
    expect(e.defaultPrevented).toBe(false);
  });
});

describe("the keyboard", () => {
  test("j and k move the focus between the rows' links and stay inside the list", async () => {
    const p = await started();
    const key = (k: string, target: Element = p.document.body) => target.dispatchEvent(new p.window.KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true }));
    key("j");
    expect(p.document.activeElement?.textContent).toBe("r3");
    key("j");
    key("j");
    key("j");
    expect(p.document.activeElement?.textContent).toBe("r1");
    key("k");
    expect(p.document.activeElement?.textContent).toBe("r2");
    key("k");
    key("k");
    expect(p.document.activeElement?.textContent).toBe("r3");
  });

  test("typing j in a field is typing", async () => {
    const p = await boot(`<input id="q"><table class="wire"><tbody><tr data-row="1"><td class="tm"><a href="/live/1">r1</a></td></tr></tbody></table>`);
    const q = p.document.getElementById("q")!;
    const e = new p.window.KeyboardEvent("keydown", { key: "j", bubbles: true, cancelable: true });
    q.dispatchEvent(e);
    expect(e.defaultPrevented).toBe(false);
    expect(p.document.activeElement?.tagName).not.toBe("A");
  });
});

describe("filters", () => {
  test("a select that is marked applies its form when it changes", async () => {
    const p = await started();
    p.document.getElementById("f")!.dispatchEvent(new p.window.Event("change", { bubbles: true }));
    expect(p.forms.length).toBe(1);
  });
});

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
    // (a page with no htmx boost: here the question is only whether OUR script takes the click)
    const p = await boot(`<a id="r" href="/live" data-resume>Resume</a>`, { plain: true });
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
    // The form is submitted through its submit event (requestSubmit), which is what htmx (or the browser) answers.
    const form = p.document.getElementById("f")!.closest("form") as HTMLFormElement;
    let asked = 0;
    form.requestSubmit = () => void asked++;
    p.document.getElementById("f")!.dispatchEvent(new p.window.Event("change", { bubbles: true }));
    expect(asked).toBe(1);
  });
});

describe("a Live drawer opened by the overlay system holds the tape", () => {
  const page = (seen: number, rows: number[]) =>
    `<div class="root"><main id="main">${tape(seen, rows)}<a id="open" class="lnk" href="/live/3?client=a" data-dialog>r3</a></main></div>`;
  const drawer = `<div class="ovl ovl-drawer root" data-overlay data-close="/live?client=a"><a class="scrim" href="/live?client=a" tabindex="-1" aria-hidden="true" data-overlay-close></a>` +
    `<div class="drawer-panel" role="dialog" aria-modal="true" aria-labelledby="t" tabindex="-1"><h2 id="t">Fetch v12</h2><a class="btn" id="close" href="/live?client=a" data-dialog-close>Close</a></div></div>`;

  test("while the real drawer is open what comes is kept aside, and closing it (Esc, the control, Back) lets the tape catch up", async () => {
    for (const how of ["esc", "close", "back"]) {
      const p = await boot(page(10, [3, 2, 1]), { stream: "/events?watch=live-wire" });
      p.autoAnswer(() => ({ headers: { "x-overlay": "1" }, body: drawer }));
      p.click(p.document.getElementById("open")!);
      await p.flush();
      expect(p.document.querySelector("[data-overlay]")).not.toBeNull();
      expect(p.history.pushes.length).toBe(1);
      p.autoAnswer(null as never); // the tape's own fetch stays pending until the test answers it
      await answer(p, 20, [8, 7, 3, 2, 1]);
      expect(rows(p)).toEqual(["3", "2", "1"]);
      expect((p.document.querySelector("[data-paused-banner]") as HTMLElement).hidden).toBe(true);
      if (how === "esc") {
        const e = new p.window.KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
        (p.document.activeElement ?? p.document.body).dispatchEvent(e);
      } else if (how === "close") {
        p.click(p.document.getElementById("close")!);
      } else {
        p.history.back();
      }
      p.runTimers();
      await p.flush();
      p.runTimers();
      await p.flush();
      if (p.document.querySelector("[data-overlay]")) throw new Error("still open after " + how);
      expect(rows(p), how).toEqual(["8", "7", "3", "2", "1"]);
    }
  });

  test("a server-rendered drawer (a direct visit) holds the tape from the start", async () => {
    const p = await boot(page(10, [3, 2, 1]) + drawer, { stream: "/events?watch=live-wire" });
    await answer(p, 20, [8, 7, 3, 2, 1]);
    expect(rows(p)).toEqual(["3", "2", "1"]);
  });

  test("j and k do not move the focus behind an open drawer", async () => {
    const p = await boot(page(10, [3, 2, 1]), { stream: "/events?watch=live-wire" });
    p.autoAnswer(() => ({ headers: { "x-overlay": "1" }, body: drawer }));
    p.click(p.document.getElementById("open")!);
    await p.flush();
    const before = p.document.activeElement;
    const e = new p.window.KeyboardEvent("keydown", { key: "j", bubbles: true, cancelable: true });
    (p.document.activeElement ?? p.document.body).dispatchEvent(e);
    expect(p.document.activeElement).toBe(before);
    expect(p.document.querySelector("table.wire a:focus")).toBeNull();
  });
});

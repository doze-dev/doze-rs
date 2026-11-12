import { describe, expect, test } from "bun:test";
import { describe as say } from "../lib/compare.ts";
import { type BoardFile } from "../lib/boards.ts";
import { boards, boardSig, diff, failing, passing, render, routes } from "./helpers.ts";
import { compare } from "../lib/compare.ts";
import { signature } from "../board-signature.ts";
import { renderBoard } from "./render-board.ts";

// Every mutation is applied to every board it can apply to (not a sample), because that is cheap; each must be caught
// with the right message, or (for the passing ones) not be seen at all.
const MIN_APPLIED: Record<string, number> = {
  "delete a button": 60,
  "rename a button": 60,
  "change a data-op": 40,
  "delete a table column": 30,
  "drop a card's dt": 15,
  "remove a tab": 8,
  "add a link to another screen": 90,
  "change a state class (st ok -> st bad)": 8,
  "remove the h1": 60,
  "change the h1 text": 60,
};

describe("mutations the gate must catch", () => {
  for (const m of failing) {
    test(m.name, () => {
      let applied = 0;
      const misses: string[] = [];
      for (const b of boards) {
        const html = m.apply(render(b), b);
        if (html === null) continue;
        applied++;
        const d = diff(b, html, "strict");
        if (d.length === 0) {
          misses.push(`${b.id}: not caught`);
          continue;
        }
        for (const want of m.expect) {
          if (!d.some((x) => x.kind === want.kind && x.field === want.field)) misses.push(`${b.id}: expected ${want.kind} ${want.field}, got ${d.map(say).join("; ")}`);
        }
      }
      expect(misses).toEqual([]);
      expect(applied, `${m.name} applied to only ${applied} boards`).toBeGreaterThanOrEqual(MIN_APPLIED[m.name] ?? 20);
    });
  }

  test("the sample is at least 20 boards wide for every mutation that has 20 candidates", () => {
    const sample = boards.filter((_, i) => i % 4 === 0);
    expect(sample.length).toBeGreaterThanOrEqual(20);
  });

  test("the live (structure-only) comparison catches the structural mutations too", () => {
    const structural = failing.filter((m) => ["delete a table column", "remove a tab", "add a link to another screen", "remove the h1", "change a data-op"].includes(m.name));
    for (const m of structural) {
      let caught = 0;
      let applied = 0;
      for (const b of boards) {
        const html = m.apply(render(b), b);
        if (html === null) continue;
        applied++;
        if (diff(b, html, "live").length > 0) caught++;
      }
      expect(caught, m.name).toBe(applied);
    }
  });
});

describe("changes the gate must NOT see", () => {
  for (const m of passing) {
    test(m.name, () => {
      let applied = 0;
      const seen: string[] = [];
      for (const b of boards) {
        const html = m.apply(render(b), b);
        if (html === null) continue;
        applied++;
        for (const mode of ["strict", "live"] as const) {
          const d = diff(b, html, mode);
          if (d.length) seen.push(`${b.id} (${mode}): ${d.map(say).join("; ")}`);
        }
      }
      expect(seen).toEqual([]);
      expect(applied, `${m.name} applied to only ${applied} boards`).toBeGreaterThanOrEqual(m.name === "add a scaffold row" ? 90 : 30);
    });
  }

  test("changing a number inside [data-vary] is invisible", () => {
    // The boards carry no data-vary yet, so put one into 24 boards (a number at the end of the first heading or button)
    // and change it on the page only.
    let n = 0;
    for (const b of boards) {
      const marked = withVary(b, "12");
      if (!marked) continue;
      n++;
      const page = renderBoard(marked, routes).replace(/(<[^>]*data-vary[^>]*>)12(<)/, "$198$2");
      expect(page).not.toBe(renderBoard(marked, routes));
      expect(compare(boardSig(marked), signature(page, { routes }), "strict").map(say), b.id).toEqual([]);
      // ... and without data-vary the same change IS seen (the test is not vacuous).
      const plain = renderBoard(marked, routes).replace(/(<[^>]*data-vary[^>]*>)12(<)/, "$198$2").replace(/ data-vary/g, "");
      expect(compare(boardSig(marked), signature(plain, { routes }), "strict").length, b.id).toBeGreaterThan(0);
      if (n >= 24) break;
    }
    expect(n).toBeGreaterThanOrEqual(20);
  });
});

/** The board with `<span data-vary>12</span>` appended to its first h2/h1 (or first button label). */
function withVary(b: BoardFile, num: string): BoardFile | null {
  const span = `<span data-vary>${num}</span>`;
  let done = false;
  const html = new HTMLRewriter()
    .on("h2, h1, .btn, button", {
      element(e) {
        if (done) return;
        done = true;
        e.onEndTag((t) => t.before(span, { html: true }));
      },
    })
    .transform(b.html);
  return done ? { ...b, html } : null;
}

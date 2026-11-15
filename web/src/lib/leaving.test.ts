import { describe, expect, test } from "bun:test";
import { createLeaving, unlessLeaving } from "./leaving";

const tick = () => new Promise((r) => setTimeout(r, 0));

/** Timers run by hand: `fire()` runs the pending one. */
function manual() {
  let pending: (() => void) | null = null;
  return {
    set: (fn: () => void) => {
      pending = fn;
      return 1;
    },
    clear: () => {
      pending = null;
    },
    fire: () => pending?.(),
    get pending() {
      return pending !== null;
    },
  };
}

describe("a page being left (J500)", () => {
  test("a page not being left lets a request go at once", async () => {
    const l = createLeaving(3000, manual());
    expect(l.leaving()).toBe(false);
    await l.untilStaying();
  });

  test("a request asked for while leaving waits, and goes when the page stays", async () => {
    const l = createLeaving(3000, manual());
    l.leave();
    let went = false;
    void l.untilStaying().then(() => (went = true));
    await tick();
    expect(went).toBe(false);
    l.stay();
    await tick();
    expect(went).toBe(true);
    expect(l.leaving()).toBe(false);
  });

  test("a page still here when the wait is over is staying: the held requests go", async () => {
    const t = manual();
    const l = createLeaving(3000, t);
    l.leave();
    let went = 0;
    void l.untilStaying().then(() => went++);
    void l.untilStaying().then(() => went++);
    expect(t.pending).toBe(true);
    t.fire();
    await tick();
    expect(went).toBe(2);
    expect(l.leaving()).toBe(false);
  });

  test("with the real timers too", async () => {
    const l = createLeaving(10);
    l.leave();
    let went = false;
    void l.untilStaying().then(() => (went = true));
    await tick();
    expect(went).toBe(false);
    await new Promise((r) => setTimeout(r, 30));
    expect(went).toBe(true);
  });

  test("an abort while held rejects with the abort, and the request never goes", async () => {
    const l = createLeaving(3000, manual());
    l.leave();
    const ctrl = new AbortController();
    const p = l.untilStaying(ctrl.signal);
    ctrl.abort();
    await expect(p).rejects.toHaveProperty("name", "AbortError");
    l.stay();
  });

  test("a chunk load cut by the leaving waits, and loads again when the page stays; any other failure is thrown", async () => {
    const l = createLeaving(3000, manual());
    let calls = 0;
    const load = unlessLeaving(async () => {
      calls++;
      if (calls === 1) {
        l.leave();
        throw new TypeError("error loading dynamically imported module");
      }
      return { Page: "ok" };
    }, l);
    let got: unknown;
    void load().then((m) => (got = m));
    await tick();
    expect(calls).toBe(1);
    expect(got).toBeUndefined();
    l.stay();
    await tick();
    expect(calls).toBe(2);
    expect(got).toEqual({ Page: "ok" });
    const broken = unlessLeaving(() => Promise.reject(new Error("no such chunk")), l);
    await expect(broken()).rejects.toThrow("no such chunk");
  });

  test("an aborted signal rejects at once, held or not", async () => {
    const l = createLeaving(3000, manual());
    const ctrl = new AbortController();
    ctrl.abort();
    await expect(l.untilStaying(ctrl.signal)).rejects.toHaveProperty("name", "AbortError");
  });
});

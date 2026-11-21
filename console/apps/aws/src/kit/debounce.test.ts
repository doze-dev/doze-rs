import { describe, expect, test } from "bun:test";
import { debounce } from "./debounce";

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

describe("debounce", () => {
  test("runs once, with the last argument, after the pause", async () => {
    const seen: number[] = [];
    const d = debounce((n: number) => seen.push(n), 20);
    d.call(1);
    d.call(2);
    d.call(3);
    expect(seen).toEqual([]);
    expect(d.pending()).toBe(true);
    await wait(50);
    expect(seen).toEqual([3]);
    expect(d.pending()).toBe(false);
  });
  test("a call after the pause is a new run", async () => {
    const seen: number[] = [];
    const d = debounce((n: number) => seen.push(n), 10);
    d.call(1);
    await wait(30);
    d.call(2);
    await wait(30);
    expect(seen).toEqual([1, 2]);
  });
  test("flush runs it now, once; cancel drops it", async () => {
    const seen: string[] = [];
    const d = debounce((s: string) => seen.push(s), 20);
    d.call("a");
    d.flush();
    expect(seen).toEqual(["a"]);
    d.call("b");
    d.cancel();
    await wait(40);
    expect(seen).toEqual(["a"]);
    d.flush();
    expect(seen).toEqual(["a"]);
  });
});

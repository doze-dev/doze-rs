import { describe, expect, test } from "bun:test";
import { compare, describe as say } from "../lib/compare.ts";
import { signature } from "../board-signature.ts";
import { renderBoard, paramRoutes } from "./render-board.ts";
import { boards, boardSig, diff, render, routes } from "./helpers.ts";

describe("identity: a board rendered into the page shell diffs clean against its own snapshot", () => {
  test("all 96 boards, fixture (strict) comparison", () => {
    const bad: string[] = [];
    for (const b of boards) {
      const d = diff(b, render(b), "strict");
      if (d.length) bad.push(`${b.id}: ${d.map(say).join("; ")}`);
    }
    expect(bad).toEqual([]);
  });

  test("all 96 boards, live (structure only) comparison", () => {
    const bad: string[] = [];
    for (const b of boards) {
      const d = diff(b, render(b), "live");
      if (d.length) bad.push(`${b.id}: ${d.map(say).join("; ")}`);
    }
    expect(bad).toEqual([]);
  });

  test("with parameter routes and a base prefix, links still resolve to the board's screen ids", () => {
    const pr = paramRoutes(boards);
    for (const b of boards) {
      const html = renderBoard(b, pr, { base: "/console" });
      const d = compare(boardSig(b), signature(html, { routes: pr, base: "/console" }), "strict");
      expect(d.map(say), b.id).toEqual([]);
    }
  });

  test("without routes the same pages differ exactly in their links", () => {
    const b = boards.find((x) => x.id === "topic-detail")!;
    const d = compare(boardSig(b), signature(render(b), {}), "strict");
    expect(d.length).toBeGreaterThan(0);
    expect(new Set(d.map((x) => x.field))).toEqual(new Set(["link", "button"]));
  });

  test("the renders are stable (the same board renders to the same page)", () => {
    for (const b of boards.slice(0, 10)) expect(render(b)).toBe(renderBoard(b, routes));
  });
});

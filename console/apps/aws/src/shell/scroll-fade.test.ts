import { expect, test } from "bun:test";
import { FADE, fadeOf } from "./scroll-fade";

test("a box that does not scroll fades nothing", () => {
  expect(fadeOf({ scrollTop: 0, scrollHeight: 600, clientHeight: 600 })).toEqual({ top: 0, bottom: 0 });
  expect(fadeOf({ scrollTop: 0, scrollHeight: 600.5, clientHeight: 600 })).toEqual({ top: 0, bottom: 0 });
});

test("at the top only the bottom edge has more past it; at the end only the top", () => {
  expect(fadeOf({ scrollTop: 0, scrollHeight: 1000, clientHeight: 800 })).toEqual({ top: 0, bottom: FADE });
  expect(fadeOf({ scrollTop: 200, scrollHeight: 1000, clientHeight: 800 })).toEqual({ top: FADE, bottom: 0 });
});

test("an edge fades by how little is left past it, never by more than the fade", () => {
  expect(fadeOf({ scrollTop: 10, scrollHeight: 1000, clientHeight: 800 })).toEqual({ top: 10, bottom: FADE });
  expect(fadeOf({ scrollTop: 190, scrollHeight: 1000, clientHeight: 800 })).toEqual({ top: FADE, bottom: 10 });
});

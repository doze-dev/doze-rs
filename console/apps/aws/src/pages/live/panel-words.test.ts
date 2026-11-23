import { expect, test } from "bun:test";
import { waitingHeadline } from "./panel-words";

test("a queue's sentence gives its waiting count as the headline and keeps the rest", () => {
  expect(waitingHeadline("4 waiting · 1 in flight")).toEqual({ count: "4", rest: "1 in flight" });
  expect(waitingHeadline("1,204 waiting · 0 in flight · 3 delayed")).toEqual({ count: "1,204", rest: "0 in flight · 3 delayed" });
  expect(waitingHeadline("0 waiting")).toEqual({ count: "0", rest: "" });
});

test("any other sentence has no headline", () => {
  expect(waitingHeadline("1 dead letter · oldest 14 min")).toBeNull();
  expect(waitingHeadline(null)).toBeNull();
});

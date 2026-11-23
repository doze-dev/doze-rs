import { expect, test } from "bun:test";
import { waitBefore } from "./queue-model";

test("a period before a noun is hyphenated, as the boards write it", () => {
  expect(waitBefore(345_600)).toBe("4-day");
  expect(waitBefore(86_400)).toBe("1-day");
  expect(waitBefore(3_600)).toBe("1-hour");
  expect(waitBefore(30)).toBe("30-second");
  expect(waitBefore(90)).toBe("90-second");
});

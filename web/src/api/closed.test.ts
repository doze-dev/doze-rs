import { expect, test } from "bun:test";
import { CLOSED_TO_NETWORK, isClosed } from "./closed";

test("only the door's refusal closes the app (SEC-250)", () => {
  expect(isClosed({ code: CLOSED_TO_NETWORK, message: "This broker now has ACLs" })).toBe(true);
  expect(isClosed({ code: "FORBIDDEN", message: "x" })).toBe(false);
  expect(isClosed({ code: CLOSED_TO_NETWORK })).toBe(false);
  expect(isClosed(null)).toBe(false);
  expect(isClosed("CONSOLE_CLOSED_TO_NETWORK")).toBe(false);
});

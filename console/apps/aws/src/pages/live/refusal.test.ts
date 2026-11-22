import { expect, test } from "bun:test";
import { whatWouldPass } from "./refusal";

test("a code says what would pass", () => {
  expect(whatWouldPass({ error_code: "InvalidParameterValue", status: 400 })).toContain("Change the one the message names");
  expect(whatWouldPass({ error_code: "AWS.SimpleQueueService.NonExistentQueue", status: 400 })).toContain("Create it first");
  expect(whatWouldPass({ error_code: "QueueNameExists", status: 400 })).toContain("name is taken");
  expect(whatWouldPass({ error_code: "ThrottlingException", status: 400 })).toContain("Send fewer");
});
test("an unknown code still gets a sentence, a 404 the one for a missing thing", () => {
  expect(whatWouldPass({ error_code: "Weird", status: 400 })).toContain("service's words");
  expect(whatWouldPass({ error_code: null, status: 404 })).toContain("Create it first");
});

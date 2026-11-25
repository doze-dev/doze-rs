import { expect, test } from "bun:test";
import { mentions } from "../../../api/forget";
import { keyOf, USERS } from "./api";

test("a key's last use belongs to its user, so deleting the user does not read it again (IAM would answer 404)", () => {
  const key = keyOf(USERS, "GetAccessKeyLastUsed", { AccessKeyId: "AKIAQ4HZLARDER7DEV2K" }, "dev");
  expect(mentions(key, "dev")).toBe(true);
  expect(mentions(key, "tester")).toBe(false);
  // Without an owner the read names only the key's id: a delete of the user cannot find it.
  expect(mentions(keyOf(USERS, "GetAccessKeyLastUsed", { AccessKeyId: "AKIAQ4HZLARDER7DEV2K" }), "dev")).toBe(false);
});

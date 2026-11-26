import { describe, expect, test } from "bun:test";
import type { ResourceRow } from "../../../api/types/ResourceRow";
import { kmsHint, kmsKeys, whyNot } from "./kms-keys";

const key = (id: string, cells: (string | null)[]): ResourceRow => ({ name: id, label: id, id, cells, link: null });
const alias = (name: string, id: string): ResourceRow => ({ name, label: name, id: name, cells: [id, null], link: null });
const app = key("k1", ["app", "Enabled", "SYMMETRIC_DEFAULT", "ENCRYPT_DECRYPT", "Customer", null, null]);
const signing = key("k2", ["signing", "Enabled", "RSA_2048", "SIGN_VERIFY", "Customer", null, null]);
const aliases = [alias("alias/larder-app", "k1"), alias("alias/larder-signing", "k2")];

describe("the KMS keys S3 can use", () => {
  test("a key is chosen by its alias's ARN, and a signing key is left out with its reason", () => {
    const k = kmsKeys([app, signing], aliases, "us-east-1", "111122223333");
    expect(k.usable).toEqual([{ value: "arn:aws:kms:us-east-1:111122223333:alias/larder-app", label: "alias/larder-app", hint: "app" }]);
    expect(k.unusable).toEqual([{ label: "alias/larder-signing", why: "is RSA_2048 for signing" }]);
    expect(kmsHint(k)).toBe("Only a symmetric encryption key in the bucket's Region can be used, and the console lists the first 100. alias/larder-app qualifies; alias/larder-signing is RSA_2048 for signing, so S3 cannot use it.");
  });
  test("a key with no alias is chosen by its own ARN, and a disabled one is not usable", () => {
    const k = kmsKeys([app, key("k3", ["old", "Disabled", "SYMMETRIC_DEFAULT", "ENCRYPT_DECRYPT", "Customer", null, null])], [], "eu-west-1", "1");
    expect(k.usable[0]?.value).toBe("arn:aws:kms:eu-west-1:1:key/k1");
    expect(k.unusable).toEqual([{ label: "k3", why: "is disabled" }]);
  });
  test("an AWS managed key is not offered", () => {
    expect(kmsKeys([key("k4", ["", "Enabled", "SYMMETRIC_DEFAULT", "ENCRYPT_DECRYPT", "AWS", null, null])], [], "us-east-1", "1").usable).toEqual([]);
    expect(whyNot(["", "Enabled", "SYMMETRIC_DEFAULT", "ENCRYPT_DECRYPT", "Customer"])).toBeNull();
  });
});

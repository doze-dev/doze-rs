import { describe, expect, test } from "bun:test";
import { policyExamples } from "./policy-examples";

describe("bucket policy examples", () => {
  const all = policyExamples("product-images", "111122223333");
  test("each is a policy document for this bucket and account", () => {
    expect(all.length).toBeGreaterThanOrEqual(3);
    for (const e of all) {
      const doc = JSON.parse(e.policy) as { Version: string; Statement: { Resource: string | string[] }[] };
      expect(doc.Version).toBe("2012-10-17");
      expect(JSON.stringify(doc.Statement[0]?.Resource)).toContain("arn:aws:s3:::product-images");
    }
    expect(all[0]?.policy).toContain("arn:aws:iam::111122223333:role/lambda-exec");
  });
  test("the ids are distinct", () => {
    expect(new Set(all.map((e) => e.id)).size).toBe(all.length);
  });
});

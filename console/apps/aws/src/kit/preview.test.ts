import { expect, test } from "bun:test";
import type { Preview } from "./contract";
import { keepSameOperation, previewKey, sameOperation } from "./preview";

const key = (operation: string, input: unknown = {}) => previewKey({ service: "logs", operation, input });
const old: Preview = { json: { logGroupName: "g", retentionInDays: 7 }, cli: "aws logs put-retention-policy", boto3: "logs.put_retention_policy()", missing: [] };

test("two requests are for one operation when the service and the operation are the same, whatever the input", () => {
  expect(sameOperation(key("PutRetentionPolicy", { retentionInDays: 7 }), key("PutRetentionPolicy", { retentionInDays: 14 }))).toBe(true);
  expect(sameOperation(key("PutRetentionPolicy"), key("DeleteRetentionPolicy"))).toBe(false);
  expect(sameOperation(previewKey({ service: "sqs", operation: "PutRetentionPolicy", input: {} }), key("PutRetentionPolicy"))).toBe(false);
  expect(sameOperation(undefined, key("PutRetentionPolicy"))).toBe(false);
  expect(sameOperation("not json", key("PutRetentionPolicy"))).toBe(false);
});

test("while the next preview is on its way the last body stays only for the same operation: another operation's body would sit under the new name", () => {
  const before = { queryKey: ["preview", key("PutRetentionPolicy", { retentionInDays: 7 })] };
  expect(keepSameOperation(key("PutRetentionPolicy", { retentionInDays: 14 }))(old, before)).toBe(old);
  expect(keepSameOperation(key("DeleteRetentionPolicy"))(old, before)).toBeUndefined();
  expect(keepSameOperation(key("DeleteRetentionPolicy"))(undefined, undefined)).toBeUndefined();
});

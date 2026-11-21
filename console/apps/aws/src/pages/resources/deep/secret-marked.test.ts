import { describe, expect, test } from "bun:test";
import type { FieldValue } from "../../../api/types/FieldValue";
import { markedAt } from "./secret-marked";

const field = (label: string, value: string | null): FieldValue => ({ label, value, format: "time", hidden: false, link: null });

describe("a secret marked for deletion", () => {
  test("is told by the mark's value", () => {
    expect(markedAt({ fields: [field("ARN", "arn"), field("Marked for deletion", "1795268221.976")] })).toBe("1795268221.976");
  });
  test("a secret with no mark is not", () => {
    expect(markedAt({ fields: [field("Marked for deletion", null)] })).toBeNull();
    expect(markedAt({ fields: [] })).toBeNull();
  });
});

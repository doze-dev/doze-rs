import { describe, expect, test } from "bun:test";
import type { ResourceRow } from "../../../api/types/ResourceRow";
import { tagsHint, versioningHint } from "./create-hints";

const row = (name: string, cells: (string | null)[]): ResourceRow => ({ name, label: name, id: name, cells, link: null });

describe("what Create bucket says of the account's own buckets", () => {
  test("the bucket that turned versioning on, and where its events go", () => {
    const rows = [row("receipts", ["us-east-1", null, "Disabled", null, null, null, null]), row("product-images", ["us-east-1", null, "Enabled", "resize-product-image", null, null, "s3:ObjectCreated:*"])];
    expect(versioningHint(rows)).toBe("Versioning is off by default. product-images turned it on; the ObjectCreated notification to resize-product-image fires for each new version.");
    expect(versioningHint([row("a", ["us-east-1", null, "Enabled"])])).toBe("Versioning is off by default. a turned it on.");
    expect(versioningHint([rows[0]!])).toBeNull();
  });
  test("the tag keys of the first bucket with tags", () => {
    expect(tagsHint([{ bucket: "a", keys: [] }, { bucket: "product-images", keys: ["app", "owner"] }])).toBe("product-images uses app and owner.");
    expect(tagsHint([])).toBeNull();
  });
});

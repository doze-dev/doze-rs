import { describe, expect, test } from "bun:test";
import type { ResourceRow } from "../../../api/types/ResourceRow";
import { ownedRefusal, startSentence, tagsHint, versioningHint } from "./create-hints";

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

describe("what Create bucket says when it must not claim to have made a bucket", () => {
  test("a sentence that follows a full stop starts with a capital", () => {
    expect(startSentence("versioning on, all four Block Public Access settings on, nothing in it yet")).toBe("Versioning on, all four Block Public Access settings on, nothing in it yet");
    expect(startSentence("")).toBe("");
  });
  test("a name that is one of the account's own buckets is refused before any call, in words that say why", () => {
    const r = ownedRefusal("thumbnails");
    expect(r.why).toBe("thumbnails is already your bucket. In us-east-1 S3 answers CreateBucket for a bucket you own with 200 and changes nothing, so nothing was sent.");
    expect(r.suggestion).toBe("Choose another name, or open it from the list.");
  });
});

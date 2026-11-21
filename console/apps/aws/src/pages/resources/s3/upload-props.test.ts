import { describe, expect, test } from "bun:test";
import { NO_PROPS, uploadProps } from "./upload-props";

describe("upload properties", () => {
  test("what is left at its default is left out", () => {
    expect(uploadProps(NO_PROPS)).toEqual({});
  });
  test("each property becomes the member of PutObject it is", () => {
    expect(
      uploadProps({ storageClass: "STANDARD_IA", encryption: "aws:kms", kmsKey: "alias/larder-app", checksum: "SHA256", tags: [{ key: "sku", value: "a b" }, { key: "", value: "x" }], metadata: [{ key: "x-amz-meta-source", value: "catalog" }, { key: "Cache-Control", value: "max-age=60" }, { key: "owner", value: "me" }] }),
    ).toEqual({ StorageClass: "STANDARD_IA", ServerSideEncryption: "aws:kms", SSEKMSKeyId: "alias/larder-app", ChecksumAlgorithm: "SHA256", Tagging: "sku=a%20b", CacheControl: "max-age=60", Metadata: { source: "catalog", owner: "me" } });
  });
});

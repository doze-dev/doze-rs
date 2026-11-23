import { describe, expect, test } from "bun:test";
import { aclChoices, type CopyForm, sourceMetadata, specifyProps } from "./copy-props";

const form: CopyForm = { metadata: "copy", metaRows: [], storageClass: "STANDARD", sourceClass: "STANDARD", acl: "", tags: [], encryption: "bucket", kmsKey: "", checksum: "" };

describe("specifyProps", () => {
  test("a form left as it started keeps the metadata, replaces the tags with the source's and sends nothing else", () => {
    expect(specifyProps({ ...form, tags: [{ key: "sku", value: "FV-TOM-V" }] })).toEqual({ MetadataDirective: "COPY", TaggingDirective: "REPLACE", Tagging: "sku=FV-TOM-V" });
    expect(specifyProps(form)).toEqual({ MetadataDirective: "COPY", TaggingDirective: "REPLACE" });
  });

  test("replacing the metadata sends the rows: the type, the system headers and the user keys", () => {
    const p = specifyProps({ ...form, metadata: "replace", metaRows: [{ key: "Content-Type", value: "text/plain" }, { key: "Cache-Control", value: "max-age=60" }, { key: "x-amz-meta-source", value: "catalog" }, { key: "owner", value: "web" }] });
    expect(p).toEqual({ MetadataDirective: "REPLACE", TaggingDirective: "REPLACE", ContentType: "text/plain", CacheControl: "max-age=60", Metadata: { source: "catalog", owner: "web" } });
  });

  test("metadata rows are not sent while the metadata is copied", () => {
    expect(specifyProps({ ...form, metaRows: [{ key: "owner", value: "web" }] })).not.toHaveProperty("Metadata");
  });

  test("a storage class is sent only when it is not the source's", () => {
    expect(specifyProps({ ...form, sourceClass: "GLACIER_IR", storageClass: "GLACIER_IR" })).not.toHaveProperty("StorageClass");
    expect(specifyProps({ ...form, storageClass: "STANDARD_IA" }).StorageClass).toBe("STANDARD_IA");
    // Back to Standard from another class is a change too.
    expect(specifyProps({ ...form, sourceClass: "GLACIER_IR", storageClass: "STANDARD" }).StorageClass).toBe("STANDARD");
  });

  test("an ACL, an encryption with its key and a checksum are sent when chosen", () => {
    const p = specifyProps({ ...form, acl: "authenticated-read", encryption: "aws:kms", kmsKey: "alias/larder-app", checksum: "SHA256" });
    expect(p).toMatchObject({ ACL: "authenticated-read", ServerSideEncryption: "aws:kms", SSEKMSKeyId: "alias/larder-app", ChecksumAlgorithm: "SHA256" });
  });
});

describe("sourceMetadata", () => {
  test("the system headers by name, then the user keys with their prefix", () => {
    expect(sourceMetadata({ ContentType: "image/jpeg", CacheControl: "no-cache", Metadata: { source: "catalog" } })).toEqual([{ key: "Content-Type", value: "image/jpeg" }, { key: "Cache-Control", value: "no-cache" }, { key: "x-amz-meta-source", value: "catalog" }]);
    expect(sourceMetadata(undefined)).toEqual([]);
  });
});

describe("aclChoices", () => {
  const why = (enforced: boolean, block: boolean) => Object.fromEntries(aclChoices(enforced, block).map((a) => [a.value, a.why ?? null]));
  test("ownership enforced takes only the owner's full control", () => {
    const w = why(true, false);
    expect(w["bucket-owner-full-control"]).toBeNull();
    expect(w.private).toContain("disabled");
    expect(w["public-read"]).toContain("disabled");
  });
  test("with ACLs enabled, blocking public ACLs leaves the public ones out", () => {
    const w = why(false, true);
    expect(w.private).toBeNull();
    expect(w["authenticated-read"]).toContain("blocks public");
    expect(w["public-read"]).toContain("blocks public");
  });
  test("with ACLs enabled and nothing blocked, every one is offered", () => {
    expect(Object.values(why(false, false)).every((x) => x === null)).toBe(true);
  });
});

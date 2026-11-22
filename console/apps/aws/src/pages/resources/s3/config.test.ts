import { describe, expect, test } from "bun:test";
import { aclRows, BPA_ALL, bpaOf, bpaRows, encryptionConfig, encryptionOf, lifecycleProblem, lifecycleRule, lifecycleRules, managementSentence, notificationRules, ownershipOf, permissionsSentence, propertiesSentence, websiteConfig, websiteOf, withNotification, withRule, type LifecycleForm } from "./config";

const FORM: LifecycleForm = { name: "prune-old-versions", allObjects: false, prefix: "catalog/", tags: [], min: "", max: "", expireDays: "", noncurrentDays: "30", keepNewer: "", abortDays: "", deleteMarkers: false };

describe("S3 settings", () => {
  test("Block Public Access reads as the master switch and the four settings", () => {
    const rows = bpaRows(bpaOf({ PublicAccessBlockConfiguration: { ...BPA_ALL, BlockPublicPolicy: false } }));
    expect(rows.map((r) => [r.setting, r.on])).toEqual([["Block all public access", false], ["BlockPublicAcls", true], ["IgnorePublicAcls", true], ["BlockPublicPolicy", false], ["RestrictPublicBuckets", true]]);
    expect(bpaRows(bpaOf({ PublicAccessBlockConfiguration: BPA_ALL }))[0]?.on).toBe(true);
    expect(bpaOf({}).BlockPublicAcls).toBe(false);
  });

  test("encryption and ownership are read and written as S3 takes them", () => {
    const answer = { ServerSideEncryptionConfiguration: { Rules: [{ ApplyServerSideEncryptionByDefault: { SSEAlgorithm: "aws:kms", KMSMasterKeyID: "arn:k" }, BucketKeyEnabled: true }] } };
    expect(encryptionOf(answer)).toEqual({ algorithm: "aws:kms", key: "arn:k", bucketKey: true });
    expect(encryptionConfig("aws:kms", "arn:k", true)).toEqual(answer.ServerSideEncryptionConfiguration);
    expect(encryptionConfig("AES256", "ignored", true)).toEqual({ Rules: [{ ApplyServerSideEncryptionByDefault: { SSEAlgorithm: "AES256" } }] });
    expect(encryptionConfig("aws:kms:dsse", "k", true)).toEqual({ Rules: [{ ApplyServerSideEncryptionByDefault: { SSEAlgorithm: "aws:kms:dsse", KMSMasterKeyID: "k" } }] });
    expect(ownershipOf({ OwnershipControls: { Rules: [{ ObjectOwnership: "BucketOwnerEnforced" }] } })).toBe("BucketOwnerEnforced");
    expect(ownershipOf({})).toBeNull();
    // doze-aws names the owner "doze" on the grant too: the grant to the bucket's owner is the owner, whatever it is called.
    expect(aclRows({ Owner: { DisplayName: "doze", ID: "x" }, Grants: [{ Grantee: { DisplayName: "doze", ID: "x", Type: "CanonicalUser" }, Permission: "FULL_CONTROL" }] })).toEqual([{ grantee: "Bucket owner (your AWS account)", permission: "FULL_CONTROL" }]);
    expect(aclRows({ Owner: { ID: "x" }, Grants: [{ Grantee: { ID: "x", Type: "CanonicalUser" }, Permission: "FULL_CONTROL" }] })).toEqual([{ grantee: "Bucket owner (your AWS account)", permission: "FULL_CONTROL" }]);
  });

  test("the permissions sentence names who the one statement lets in", () => {
    const policy = JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Principal: { AWS: "arn:aws:iam::123456789012:role/lambda-exec" }, Action: "s3:GetObject", Resource: "x" }] });
    expect(permissionsSentence("product-images", BPA_ALL, policy)).toBe("product-images is private: all four Block Public Access settings are on, and its one policy statement lets the role lambda-exec read objects.");
    expect(permissionsSentence("b", { ...BPA_ALL, BlockPublicAcls: false }, undefined)).toBe("b has 3 of the four Block Public Access settings on, and it has no bucket policy.");
  });

  test("event notification rules are read from the three destination lists and a new one is added to the right one", () => {
    const current = { LambdaFunctionConfigurations: [{ Id: "resize-on-create", LambdaFunctionArn: "arn:aws:lambda:us-east-1:1:function:resize-product-image", Events: ["s3:ObjectCreated:*"], Filter: { Key: { FilterRules: [{ Name: "suffix", Value: ".jpg" }] } } }] };
    expect(notificationRules(current)).toEqual([{ id: "resize-on-create", events: ["s3:ObjectCreated:*"], prefix: "", suffix: ".jpg", kind: "Lambda Function", arn: "arn:aws:lambda:us-east-1:1:function:resize-product-image", name: "resize-product-image" }]);
    const next = withNotification(current, { id: "image-removed", events: ["s3:ObjectRemoved:*"], prefix: "", suffix: ".jpg", kind: "SQS Queue", arn: "arn:aws:sqs:us-east-1:1:inventory-queue" });
    expect(next).toEqual({ ...current, QueueConfigurations: [{ Id: "image-removed", QueueArn: "arn:aws:sqs:us-east-1:1:inventory-queue", Events: ["s3:ObjectRemoved:*"], Filter: { Key: { FilterRules: [{ Name: "suffix", Value: ".jpg" }] } } }] });
    expect(notificationRules(next).map((r) => r.kind)).toEqual(["Lambda Function", "SQS Queue"]);
  });

  test("the website is a hosted index and error document, or a redirect of every request", () => {
    expect(websiteOf({ IndexDocument: { Suffix: "index.html" }, ErrorDocument: { Key: "error.html" } })).toEqual({ index: "index.html", error: "error.html", redirectHost: "", redirectProtocol: "" });
    expect(websiteOf({})).toBeNull();
    expect(websiteConfig({ mode: "host", index: "index.html", error: "", host: "", protocol: "" })).toEqual({ IndexDocument: { Suffix: "index.html" } });
    expect(websiteConfig({ mode: "redirect", index: "", error: "", host: "example.com", protocol: "https" })).toEqual({ RedirectAllRequestsTo: { HostName: "example.com", Protocol: "https" } });
  });

  test("the properties sentence counts what is set and says what is off", () => {
    expect(propertiesSentence("product-images", { versioning: "Enabled", notifications: 1, encryption: "AES256", website: false, lock: false })).toBe("product-images has 1 event notification rule, versioning on and default encryption SSE-S3. Static website hosting and Object Lock are off.");
    expect(propertiesSentence("b", { versioning: "Disabled", notifications: 0, encryption: null, website: true, lock: false })).toBe("b has no event notifications, versioning off and no default encryption. Static website hosting is on. Object Lock is off.");
  });

  test("lifecycle rules are read as rows and the sentence says what nothing prunes", () => {
    const answer = { Rules: [{ ID: "abort-stale-uploads", Status: "Enabled", Filter: {}, AbortIncompleteMultipartUpload: { DaysAfterInitiation: 7 } }] };
    const rules = lifecycleRules(answer);
    expect(rules).toEqual([{ name: "abort-stale-uploads", enabled: true, scope: "Entire bucket", actions: ["Delete incomplete multipart uploads 7 days after they start"], short: ["cleans up unfinished multipart uploads"], prunesVersions: false }]);
    expect(managementSentence("product-images", rules, "Enabled")).toBe("product-images has 1 lifecycle rule: abort-stale-uploads cleans up unfinished multipart uploads. Nothing yet prunes old versions, so each overwrite adds a stored version.");
    expect(managementSentence("product-images", rules, "Disabled")).not.toContain("Nothing yet");
    expect(managementSentence("thumbnails", [], "Disabled")).toBe("thumbnails has no lifecycle rules, so objects stay until someone deletes them.");
  });

  test("a lifecycle form makes the rule S3 takes, and the whole configuration carries the old rules", () => {
    expect(lifecycleRule(FORM)).toEqual({ ID: "prune-old-versions", Status: "Enabled", Filter: { Prefix: "catalog/" }, NoncurrentVersionExpiration: { NoncurrentDays: 30 } });
    expect(lifecycleRule({ ...FORM, allObjects: true, min: "1024", expireDays: "90", noncurrentDays: "", keepNewer: "" })).toEqual({ ID: "prune-old-versions", Status: "Enabled", Filter: { ObjectSizeGreaterThan: 1024 }, Expiration: { Days: 90 } });
    // One condition is itself, two or more are under And: a tag alone is a Tag, tags and a prefix are And with Tags.
    expect(lifecycleRule({ ...FORM, prefix: "", tags: [{ key: "env", value: "dev" }] }).Filter).toEqual({ Tag: { Key: "env", Value: "dev" } });
    expect(lifecycleRule({ ...FORM, tags: [{ key: "env", value: "dev" }, { key: "app", value: "larder" }] }).Filter).toEqual({ And: { Prefix: "catalog/", Tags: [{ Key: "env", Value: "dev" }, { Key: "app", Value: "larder" }] } });
    expect(lifecycleRule({ ...FORM, min: "10", max: "99" }).Filter).toEqual({ And: { Prefix: "catalog/", ObjectSizeGreaterThan: 10, ObjectSizeLessThan: 99 } });
    expect(lifecycleRule({ ...FORM, prefix: "", allObjects: true, tags: [{ key: "env", value: "dev" }] }).Filter).toEqual({});
    expect(lifecycleRule({ ...FORM, noncurrentDays: "30", keepNewer: "2" }).NoncurrentVersionExpiration).toEqual({ NoncurrentDays: 30, NewerNoncurrentVersions: 2 });
    const first = { ID: "a", Status: "Enabled", Filter: {}, Expiration: { Days: 1 } };
    expect(withRule({ Rules: [first] }, lifecycleRule(FORM))).toEqual({ Rules: [first, lifecycleRule(FORM)] });
  });

  test("a lifecycle form that cannot be sent says why", () => {
    expect(lifecycleProblem(FORM, [])).toBeNull();
    expect(lifecycleProblem({ ...FORM, name: "" }, [])).toBe("Name the rule.");
    expect(lifecycleProblem(FORM, ["prune-old-versions"])).toContain("already here");
    expect(lifecycleProblem({ ...FORM, noncurrentDays: "0" }, [])).toContain("whole number from 1");
    expect(lifecycleProblem({ ...FORM, noncurrentDays: "" }, [])).toBe("Choose at least one action.");
    expect(lifecycleProblem({ ...FORM, expireDays: "5", deleteMarkers: true }, [])).toContain("separate choices");
    expect(lifecycleProblem({ ...FORM, prefix: "" }, [])).toContain("prefix or a size");
  });
});

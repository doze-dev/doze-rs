// The S3 boards, each reached in a real browser against a real doze-aws seeded with the AWS CLI: the list, creating, deleting and
// emptying a bucket, a bucket's four tabs and each setting's edit page, uploading, an object's page, copying, a presigned link and
// deleting a version. With S3_SHOTS=<dir> the chromium run saves each board's state, light and dark, as <dir>/<board>.<mode>.png.
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { start } from "../server";
import { expect, expectNoErrorAnswers, settled, test } from "./fixtures";

/** Where the doze-aws with no bucket listens (two ports a browser); `E2E_BASE_PORT` moves it when another run holds these. */
const S3_EMPTY = Number(process.env.E2E_BASE_PORT ?? 44690);

const SHOTS = process.env.S3_SHOTS;
test.describe.configure({ mode: "serial" });

/** The board's state as the person sees it, light and dark. */
async function shot(page: Page, board: string, browserName: string) {
  if (!SHOTS || browserName !== "chromium") return;
  mkdirSync(SHOTS, { recursive: true });
  await page.waitForTimeout(500);
  for (const mode of ["light", "dark"] as const) {
    await page.emulateMedia({ colorScheme: mode });
    await page.waitForTimeout(250);
    await page.screenshot({ path: join(SHOTS, `${board}.${mode}.png`), fullPage: true });
  }
  await page.emulateMedia({ colorScheme: "light" });
}

const tmp = mkdtempSync(join(tmpdir(), "s3-boards-"));
function file(name: string, bytes: number): string {
  const p = join(tmp, name);
  writeFileSync(p, Buffer.alloc(bytes, 97));
  return p;
}

const names = (browser: string) => ({ img: `product-images-${browser}`, rec: `receipts-${browser}`, thumb: `thumbnails-${browser}`, made: `larder-exports-${browser}` });
const h1 = (page: Page, t: string | RegExp) => expect(page.locator("h1").first()).toHaveText(t);
const bucketUrl = (b: string, q = "") => `/resources/s3/buckets/${b}${q}`;

test("seed: three buckets with objects, versions, a delete marker, tags, a notification, a lifecycle rule, CORS and a policy", async ({ cli, browserName }) => {
  const n = names(browserName);
  for (const b of [n.img, n.rec, n.thumb]) expect(cli(["s3api", "create-bucket", "--bucket", b]).code).toBe(0);
  expect(cli(["s3api", "put-bucket-versioning", "--bucket", n.img, "--versioning-configuration", "Status=Enabled"]).code).toBe(0);
  const put = (b: string, key: string, f: string, more: string[] = []) => expect(cli(["s3api", "put-object", "--bucket", b, "--key", key, "--body", f, ...more]).code).toBe(0);
  for (const k of ["sku-DA-MLK-1.jpg", "sku-FV-APL-G.jpg", "sku-FV-ORG-B.jpg", "banners/hero.jpg", "catalog/index.json"]) put(n.img, k, file("a.bin", 4000), ["--content-type", "image/jpeg"]);
  for (const size of [4000, 5000, 6000]) put(n.img, "sku-FV-TOM-V.jpg", file(`v${size}.bin`, size), ["--content-type", "image/jpeg", "--metadata", "source=catalog", "--tagging", "sku=FV-TOM-V"]);
  expect(cli(["s3api", "delete-object", "--bucket", n.img, "--key", "sku-FV-ORG-B.jpg"]).code).toBe(0);
  expect(cli(["s3api", "put-bucket-tagging", "--bucket", n.img, "--tagging", "TagSet=[{Key=app,Value=larder},{Key=owner,Value=catalog}]"]).code).toBe(0);
  const arn = `arn:aws:lambda:us-east-1:111122223333:function:resize-${browserName}`;
  expect(cli(["s3api", "put-bucket-notification-configuration", "--bucket", n.img, "--notification-configuration", JSON.stringify({ LambdaFunctionConfigurations: [{ Id: "resize-on-create", LambdaFunctionArn: arn, Events: ["s3:ObjectCreated:*"], Filter: { Key: { FilterRules: [{ Name: "suffix", Value: ".jpg" }] } } }] })]).code).toBe(0);
  expect(cli(["s3api", "put-bucket-lifecycle-configuration", "--bucket", n.img, "--lifecycle-configuration", JSON.stringify({ Rules: [{ ID: "abort-stale-uploads", Status: "Enabled", Filter: {}, AbortIncompleteMultipartUpload: { DaysAfterInitiation: 7 } }] })]).code).toBe(0);
  expect(cli(["s3api", "put-bucket-cors", "--bucket", n.img, "--cors-configuration", JSON.stringify({ CORSRules: [{ AllowedHeaders: ["*"], AllowedMethods: ["GET", "HEAD"], AllowedOrigins: ["http://localhost:3000"], ExposeHeaders: ["ETag"], MaxAgeSeconds: 3000 }] })]).code).toBe(0);
  const policy = { Version: "2012-10-17", Statement: [{ Sid: "LambdaReadsImages", Effect: "Allow", Principal: { AWS: "arn:aws:iam::111122223333:role/lambda-exec" }, Action: "s3:GetObject", Resource: `arn:aws:s3:::${n.img}/*` }] };
  expect(cli(["s3api", "put-bucket-policy", "--bucket", n.img, "--policy", JSON.stringify(policy)]).code).toBe(0);
  for (const i of [1, 2, 3]) put(n.rec, `2026/r${i}.pdf`, file("r.bin", 3000));
  // Two KMS keys: one S3 can encrypt with, and one that signs (so it cannot be used for default encryption).
  const kms = (args: string[]) => JSON.parse(cli(["kms", ...args]).out) as { KeyMetadata: { KeyId: string } };
  const app = kms(["create-key", "--description", "app"]).KeyMetadata.KeyId;
  const signing = kms(["create-key", "--key-spec", "RSA_2048", "--key-usage", "SIGN_VERIFY", "--description", "signing"]).KeyMetadata.KeyId;
  expect(cli(["kms", "create-alias", "--alias-name", `alias/larder-app-${browserName}`, "--target-key-id", app]).code).toBe(0);
  expect(cli(["kms", "create-alias", "--alias-name", `alias/larder-signing-${browserName}`, "--target-key-id", signing]).code).toBe(0);
});

test("s3__buckets-list and s3__buckets-none: the list says how many and where; an account with none says so", async ({ page, browserName }) => {
  const n = names(browserName);
  await page.goto("/resources/s3/buckets");
  await h1(page, "General purpose buckets");
  await expect(page.getByTestId("list-sentence")).toContainText("buckets in this account");
  // The line names the bucket that keeps versions and whom it notifies, and when.
  await expect(page.getByTestId("list-sentence")).toContainText(`${n.img} has versioning on and notifies Lambda function resize-${browserName} when an object is created.`);
  await expect(page.getByRole("link", { name: n.img })).toBeVisible();
  await expect(page.getByText("US East (N. Virginia) us-east-1").first()).toBeVisible();
  await expect(page.getByTestId("bucket-delete")).toBeDisabled();
  await page.getByTestId("list-search").fill(n.rec);
  await expect(page.getByTestId("row-link")).toHaveCount(1);
  await page.getByTestId(`select-${n.rec}`).click();
  await expect(page.getByTestId("bucket-delete")).toBeEnabled();
  await expect(page.getByTestId("bucket-empty")).toBeEnabled();
  await page.getByTestId("list-search").fill("");
  // One bucket at a time, chosen with a radio: choosing another moves the choice.
  await expect(page.getByTestId(`select-${n.rec}`)).toHaveAttribute("role", "radio");
  await page.getByTestId(`select-${n.thumb}`).click();
  await expect(page.getByTestId(`select-${n.rec}`)).toHaveAttribute("aria-checked", "false");
  await page.getByTestId(`select-${n.rec}`).click();
  await shot(page, "s3__buckets-list", browserName);
  // A doze-aws with no bucket: the empty state.
  const other = await start(S3_EMPTY + (browserName === "firefox" ? 0 : browserName === "webkit" ? 2 : 4), S3_EMPTY + 1 + (browserName === "firefox" ? 0 : browserName === "webkit" ? 2 : 4));
  try {
    await page.goto(`${other.console}/resources/s3/buckets`);
    await expect(page.getByTestId("list-empty")).toContainText("You have no buckets in this region");
    await expect(page.getByTestId("list-empty").getByRole("button", { name: "Create bucket" })).toBeVisible();
    await shot(page, "s3__buckets-none", browserName);
  } finally {
    await other.stop();
  }
});

// The refused create is a 400 the browser logs.
test.describe("create", () => {
test("s3__bucket-create, -create-refused, -created: a refused name says why, a good one lands on the list with the bucket named", async ({ page, cli, browserName, refusals }) => {
  // The name S3 refuses is shown on the form.
  refusals.allow(/400 POST .*\/s3\/buckets\/op\/CreateBucket$/);
  const n = names(browserName);
  await page.goto("/resources/s3/buckets");
  await page.getByTestId("create-open").click();
  await h1(page, "Create bucket");
  await expect(page.getByTestId("region-line")).toContainText("US East (N. Virginia) us-east-1");
  await expect(page.getByText("This account already has")).toContainText(n.img);
  await page.getByTestId("bucket-name").fill("x");
  await expect(page.getByTestId("call-panel-code")).toContainText("CreateBucket");
  await expect(page.getByTestId("versioning-hint")).toContainText("turned it on; the ObjectCreated notification to resize-");
  await expect(page.getByText("uses app and owner")).toBeVisible();
  await expect(page.getByText(/alias\/larder-app-\w+ is a symmetric key in this Region, so it would appear in the KMS key list/)).toBeVisible();
  // Advanced settings are open; Object Lock asks for the warning to be acknowledged before the bucket is made.
  await expect(page.getByTestId("lock-on")).toBeVisible();
  await page.getByTestId("lock-on").click();
  await expect(page.getByTestId("lock-warning")).toBeVisible();
  await expect(page.getByTestId("form-submit")).toBeDisabled();
  await page.getByTestId("lock-ack").click();
  await expect(page.getByTestId("form-submit")).toBeEnabled();
  await page.getByTestId("lock-off").click();
  await expect(page.getByTestId("lock-warning")).toHaveCount(0);
  await shot(page, "s3__bucket-create", browserName);
  // Refused: an uppercase letter and an underscore.
  await page.getByTestId("bucket-name").fill("Larder_Exports");
  await page.getByTestId("form-submit").click();
  const banner = page.getByTestId("create-refused");
  await expect(banner).toContainText("CreateBucket answered 400 InvalidBucketName");
  await expect(banner).toContainText("has an uppercase letter and an underscore");
  await expect(banner).toContainText("for example to larder-exports");
  await expect(page.getByTestId("call-panel-came-back")).toContainText("InvalidBucketName");
  await shot(page, "s3__bucket-create-refused", browserName);
  // Corrected, with versioning, a tag and the block settings as they come.
  await settled(page);
  await page.getByTestId("bucket-name").fill(n.made);
  await page.getByTestId("create-versioning-on").click();
  // The form starts with one empty tag row; the lines under the choices name what the account's own buckets do.
  await page.getByTestId("create-tags-key-0").fill("env");
  await page.getByTestId("create-tags-value-0").fill("dev");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("flash")).toContainText(`Bucket ${n.made} created.`);
  await expect(page.getByTestId("flash")).toContainText("versioning on");
  await expect(page.getByRole("link", { name: n.made })).toBeVisible();
  await shot(page, "s3__bucket-created", browserName);
  expect(cli(["s3api", "get-bucket-versioning", "--bucket", n.made]).out).toContain("Enabled");
  expect(cli(["s3api", "get-bucket-tagging", "--bucket", n.made]).out).toContain("dev");
});

});

test("s3__bucket-objects, -objects-empty, -objects-versions: folders, a prefix with no match, and every version as a row", async ({ page, browserName }) => {
  const n = names(browserName);
  await page.goto(bucketUrl(n.img));
  await h1(page, n.img);
  await expect(page.getByTestId("detail-sentence")).toContainText("holds 3 objects and 2 folders at the top level, with versioning on");
  await expect(page.getByTestId("wired-strip")).toContainText(`resize-${browserName} (Lambda)`);
  // The bucket sits in the strip between what writes to it and what it notifies.
  await expect(page.getByTestId("wired-self")).toHaveText(n.img);
  await expect(page.getByText("US East (N. Virginia) us-east-1").first()).toBeVisible();
  await expect(page.getByTestId("tab-objects")).toBeVisible();
  await expect(page.getByTestId("objects-card")).toContainText("Objects (5)");
  await expect(page.getByTestId("page-line")).toHaveText("1 to 5 of 5 objects");
  await page.getByTestId("select-sku-FV-TOM-V.jpg").click();
  await expect(page.getByTestId("download-picked")).toBeEnabled();
  await page.getByTestId("objects-actions").click();
  await expect(page.getByTestId("action-presign")).toBeVisible();
  await shot(page, "s3__bucket-objects", browserName);
  await page.keyboard.press("Escape");
  // A folder opens, and back up.
  await page.getByRole("button", { name: "banners/" }).click();
  await expect(page.getByTestId("object-crumbs")).toContainText("banners");
  await expect(page.getByRole("button", { name: "hero.jpg" })).toBeVisible();
  await page.goBack();
  // A prefix nothing begins with.
  await page.getByTestId("object-find").fill("seasonal/");
  await page.getByTestId("object-search").click();
  await expect(page.getByTestId("objects-none")).toContainText("ListObjectsV2 with the prefix seasonal/ returned no keys");
  await expect(page.getByTestId("objects-card")).toContainText("Objects (0)");
  await shot(page, "s3__bucket-objects-empty", browserName);
  await page.getByTestId("object-find").fill("");
  await page.getByTestId("object-search").click();
  // Versions.
  await page.getByTestId("show-versions").click();
  await expect(page.getByText("Delete marker", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "sku-FV-TOM-V.jpg" })).toHaveCount(3);
  await expect(page.getByTestId("objects-card")).toContainText("Objects (9)");
  // The delete marker only looks like a delete: the page says which key and why.
  await expect(page.getByTestId("hidden-by-marker")).toContainText("sku-FV-ORG-B.jpg looks deleted but is not. Its current version is a delete marker");
  await shot(page, "s3__bucket-objects-versions", browserName);
});

test("s3__bucket-properties, -tags-none, -notifications-none: sections with their Edit, and what a bucket with none says", async ({ page, browserName }) => {
  const n = names(browserName);
  await page.goto(bucketUrl(n.img, "?tab=properties"));
  await expect(page.getByTestId("detail-sentence")).toContainText("has 1 event notification rule, versioning on and default encryption SSE-S3. Static website hosting and Object Lock are off.");
  await expect(page.getByTestId("properties")).toContainText(`arn:aws:s3:::${n.img}`);
  await expect(page.getByTestId("properties")).toContainText("Server-side encryption with Amazon S3 managed keys (SSE-S3)");
  await expect(page.getByTestId("properties")).toContainText("resize-on-create");
  await expect(page.getByTestId("properties")).toContainText("larder");
  await shot(page, "s3__bucket-properties", browserName);
  await page.goto(bucketUrl(n.thumb, "?tab=properties"));
  await expect(page.getByTestId("tags-none")).toContainText(`${n.thumb} has no tags`);
  await expect(page.getByTestId("notifications-none")).toContainText(`${n.thumb} sends no notifications`);
  await shot(page, "s3__bucket-tags-none", browserName);
  await shot(page, "s3__bucket-notifications-none", browserName);
});

test("s3__bucket-permissions, -management, -lifecycle-none: the Block Public Access table, the policy, ownership, CORS, and the lifecycle rules", async ({ page, browserName }) => {
  const n = names(browserName);
  await page.goto(bucketUrl(n.img, "?tab=permissions"));
  await expect(page.getByTestId("detail-sentence")).toContainText("is private: all four Block Public Access settings are on, and its one policy statement lets the role lambda-exec read objects.");
  await expect(page.getByTestId("permissions")).toContainText("Block all public access");
  await expect(page.getByTestId("policy-json")).toContainText("LambdaReadsImages");
  await expect(page.getByTestId("permissions")).toContainText("Bucket owner enforced");
  await expect(page.getByTestId("permissions")).toContainText("FULL_CONTROL");
  // The owner's grant is named for what it is, and the ACL is a card of its own.
  await expect(page.getByRole("table", { name: "Access control list" })).toContainText("Bucket owner (your AWS account)");
  await expect(page.getByRole("heading", { name: "Access control list (ACL)" })).toBeVisible();
  // The policy is written a member a line, and a CORS rule keeps each list of values on one line.
  await expect(page.getByTestId("policy-json")).toContainText('"Effect": "Allow",');
  expect((await page.getByTestId("policy-json").innerText()).split("\n").length).toBeGreaterThan(8);
  await expect(page.getByTestId("cors-json")).toContainText('"AllowedMethods": ["GET", "HEAD"],');
  await expect(page.getByTestId("cors-json")).toContainText("http://localhost:3000");
  await shot(page, "s3__bucket-permissions", browserName);
  await page.goto(bucketUrl(n.img, "?tab=management"));
  await expect(page.getByTestId("detail-sentence")).toContainText("has 1 lifecycle rule: abort-stale-uploads cleans up unfinished multipart uploads. Nothing yet prunes old versions");
  await expect(page.getByTestId("management")).toContainText("Delete incomplete multipart uploads 7 days after they start");
  await shot(page, "s3__bucket-management", browserName);
  await page.goto(bucketUrl(n.thumb, "?tab=management"));
  await expect(page.getByTestId("lifecycle-none")).toContainText(`${n.thumb} has no lifecycle rules`);
  await shot(page, "s3__lifecycle-none", browserName);
});

test("the edit pages: versioning, ownership, Block Public Access, encryption, website, CORS, policy, tags", async ({ page, cli, browserName }) => {
  const n = names(browserName);
  const edit = async (what: string, board: string) => {
    await page.goto(bucketUrl(n.thumb, `?do=${what}`));
    await expect(page.getByTestId("form-page")).toBeVisible();
    await expect(page.getByTestId("call-panel-code")).toBeVisible();
    await shot(page, board, browserName);
  };
  await edit("versioning-edit", "s3__versioning-edit");
  await page.getByTestId("versioning-Suspended").click();
  await expect(page.getByTestId("call-panel-code")).toContainText("PutBucketVersioning");
  await page.getByTestId("versioning-Enabled").click();
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("flash")).toContainText("Bucket Versioning is now enabled.");
  expect(cli(["s3api", "get-bucket-versioning", "--bucket", n.thumb]).out).toContain("Enabled");

  await edit("ownership-edit", "s3__ownership-edit");
  await page.getByTestId("ownership-ObjectWriter").click();
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("permissions")).toContainText("Object writer");

  await edit("bpa-edit", "s3__bpa-edit");
  await page.getByTestId("bpa-BlockPublicPolicy").click();
  await expect(page.getByTestId("call-panel-code")).toContainText('"BlockPublicPolicy": false');
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("permissions")).toContainText("BlockPublicPolicy");
  expect(cli(["s3api", "get-public-access-block", "--bucket", n.thumb]).out).toContain('"BlockPublicPolicy": false');

  await edit("encryption-edit", "s3__encryption-edit");
  await page.getByTestId("encryption-aws:kms").click();
  // The key list opens: the keys S3 can use, and which cannot be used and why.
  await expect(page.getByText(/Only a symmetric encryption key in the bucket's Region can be used\..*qualif/)).toBeVisible();
  await expect(page.getByText(/is RSA_2048 for signing, so S3 cannot use it/)).toBeVisible();
  await page.getByRole("combobox", { name: "Choose a KMS key" }).click();
  await page.getByRole("option", { name: new RegExp(`alias/larder-app-${browserName}`) }).click();
  await expect(page.getByTestId("call-panel-code")).toContainText(`:alias/larder-app-${browserName}`);
  await page.getByTestId("kms-arn").click();
  await page.getByTestId("kms-arn-input").fill("arn:aws:kms:us-east-1:111122223333:key/abc");
  await page.getByTestId("bucket-key-on").click();
  await expect(page.getByTestId("call-panel-code")).toContainText("BucketKeyEnabled");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("properties")).toContainText("SSE-KMS");

  await edit("website-edit", "s3__website-edit");
  await page.getByTestId("website-on-on").click();
  await page.getByTestId("website-error").fill("error.html");
  await expect(page.getByTestId("call-panel-code")).toContainText("PutBucketWebsite");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("properties")).toContainText("index.html");

  await edit("cors-edit", "s3__cors-edit");
  await expect(page.getByTestId("form-submit")).toBeEnabled();
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("cors-json")).toContainText("localhost:3000");

  await edit("policy-edit", "s3__policy-edit");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("policy-json")).toContainText("ReadObjects");

  await edit("tags-edit", "s3__tags-edit");
  await page.getByTestId("tags-kv-add").click();
  await page.getByTestId("tags-kv-key-0").fill("team");
  await page.getByTestId("tags-kv-value-0").fill("search");
  await expect(page.getByTestId("call-panel-code")).toContainText("PutBucketTagging");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("properties")).toContainText("search");
});

test("s3__notification-create and s3__lifecycle-create: each adds a rule to the whole configuration", async ({ page, cli, browserName }) => {
  const n = names(browserName);
  expect(cli(["sqs", "create-queue", "--queue-name", `inventory-${browserName}`]).code).toBe(0);
  await page.goto(bucketUrl(n.img, "?do=notification-create"));
  await page.getByTestId("notification-name").fill("image-removed");
  await page.getByTestId("event-ObjectRemoved").click();
  await page.getByTestId("dest-kind-SQS Queue").click();
  await page.getByTestId("dest-enter-arn").click();
  await page.getByTestId("dest-arn").fill(`arn:aws:sqs:us-east-1:111122223333:inventory-${browserName}`);
  await expect(page.getByTestId("call-panel-code")).toContainText("LambdaFunctionConfigurations");
  await expect(page.getByTestId("call-panel-code")).toContainText("QueueConfigurations");
  await shot(page, "s3__notification-create", browserName);
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("properties")).toContainText("image-removed");
  await expect(page.getByTestId("properties")).toContainText("resize-on-create");

  await page.goto(bucketUrl(n.img, "?do=lifecycle-create"));
  await page.getByTestId("lc-name").fill("prune-old-versions");
  await page.getByTestId("lc-prefix").fill("catalog/");
  await page.getByTestId("lc-noncurrentDays").fill("30");
  await expect(page.getByTestId("call-panel-code")).toContainText("abort-stale-uploads");
  await expect(page.getByTestId("call-panel-code")).toContainText("NoncurrentDays");
  await shot(page, "s3__lifecycle-create", browserName);
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("management")).toContainText("prune-old-versions");
  await expect(page.getByTestId("management")).toContainText("abort-stale-uploads");
  await expect(page.getByTestId("detail-sentence")).toContainText("has 2 lifecycle rules");
});

test("s3__folder-create, s3__upload, s3__upload-status: a folder, three files with properties, and each PutObject's answer", async ({ page, cli, browserName }) => {
  const n = names(browserName);
  await page.goto(bucketUrl(n.img));
  await page.getByTestId("folder-open").click();
  await page.getByTestId("folder-name").fill("seasonal");
  await expect(page.getByTestId("call-panel-code")).toContainText('"Key": "seasonal/"');
  // Specify an encryption key: the choice is offered, and the call carries what is chosen.
  await expect(page.getByTestId("sse-bucket")).toBeDisabled();
  await page.getByTestId("specify-key").click();
  await page.getByTestId("sse-override").click();
  await expect(page.getByTestId("call-panel-code")).toContainText('"ServerSideEncryption": "AES256"');
  await shot(page, "s3__folder-create", browserName);
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("flash")).toContainText("Folder seasonal/ created.");
  await expect(page.getByRole("button", { name: "seasonal/" })).toBeVisible();
  // Upload into that folder.
  await page.getByRole("button", { name: "seasonal/" }).click();
  await page.getByTestId("upload-open").click();
  await h1(page, "Upload");
  await page.getByTestId("upload-files").setInputFiles([file("sku-FV-RAD-R.jpg", 1600), file("sku-FV-BRC-G.jpg", 2000), file("hero-autumn.jpg", 2500)]);
  await expect(page.getByText("3 files total")).toBeVisible();
  // Properties are open: the classes are named as AWS names them, and a choice changes the call.
  await expect(page.getByText("S3 Glacier Instant Retrieval")).toBeVisible();
  await page.getByTestId("storage-GLACIER_IR").click();
  await page.getByTestId("checksum-on").click();
  await page.getByTestId("checksum").selectOption("SHA256");
  await expect(page.getByTestId("call-panel-code")).toContainText('"StorageClass": "GLACIER_IR"');
  await expect(page.getByTestId("call-panel-code")).toContainText('"ChecksumAlgorithm": "SHA256"');
  await page.getByTestId("storage-STANDARD").click();
  await page.getByTestId("checksum-off").click();
  await page.getByTestId("upload-tags-add").click();
  await page.getByTestId("upload-tags-key-0").fill("batch");
  await page.getByTestId("upload-tags-value-0").fill("autumn");
  await page.getByTestId("upload-meta-add").click();
  await page.getByTestId("upload-meta-key-0").fill("x-amz-meta-source");
  await page.getByTestId("upload-meta-value-0").fill("e2e");
  await expect(page.getByTestId("call-panel-code")).toContainText("Tagging");
  await shot(page, "s3__upload", browserName);
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("upload-summary")).toContainText(`3 PutObject calls succeeded in ${n.img}.`);
  await expect(page.getByTestId("upload-status-page")).toContainText("sku-FV-RAD-R.jpg");
  // The PutObject calls just made are in Recent calls (this console's own are pinned here), each with the key, and the event's destination is named.
  await expect(page.getByTestId("upload-summary")).toContainText(`Each ObjectCreated event was sent to resize-${browserName}.`);
  await expect(page.getByTestId("recent-calls")).toContainText("seasonal/hero-autumn.jpg");
  for (const k of ["sku-FV-RAD-R.jpg", "sku-FV-BRC-G.jpg"]) await expect(page.getByTestId("recent-calls")).toContainText(`seasonal/${k}`);
  await shot(page, "s3__upload-status", browserName);
  const head = cli(["s3api", "head-object", "--bucket", n.img, "--key", "seasonal/hero-autumn.jpg"]).out;
  expect(head).toContain('"source": "e2e"');
  expect(cli(["s3api", "get-object-tagging", "--bucket", n.img, "--key", "seasonal/hero-autumn.jpg"]).out).toContain("autumn");
});

test("s3__object-detail, -versions, -presign, -copy, -delete-version, -delete: one object, its versions, a presigned link that downloads it, a copy, and the two deletes", async ({ page, cli, browserName, request, refusals }) => {
  const n = names(browserName);
  await page.goto(bucketUrl(n.img));
  await page.getByRole("button", { name: "sku-FV-TOM-V.jpg" }).click();
  await h1(page, "sku-FV-TOM-V.jpg");
  await expect(page.getByTestId("object-sentence")).toContainText("is 5.9 KB");
  await expect(page.getByTestId("object-sentence")).toContainText("It has 3 versions");
  await expect(page.getByTestId("overview")).toContainText(`s3://${n.img}/sku-FV-TOM-V.jpg`);
  await expect(page.getByTestId("overview")).toContainText(`arn:aws:s3:::${n.img}/sku-FV-TOM-V.jpg`);
  await expect(page.getByTestId("overview")).toContainText("http://127.0.0.1");
  await expect(page.getByText("x-amz-meta-source")).toBeVisible();
  await expect(page.getByText("FV-TOM-V").first()).toBeVisible();
  await shot(page, "s3__object-detail", browserName);
  // The link: a presigned URL downloads the object with no key of ours.
  await page.getByTestId("object-actions").click();
  await page.getByTestId("action-presign").click();
  await expect(page.getByTestId("presign-dialog")).toBeVisible();
  await page.getByTestId("presign-submit").click();
  const url = (await page.getByTestId("presigned-url").textContent()) ?? "";
  expect(url).toContain("X-Amz-Signature=");
  await shot(page, "s3__object-presign", browserName);
  const got = await request.get(url);
  expect(got.status()).toBe(200);
  expect((await got.body()).length).toBe(6000);
  await page.keyboard.press("Escape");
  // Versions.
  await page.getByTestId("tab-versions").click();
  await expect(page.getByTestId("versions")).toContainText("Versions (3)");
  await shot(page, "s3__object-versions", browserName);
  // Delete the oldest version for good.
  const rows = page.getByTestId("versions").locator("tbody tr");
  await rows.last().getByRole("checkbox").click();
  await page.getByTestId("version-delete").click();
  await expect(page.getByTestId("object-delete-version-page")).toContainText("Permanently delete 1 version of");
  await expect(page.getByTestId("object-delete-version-page")).toContainText("The other 2 versions stay.");
  await expect(page.getByTestId("delete-objects-submit")).toBeDisabled();
  await page.getByTestId("confirm-input").fill("permanently delete");
  await shot(page, "s3__object-delete-version", browserName);
  await page.getByTestId("delete-objects-submit").click();
  await expect(page.getByTestId("flash")).toContainText("permanently deleted");
  await expectNoErrorAnswers(page, refusals);
  expect(JSON.parse(cli(["s3api", "list-object-versions", "--bucket", n.img, "--prefix", "sku-FV-TOM-V.jpg"]).out).Versions).toHaveLength(2);
  // Copy into a folder of another bucket.
  await page.goto(bucketUrl(n.img, "?do=object-copy&key=sku-FV-TOM-V.jpg&mode=copy"));
  await page.getByTestId("copy-dest").fill(`s3://${n.rec}/copies/`);
  await expect(page.getByTestId("call-panel-code")).toContainText(`"Key": "copies/sku-FV-TOM-V.jpg"`);
  await shot(page, "s3__object-copy", browserName);
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("flash")).toContainText(`to s3://${n.rec}/copies/sku-FV-TOM-V.jpg`);
  expect(cli(["s3api", "head-object", "--bucket", n.rec, "--key", "copies/sku-FV-TOM-V.jpg"]).code).toBe(0);
  // Delete: a delete marker, not the data.
  await page.goto(bucketUrl(n.img));
  await page.getByTestId("select-sku-FV-TOM-V.jpg").click();
  await page.getByTestId("delete-objects-open").click();
  await expect(page.getByTestId("object-delete-page")).toContainText("Versioning is on, so this does not remove the data.");
  await expect(page.getByTestId("object-delete-page")).toContainText("An ObjectRemoved notification rule exists on this bucket");
  await page.getByTestId("confirm-input").fill("delete");
  await shot(page, "s3__object-delete", browserName);
  await page.getByTestId("delete-objects-submit").click();
  await expect(page.getByTestId("flash")).toContainText("1 object deleted");
  await expectNoErrorAnswers(page, refusals);
  const after = JSON.parse(cli(["s3api", "list-object-versions", "--bucket", n.img, "--prefix", "sku-FV-TOM-V.jpg"]).out);
  expect(after.DeleteMarkers).toHaveLength(1);
  expect(after.Versions).toHaveLength(2);
});

// Deleting a bucket that is not empty is the 409 the page is there to show.
test.describe("danger", () => {
test("s3__bucket-empty, -empty-status, -delete, -deleted: emptying is counted and reported; deleting asks for the name and refuses a bucket that is not empty", async ({ page, cli, browserName, refusals }) => {
  // A bucket that is not empty is refused its delete (409), and the dialog says so.
  refusals.allow(/409 DELETE .*\/s3\/buckets\/receipts-/);
  const n = names(browserName);
  await page.goto("/resources/s3/buckets");
  await page.getByTestId(`select-${n.rec}`).click();
  // Delete first: the bucket holds objects.
  await page.getByTestId("bucket-delete").click();
  await h1(page, "Delete bucket");
  await expect(page.getByTestId("not-empty")).toContainText("This bucket is not empty");
  await page.getByTestId("confirm-input").fill(n.rec);
  await page.getByTestId("delete-submit").click();
  await expect(page.getByTestId("delete-refused")).toContainText("BucketNotEmpty");
  await expectNoErrorAnswers(page, refusals);
  await page.getByTestId("go-empty").click();
  await h1(page, "Empty bucket");
  await expect(page.getByTestId("holding")).toContainText("holds 4 objects");
  await expect(page.getByTestId("holding")).toContainText("It has no versioning");
  await expect(page.getByTestId("empty-submit")).toBeDisabled();
  await page.getByTestId("confirm-input").fill(n.rec);
  await shot(page, "s3__bucket-empty", browserName);
  await page.getByTestId("empty-submit").click();
  await h1(page, "Empty bucket: Status");
  await expect(page.getByTestId("deleted")).toContainText("4 objects");
  await expect(page.getByTestId("failed")).toContainText("0 objects");
  await shot(page, "s3__bucket-empty-status", browserName);
  expect(JSON.parse(cli(["s3api", "list-objects-v2", "--bucket", n.rec]).out || "{}").Contents ?? []).toHaveLength(0);
  await page.getByTestId("status-exit").click();
  await page.getByTestId(`select-${n.rec}`).click();
  await page.getByTestId("bucket-delete").click();
  await expect(page.getByTestId("not-empty")).toHaveCount(0);
  await expect(page.getByTestId("delete-submit")).toBeDisabled();
  await page.getByTestId("confirm-input").fill(n.rec);
  await shot(page, "s3__bucket-delete", browserName);
  await page.getByTestId("delete-submit").click();
  await expect(page.getByTestId("flash")).toContainText(`Bucket ${n.rec} deleted.`);
  await expectNoErrorAnswers(page, refusals);
  await expect(page.getByRole("link", { name: n.rec })).toHaveCount(0);
  await shot(page, "s3__bucket-deleted", browserName);
  expect(cli(["s3api", "head-bucket", "--bucket", n.rec]).code).not.toBe(0);
});

});

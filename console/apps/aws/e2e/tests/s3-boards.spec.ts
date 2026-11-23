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
  // The three browsers run one after another on one doze-aws: the buckets another browser left would come first in every list,
  // sentence and hint of this one (they name the first few). Each browser starts from an account holding none of the others'.
  const mine = new RegExp(`-${browserName}$`);
  const left = (JSON.parse(cli(["s3api", "list-buckets"]).out || "{}") as { Buckets?: { Name: string }[] }).Buckets ?? [];
  for (const { Name } of left) if (/-(firefox|webkit|chromium)$/.test(Name) && !mine.test(Name)) cli(["s3", "rb", `s3://${Name}`, "--force"]);
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
  // The line names the buckets that keep versions (the first two, then "N more") and whom a bucket notifies, and when. The three
  // browsers share one server and each seeds its own buckets, so a browser that runs after another finds the other's buckets
  // first in the line: it names its own bucket only when it is among the first, and says "more" when it is not.
  const sentence = page.getByTestId("list-sentence");
  await expect(sentence).toContainText(/more buckets? keep versions or notify|has versioning on and notifies Lambda function resize-\w+ when an object is created\./);
  await expect(sentence.locator('[data-slot="emphasis-bold"]').first()).toHaveText(/^\d+$/);
  if ((await sentence.innerText()).includes(`${n.img} has versioning on`)) {
    await expect(sentence).toContainText(`${n.img} has versioning on and notifies Lambda function resize-${browserName} when an object is created.`);
    await expect(sentence.locator('[data-slot="emphasis-name"]', { hasText: n.img }).first()).toBeVisible();
    await expect(sentence.locator('[data-slot="emphasis-bold"]', { hasText: `resize-${browserName}` })).toBeVisible();
  }
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
  // The hint names the first six buckets and counts the rest: another browser's buckets, left on the shared server, come first.
  const has = page.getByText("This account already has");
  await expect(has).toBeVisible();
  if (!/ and \d+ more\.$/.test((await has.innerText()).trim())) await expect(has).toContainText(n.img);
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
  // Under the field, in red, the naming rule the board prints, then what is wrong with this name.
  await expect(page.locator('[data-slot="field-refusal"]').first()).toContainText("A bucket name must be between 3 and 63 characters long");
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
  // A bucket with Object Lock: made once the warning is acknowledged, and said to have versioning on with it.
  await page.getByTestId("create-open").click();
  await page.getByTestId("bucket-name").fill(`locked-${browserName}`);
  await page.getByTestId("lock-on").click();
  await page.getByTestId("lock-ack").click();
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("flash")).toContainText(`Bucket locked-${browserName} created.`);
  await expect(page.getByTestId("flash")).toContainText("versioning on, all four Block Public Access settings on and Object Lock on");
  expect(cli(["s3api", "get-bucket-versioning", "--bucket", `locked-${browserName}`]).out).toContain("Enabled");
  expect(cli(["s3api", "get-object-lock-configuration", "--bucket", `locked-${browserName}`]).out).toContain("Enabled");
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
  await expect(page.getByTestId("versioning-delay")).toContainText("up to 15 minutes to reach every part of S3; until then a GET for a new object may return 404 NoSuchKey, so wait before writing or deleting.");
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
  // Policy examples: one is put in the editor, for this bucket, and nothing is saved by choosing it.
  await page.getByTestId("policy-examples-open").click();
  await expect(page.getByTestId("policy-examples")).toContainText("Let a role read objects");
  await page.getByTestId("use-tls-only").click();
  await expect(page.getByTestId("call-panel-code")).toContainText("DenyInsecureTransport");
  await expect(page.getByTestId("call-panel-code")).toContainText(n.thumb);
  await page.getByTestId("policy-examples-open").click();
  await page.getByTestId("use-role-read").click();
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("policy-json")).toContainText("RoleReadsObjects");

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
  // Each action is ticked to reveal its fields; the disabled transitions say why. A tag and a prefix go under And.
  await expect(page.getByTestId("lc-transition-current")).toBeDisabled();
  await expect(page.getByTestId("lc-noncurrentDays")).toHaveCount(0);
  await page.getByTestId("lc-previous-on").click();
  await page.getByTestId("lc-noncurrentDays").fill("30");
  await page.getByTestId("lc-tags-key-0").fill("env");
  await page.getByTestId("lc-tags-value-0").fill("dev");
  await expect(page.getByTestId("call-panel-code")).toContainText('"Tags"');
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
  // The PutObject calls just made are in Recent calls (this console's own are pinned here), and the event's destination is named.
  await expect(page.getByTestId("upload-summary")).toContainText(`Each ObjectCreated event was sent to resize-${browserName}`);
  // (A call this console makes is recorded with its bucket, not its key: the rows say which calls, the Status table above says which files.)
  const mine = page.getByTestId("recent-calls").locator("tr", { hasText: "This console" }).filter({ hasText: "PutObject" });
  await expect.poll(() => mine.count()).toBeGreaterThanOrEqual(3);
  await shot(page, "s3__upload-status", browserName);
  // The calls this console made name the object they acted on, not the bucket alone.
  await page.getByTestId("calls-console-toggle").click();
  await expect(page.getByTestId("recent-calls").getByTestId("call-key").filter({ hasText: "seasonal/hero-autumn.jpg" }).first()).toBeVisible();
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
  // Its Recent calls are the object's own: each names the key, and a write or a read says how big the object was.
  const calls = page.getByTestId("recent-calls");
  await expect(calls).toContainText("PutObject");
  await expect(calls.getByTestId("call-key").first()).toHaveText("sku-FV-TOM-V.jpg");
  await expect(calls.getByTestId("answer-words").filter({ hasText: /^· \d+(\.\d)? KiB$/ }).first()).toBeVisible();
  await shot(page, "s3__object-detail", browserName);
  // Tags and Metadata sit side by side, as drawn.
  const tagsBox = await page.getByTestId("object-tags").boundingBox();
  const metaBox = await page.getByTestId("object-metadata").boundingBox();
  expect(Math.abs(tagsBox!.y - metaBox!.y)).toBeLessThan(2);
  expect(tagsBox!.x + tagsBox!.width).toBeLessThanOrEqual(metaBox!.x);
  // Object actions: the list's own menu (Download as, Share, Calculate total size, Copy, Move, Rename) and Delete.
  await page.getByTestId("object-actions").click();
  for (const id of ["action-download-as", "action-presign", "action-total", "action-copy", "action-move", "action-rename", "action-delete"]) await expect(page.getByTestId(id)).toBeVisible();
  await shot(page, "s3__object-detail-actions", browserName);
  await page.getByTestId("action-total").click();
  await expect(page.getByTestId("total-size")).toContainText("in 1 object");
  await shot(page, "s3__object-detail-total", browserName);
  await page.keyboard.press("Escape");
  await page.getByTestId("object-actions").click();
  await page.getByTestId("action-download-as").click();
  await expect(page.getByTestId("save-as")).toHaveValue("sku-FV-TOM-V.jpg");
  await page.getByTestId("save-as").fill("tomato.jpg");
  const saved = page.waitForEvent("download");
  await page.getByTestId("save-as-submit").click();
  expect((await saved).suggestedFilename()).toBe("tomato.jpg");
  await expectNoErrorAnswers(page, refusals);
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

// Copy with "Specify settings": each of the six is set on the page and is what the copy has afterwards; a destination with ACLs
// disabled offers only the owner's full control; a move and a rename read nothing of the object they just deleted.
test("s3__object-copy (Specify settings), Move and Rename: the copy has the storage class, ACL, tags, metadata, encryption and checksum chosen; a move and a rename leave no refused read", async ({ page, cli, browserName, refusals }) => {
  const b = browserName;
  const src = `copyset-src-${b}`;
  const dst = `copyset-dst-${b}`;
  const enf = `copyset-enf-${b}`;
  expect(cli(["s3api", "create-bucket", "--bucket", src]).code).toBe(0);
  // ACLs enabled and public ACLs allowed on one destination; the other is a new bucket (ACLs disabled).
  expect(cli(["s3api", "create-bucket", "--bucket", dst, "--object-ownership", "ObjectWriter"]).code).toBe(0);
  expect(cli(["s3api", "put-public-access-block", "--bucket", dst, "--public-access-block-configuration", "BlockPublicAcls=false,IgnorePublicAcls=false,BlockPublicPolicy=true,RestrictPublicBuckets=true"]).code).toBe(0);
  expect(cli(["s3api", "create-bucket", "--bucket", enf]).code).toBe(0);
  const body = file("readme.txt", 300);
  const put = (key: string) => expect(cli(["s3api", "put-object", "--bucket", src, "--key", key, "--body", body, "--content-type", "text/plain", "--metadata", "source=catalog", "--tagging", "team=web"]).code).toBe(0);
  put("docs/readme.txt");
  put("docs/old.txt");

  await page.goto(bucketUrl(src, "?do=object-copy&key=docs/readme.txt&mode=copy"));
  await h1(page, "Copy");
  // The destination starts at the object's own folder.
  await expect(page.getByTestId("copy-dest")).toHaveValue(`s3://${src}/docs/`);
  await page.getByTestId("copy-dest").fill(`s3://${dst}/out/`);
  await expect(page.getByTestId("section-Destination details")).toContainText("Disabled");
  await page.getByTestId("copy-settings-specify").click();
  // Each setting starts as the source has it: its tags and its metadata are the rows.
  await expect(page.getByTestId("copy-tags-key-0")).toHaveValue("team");
  await expect(page.getByTestId("copy-tags-value-0")).toHaveValue("web");
  await expect(page.getByTestId("copy-specify")).toContainText("Stored, not applied locally");
  await page.getByTestId("storage-GLACIER").click();
  await expect(page.getByTestId("storage-archive-note")).toContainText("cannot be read afterwards");
  await page.getByTestId("storage-STANDARD_IA").click();
  await expect(page.getByTestId("storage-archive-note")).toHaveCount(0);
  await page.getByTestId("copy-acl").selectOption("authenticated-read");
  await page.getByTestId("copy-tags-add").click();
  await page.getByTestId("copy-tags-key-1").fill("stage");
  await page.getByTestId("copy-tags-value-1").fill("test");
  await page.getByTestId("copy-metadata-replace").click();
  await expect(page.getByTestId("copy-meta-key-0")).toHaveValue("Content-Type");
  await expect(page.getByTestId("copy-meta-key-1")).toHaveValue("x-amz-meta-source");
  await page.getByTestId("copy-meta-remove-1").click();
  await page.getByTestId("copy-meta-add").click();
  await page.getByTestId("copy-meta-key-1").fill("x-amz-meta-owner");
  await page.getByTestId("copy-meta-value-1").fill("web");
  await page.getByTestId("specify-key").click();
  await page.getByTestId("sse-override").click();
  await page.getByTestId("sse-kms").click();
  await page.getByTestId("kms-key").fill(`alias/larder-app-${b}`);
  await page.getByTestId("checksum-on").click();
  await page.getByTestId("checksum").selectOption("SHA256");
  const call = page.getByTestId("call-panel-code");
  for (const want of ['"StorageClass": "STANDARD_IA"', '"ACL": "authenticated-read"', '"MetadataDirective": "REPLACE"', '"ContentType": "text/plain"', '"ServerSideEncryption": "aws:kms"', '"ChecksumAlgorithm": "SHA256"', '"TaggingDirective": "REPLACE"']) await expect(call).toContainText(want);
  await shot(page, "s3__object-copy-specify", b);
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("flash")).toContainText(`to s3://${dst}/out/readme.txt`);
  await expectNoErrorAnswers(page, refusals);
  const head = JSON.parse(cli(["s3api", "head-object", "--bucket", dst, "--key", "out/readme.txt", "--checksum-mode", "ENABLED"]).out);
  expect(head.StorageClass).toBe("STANDARD_IA");
  expect(head.Metadata).toEqual({ owner: "web" });
  expect(head.ContentType).toBe("text/plain");
  expect(head.ServerSideEncryption).toBe("aws:kms");
  expect(head.SSEKMSKeyId).toBe(`alias/larder-app-${b}`);
  expect(head.ChecksumSHA256).toBeTruthy();
  const tags = JSON.parse(cli(["s3api", "get-object-tagging", "--bucket", dst, "--key", "out/readme.txt"]).out).TagSet as { Key: string; Value: string }[];
  expect(tags.map((t) => `${t.Key}=${t.Value}`).sort()).toEqual(["stage=test", "team=web"]);
  expect(cli(["s3api", "get-object-acl", "--bucket", dst, "--key", "out/readme.txt"]).out).toContain("AuthenticatedUsers");

  // A destination with ACLs disabled takes only the bucket owner full control ACL, and the page offers no other.
  await page.goto(bucketUrl(src, "?do=object-copy&key=docs/readme.txt&mode=move"));
  await page.getByTestId("copy-dest").fill(`s3://${enf}/`);
  await page.getByTestId("copy-settings-specify").click();
  await expect(page.getByTestId("copy-specify")).toContainText("ACLs disabled");
  await expect(page.locator('[data-testid="copy-acl"] option[value="private"]')).toBeDisabled();
  await expect(page.locator('[data-testid="copy-acl"] option[value="bucket-owner-full-control"]')).toBeEnabled();
  await page.getByTestId("copy-acl").selectOption("bucket-owner-full-control");
  await shot(page, "s3__object-copy-specify-enforced", b);
  // Move: the copy, then the delete of the source, and nothing asks for the source again.
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("flash")).toContainText(`Moved readme.txt to s3://${enf}/readme.txt`);
  await expectNoErrorAnswers(page, refusals);
  expect(cli(["s3api", "head-object", "--bucket", enf, "--key", "readme.txt"]).code).toBe(0);
  expect(cli(["s3api", "head-object", "--bucket", src, "--key", "docs/readme.txt"]).code).not.toBe(0);

  // Rename: the same.
  await page.goto(bucketUrl(src, "?do=object-copy&key=docs/old.txt&mode=rename"));
  await page.getByTestId("rename-name").fill("new.txt");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("flash")).toContainText(`Renamed old.txt to s3://${src}/docs/new.txt`);
  await expectNoErrorAnswers(page, refusals);
  expect(cli(["s3api", "head-object", "--bucket", src, "--key", "docs/new.txt"]).code).toBe(0);
  expect(cli(["s3api", "head-object", "--bucket", src, "--key", "docs/old.txt"]).code).not.toBe(0);
  for (const bucket of [src, dst, enf]) expect(cli(["s3", "rb", `s3://${bucket}`, "--force"]).code).toBe(0);
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

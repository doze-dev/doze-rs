// The UX audit's sweep: a real doze-aws of its own (ports from the environment, else 42630 and 42631) with a world made through
// its endpoint (the AWS CLI for the resources with parts, plain JSON calls for the bulk: 1,000 queues, 1,000 items, long names),
// then every page at each width, light and dark, in each browser: a screenshot and a measure of what spills (the page scrolling
// sideways, a table scrolling sideways inside itself, text cut without an ellipsis, a box past the window's edge). With
// `focus`, each page is also walked with Tab, noting a focused control that shows no ring. With `down`, doze-aws is stopped
// after every page has been seen once and each page is opened again, for its error state.
//   bun e2e/audit.ts <out dir> [firefox,webkit,chromium] [1024,1280,1440,1920] [focus] [down]
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { chromium, firefox, type Page, webkit } from "@playwright/test";
import { aws, start } from "./server";

const [out = "audit", only = "firefox,webkit,chromium", sizes = "1024,1280,1440,1920", ...flags] = process.argv.slice(2);
const listen = Number(process.env.DOZE_AWS_SHOTS_PORT ?? 42630);
const engines = { firefox, webkit, chromium };
const APP = "AKIALARDERAPP0000001";
const server = await start(listen, listen + 1);
const acct = server.account;
const enc = encodeURIComponent;
const LONG = "larder-fulfilment-pick-lists-for-the-north-warehouse-priority-orders-v2";

function run(args: string[]) {
  const r = aws(server.endpoint, args, { accessKey: APP });
  if (r.code !== 0) console.warn(`aws ${args.join(" ")}: ${r.out.slice(0, 200)}`);
  return r;
}

/** A JSON-protocol call signed with nothing (doze-aws trusts any key by default): the bulk goes faster than a CLI each. */
async function call(service: string, target: string, body: unknown, version = "1.0") {
  const d = new Date().toISOString().replace(/[-:]/g, "").replace(/\.\d+/, "");
  const res = await fetch(`${server.endpoint}/`, {
    method: "POST",
    headers: { "Content-Type": `application/x-amz-json-${version}`, "X-Amz-Target": target, "X-Amz-Date": d, Authorization: `AWS4-HMAC-SHA256 Credential=${APP}/${d.slice(0, 8)}/us-east-1/${service}/aws4_request, SignedHeaders=host;x-amz-date, Signature=00` },
    body: JSON.stringify(body),
  });
  if (!res.ok) console.warn(`${target}: ${res.status} ${(await res.text()).slice(0, 200)}`);
}

async function seed() {
  const dir = join(tmpdir(), `doze-aws-audit-${process.pid}`);
  mkdirSync(dir, { recursive: true });
  run(["sqs", "create-queue", "--queue-name", "pick-lists-dlq"]);
  const policy = JSON.stringify({ deadLetterTargetArn: `arn:aws:sqs:us-east-1:${acct}:pick-lists-dlq`, maxReceiveCount: 5 });
  run(["sqs", "create-queue", "--queue-name", "pick-lists", "--attributes", JSON.stringify({ RedrivePolicy: policy })]);
  run(["sqs", "create-queue", "--queue-name", LONG]);
  for (let i = 0; i < 4; i++) run(["sqs", "send-message", "--queue-url", `${server.endpoint}/${acct}/pick-lists-dlq`, "--message-body", JSON.stringify({ order_id: `ord-03${i}`, note: "a long body ".repeat(i * 6) })]);
  const queues = Array.from({ length: 1000 }, (_, i) => `bulk-${String(i).padStart(4, "0")}`);
  for (let i = 0; i < queues.length; i += 50) await Promise.all(queues.slice(i, i + 50).map((q) => call("sqs", "AmazonSQS.CreateQueue", { QueueName: q })));
  run(["dynamodb", "create-table", "--table-name", "orders", "--attribute-definitions", "AttributeName=pk,AttributeType=S", "AttributeName=sk,AttributeType=N", "--key-schema", "AttributeName=pk,KeyType=HASH", "AttributeName=sk,KeyType=RANGE", "--billing-mode", "PAY_PER_REQUEST"]);
  for (let i = 0; i < 1000; i += 25) {
    const items = Array.from({ length: 25 }, (_, j) => ({ PutRequest: { Item: { pk: { S: `customer-${(i + j) % 40}` }, sk: { N: String(i + j) }, status: { S: (i + j) % 7 ? "shipped" : "held" }, total: { N: String(((i + j) * 1.37).toFixed(2)) }, note: { S: (i + j) % 50 ? "" : `${LONG} ${LONG}` } } } }));
    await call("dynamodb", "DynamoDB_20120810.BatchWriteItem", { RequestItems: { orders: items } });
  }
  run(["s3api", "create-bucket", "--bucket", "product-images"]);
  const file = join(dir, "hero.txt");
  writeFileSync(file, "hello ".repeat(400));
  for (const key of ["banners/hero.jpg", "banners/spring/hero@2x.jpg", `catalogue/${LONG}/${LONG}-front-view-original.jpg`, "README.txt"]) run(["s3api", "put-object", "--bucket", "product-images", "--key", key, "--body", file, "--content-type", "image/jpeg"]);
  run(["kms", "create-key", "--description", "Signs the order receipts"]);
  run(["secretsmanager", "create-secret", "--name", "larder/db-password", "--secret-string", "s3cret"]);
  run(["ssm", "put-parameter", "--name", `/larder/${LONG}/endpoint`, "--type", "String", "--value", "https://api.larder.dev"]);
  run(["sns", "create-topic", "--name", "order-events"]);
  run(["sns", "subscribe", "--topic-arn", `arn:aws:sns:us-east-1:${acct}:order-events`, "--protocol", "sqs", "--notification-endpoint", `arn:aws:sqs:us-east-1:${acct}:pick-lists`]);
  const trust = JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Principal: { Service: "lambda.amazonaws.com" }, Action: "sts:AssumeRole" }] });
  run(["iam", "create-role", "--role-name", "lambda-exec", "--assume-role-policy-document", trust]);
  run(["iam", "create-user", "--user-name", "report-job"]);
  writeFileSync(join(dir, "bootstrap"), "#!/bin/sh\nexit 0\n", { mode: 0o755 });
  execFileSync("zip", ["-qj", join(dir, "f.zip"), join(dir, "bootstrap")]);
  run(["lambda", "create-function", "--function-name", "send-email", "--runtime", "provided.al2023", "--handler", "x", "--role", `arn:aws:iam::${acct}:role/lambda-exec`, "--zip-file", `fileb://${join(dir, "f.zip")}`, "--environment", "Variables={LOG_LEVEL=INFO}"]);
  const machine = { StartAt: "LoadOrder", States: { LoadOrder: { Type: "Pass", Next: "Route" }, Route: { Type: "Choice", Choices: [{ Variable: "$.hold", IsPresent: true, Next: "Hold" }], Default: "Done" }, Hold: { Type: "Wait", Seconds: 600, Next: "Done" }, Done: { Type: "Pass", End: true } } };
  const sm = run(["stepfunctions", "create-state-machine", "--name", "order-fulfilment", "--definition", JSON.stringify(machine), "--role-arn", `arn:aws:iam::${acct}:role/lambda-exec`]);
  const smArn = (JSON.parse(sm.out) as { stateMachineArn: string }).stateMachineArn;
  run(["stepfunctions", "start-execution", "--state-machine-arn", smArn, "--name", "ord-0420", "--input", '{"order_id":"ord-0420"}']);
  run(["logs", "create-log-group", "--log-group-name", "/larder/api"]);
  run(["logs", "create-log-stream", "--log-group-name", "/larder/api", "--log-stream-name", "orders-3f9a0"]);
  const lines = Array.from({ length: 30 }, (_, i) => ({ timestamp: Date.now() - (30 - i) * 20_000, message: i % 7 ? JSON.stringify({ level: "info", msg: "request done", ms: 12 + i }) : `ERROR ${LONG} ${"x".repeat(300)}` }));
  writeFileSync(join(dir, "lines.json"), JSON.stringify(lines));
  run(["logs", "put-log-events", "--log-group-name", "/larder/api", "--log-stream-name", "orders-3f9a0", "--log-events", `file://${join(dir, "lines.json")}`]);
  const template = { Resources: { Queue: { Type: "AWS::SQS::Queue", Properties: { QueueName: "stack-queue" } } }, Outputs: { QueueUrl: { Value: { Ref: "Queue" } } } };
  run(["cloudformation", "create-stack", "--stack-name", "larder-api", "--template-body", JSON.stringify(template)]);
  run(["kinesis", "create-stream", "--stream-name", "clicks", "--shard-count", "2"]);
  run(["cloudwatch", "put-metric-alarm", "--alarm-name", "dlq-not-empty", "--metric-name", "ApproximateNumberOfMessagesVisible", "--namespace", "AWS/SQS", "--statistic", "Sum", "--period", "60", "--evaluation-periods", "1", "--threshold", "1", "--comparison-operator", "GreaterThanOrEqualToThreshold"]);
  run(["events", "put-rule", "--name", "order-placed", "--event-pattern", '{"source":["larder.orders"]}']);
}

const PAGES: [string, string][] = [
  ["home", "/"],
  ["connect", "/connect"],
  ["live", "/live"],
  ["live-wire", "/live?view=wire"],
  ["resources", "/resources"],
  ["sqs-queues", "/resources/sqs/queues"],
  ["sqs-queue", "/resources/sqs/queues/pick-lists-dlq"],
  ["sqs-messages", "/resources/sqs/queues/pick-lists-dlq?tab=messages"],
  ["sqs-redrive", "/resources/sqs/queues/pick-lists-dlq?tab=redrive"],
  ["sqs-settings", "/resources/sqs/queues/pick-lists-dlq?tab=settings"],
  ["sqs-long", `/resources/sqs/queues/${LONG}`],
  ["s3-objects", "/resources/s3/buckets/product-images?tab=objects"],
  ["ddb-items", "/resources/dynamodb/tables/orders?tab=items"],
  ["lambda", "/resources/lambda/functions/send-email"],
  ["sfn", "/resources/stepfunctions/machines/order-fulfilment"],
  ["logs-streams", `/resources/logs/groups/${enc("/larder/api")}?tab=streams`],
  ["cfn", "/resources/cloudformation/stacks/larder-api"],
  ["kms", "/resources/kms/keys"],
  ["ssm", "/resources/ssm/parameters"],
  ["iam-roles", "/resources/iam/roles"],
  ["access", "/access"],
  ["lab", "/lab"],
  ["settings", "/settings"],
];

type Spill = { kind: string; what: string; by: number };

/** What spills on the page as it is now (run in the page). */
function measure(): { docX: number; spills: Spill[] } {
  const name = (el: Element) => {
    const h = el as HTMLElement;
    const id = h.dataset.testid ? `[${h.dataset.testid}]` : h.getAttribute("aria-label") ? `[${h.getAttribute("aria-label")}]` : "";
    return `${el.tagName.toLowerCase()}${id} "${(h.innerText ?? "").replace(/\s+/g, " ").slice(0, 50)}"`;
  };
  const spills: Spill[] = [];
  const w = window.innerWidth;
  for (const el of Array.from(document.querySelectorAll("body *"))) {
    const h = el as HTMLElement;
    const cs = getComputedStyle(h);
    if (cs.display === "none" || cs.visibility === "hidden" || h.closest("[aria-hidden=true]") || h.closest(".sr-only")) continue;
    const over = h.scrollWidth - h.clientWidth;
    if (over > 1 && (cs.overflowX === "auto" || cs.overflowX === "scroll") && h.clientWidth > 0) spills.push({ kind: "scrolls-sideways", what: name(h), by: over });
    else if (over > 1 && cs.overflowX === "hidden" && h.children.length === 0 && cs.textOverflow !== "ellipsis" && h.clientWidth > 0) spills.push({ kind: "cut", what: name(h), by: over });
    else if (over > 1 && cs.textOverflow === "ellipsis" && !h.title && !h.closest("[title]")) spills.push({ kind: "ellipsis-no-title", what: name(h), by: over });
    const r = h.getBoundingClientRect();
    if (r.width > 0 && r.right > w + 1 && !h.closest("[data-radix-scroll-area-viewport]") && getComputedStyle(h.parentElement ?? h).overflowX === "visible") {
      let p = h.parentElement;
      let clipped = false;
      while (p) {
        const o = getComputedStyle(p).overflowX;
        if (o !== "visible") clipped = true;
        p = p.parentElement;
      }
      if (!clipped) spills.push({ kind: "past-edge", what: name(h), by: Math.round(r.right - w) });
    }
  }
  // A control drawn past the card it belongs to (a card that is not a scroll box: the control spills over its border).
  for (const el of Array.from(document.querySelectorAll("section button, section a, section input"))) {
    const card = el.closest("section");
    if (!card) continue;
    const r = el.getBoundingClientRect();
    const c = card.getBoundingClientRect();
    if (r.width > 0 && r.right > c.right + 1) spills.push({ kind: "past-card", what: name(el), by: Math.round(r.right - c.right) });
  }
  return { docX: document.documentElement.scrollWidth - w, spills };
}

/** Tab through the page: each focused control with no visible ring. */
async function focusWalk(p: Page): Promise<string[]> {
  const bare: string[] = [];
  await p.locator("body").click({ position: { x: 1, y: 1 } });
  const seen = new Set<string>();
  for (let i = 0; i < 70; i++) {
    await p.keyboard.press("Tab");
    const f = await p.evaluate(() => {
      const el = document.activeElement as HTMLElement | null;
      if (!el || el === document.body) return null;
      const cs = getComputedStyle(el);
      const ring = (cs.outlineStyle !== "none" && Number.parseFloat(cs.outlineWidth) > 0) || (cs.boxShadow !== "none" && cs.boxShadow !== "");
      return { id: `${el.tagName.toLowerCase()} "${(el.innerText || el.getAttribute("aria-label") || el.getAttribute("placeholder") || "").replace(/\s+/g, " ").slice(0, 40)}"`, ring };
    });
    if (!f) continue;
    if (seen.has(f.id)) break;
    seen.add(f.id);
    if (!f.ring) bare.push(f.id);
  }
  return bare;
}

async function settle(p: Page) {
  await p.locator("h1").first().waitFor({ timeout: 15_000 }).catch(() => {});
  await p.evaluate(() => document.fonts.ready);
  await p.waitForTimeout(900);
}

const report: Record<string, unknown> = {};
try {
  await seed();
  // A few calls of the app's, so Live and Home have something.
  run(["sqs", "list-queues", "--max-items", "5"]);
  run(["dynamodb", "describe-table", "--table-name", "orders"]);
  run(["s3api", "get-object", "--bucket", "product-images", "--key", "missing.png", join(tmpdir(), "x")]);
  for (const name of only.split(",") as (keyof typeof engines)[]) {
    const b = await engines[name].launch();
    for (const width of sizes.split(",").map(Number)) {
      for (const mode of ["light", "dark"] as const) {
        const dir = `${out}/${name}/${width}/${mode}`;
        mkdirSync(dir, { recursive: true });
        const ctx = await b.newContext({ viewport: { width, height: 900 }, reducedMotion: "reduce" });
        await ctx.addInitScript((m) => localStorage.setItem("doze-aws.theme", m), mode);
        const p = await ctx.newPage();
        for (const [page, path] of PAGES) {
          const started = Date.now();
          await p.goto(`${server.console}${path}`);
          await settle(p);
          const took = Date.now() - started;
          const m = await p.evaluate(measure);
          await p.screenshot({ path: `${dir}/${page}.png` });
          const entry: Record<string, unknown> = { took, ...m };
          if (flags.includes("focus") && mode === "light") entry.bare = await focusWalk(p);
          report[`${name}/${width}/${mode}/${page}`] = entry;
        }
        await ctx.close();
      }
    }
    if (flags.includes("down")) {
      const ctx = await b.newContext({ viewport: { width: 1440, height: 900 }, reducedMotion: "reduce" });
      const p = await ctx.newPage();
      for (const [, path] of PAGES) {
        await p.goto(`${server.console}${path}`);
        await settle(p);
      }
      await p.goto(`${server.console}/`);
      await settle(p);
      await server.stop();
      mkdirSync(`${out}/${name}/down`, { recursive: true });
      for (const [page, path] of PAGES) {
        await p.evaluate((to) => {
          window.history.pushState({}, "", to);
          window.dispatchEvent(new PopStateEvent("popstate"));
        }, path);
        await p.waitForTimeout(2500);
        await p.screenshot({ path: `${out}/${name}/down/${page}.png` });
      }
      await ctx.close();
    }
    await b.close();
  }
} finally {
  await server.stop();
  writeFileSync(`${out}/report.json`, JSON.stringify(report, null, 1));
}

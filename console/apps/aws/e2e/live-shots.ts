// Screenshots of Live for a person to look at: a real doze-aws of its own (ports from the environment, else 42031 and 42032),
// traffic made with the real AWS CLI (two keys; SNS delivering to SQS; an EventBridge rule; repeats; an error, a call that is
// not built, a call soft IAM would deny), then the map, the wire, both, the filters and the drawer of a call with a cause
// chain, light and dark, in each browser.
//   bun e2e/live-shots.ts <out dir> [firefox,webkit,chromium]
import { mkdirSync } from "node:fs";
import { chromium, firefox, type Page, webkit } from "@playwright/test";
import { aws, start } from "./server";

const [out = "live-shots", only = "firefox,webkit,chromium"] = process.argv.slice(2);
const listen = Number(process.env.DOZE_AWS_SHOTS_PORT ?? 42031);
const server = await start(listen, listen + 1);
const engines = { firefox, webkit, chromium };

function run(args: string[], key = "AKIAORDERSAPP0000001") {
  return aws(server.endpoint, args, { accessKey: key });
}

try {
  const acct = server.account;
  run(["sqs", "create-queue", "--queue-name", "pick-lists"]);
  run(["sqs", "create-queue", "--queue-name", "audit"]);
  run(["sns", "create-topic", "--name", "order-placed"]);
  const topic = `arn:aws:sns:us-east-1:${acct}:order-placed`;
  run(["sns", "subscribe", "--topic-arn", topic, "--protocol", "sqs", "--notification-endpoint", `arn:aws:sqs:us-east-1:${acct}:pick-lists`]);
  run(["events", "put-rule", "--name", "audit-everything", "--event-pattern", '{"source":["larder.payments"]}']);
  run(["events", "put-targets", "--rule", "audit-everything", "--targets", `Id=1,Arn=arn:aws:sqs:us-east-1:${acct}:audit`]);
  run(["dynamodb", "create-table", "--table-name", "orders", "--attribute-definitions", "AttributeName=id,AttributeType=S", "--key-schema", "AttributeName=id,KeyType=HASH", "--billing-mode", "PAY_PER_REQUEST"]);
  run(["s3api", "create-bucket", "--bucket", "product-images"]);
  for (let i = 0; i < 3; i++) run(["sns", "publish", "--topic-arn", topic, "--message", `order ${i}`]);
  for (let i = 0; i < 2; i++) run(["events", "put-events", "--entries", JSON.stringify([{ Source: "larder.payments", DetailType: "settled", Detail: JSON.stringify({ n: i }) }])], "AKIAPAYMENTS00000001");
  const url = `${server.endpoint}/${acct}/pick-lists`;
  for (let i = 0; i < 4; i++) run(["sqs", "receive-message", "--queue-url", url]);
  run(["dynamodb", "put-item", "--table-name", "orders", "--item", '{"id":{"S":"o-1"}}']);
  run(["s3api", "put-object", "--bucket", "product-images", "--key", "sku/tomato.jpg", "--body", import.meta.filename]);
  run(["sqs", "get-queue-url", "--queue-name", "pick-list"]);
  run(["ssm", "describe-instance-information"]);
  // A user with no policy: soft IAM lets its call through and marks it.
  run(["iam", "create-user", "--user-name", "catalog"]);
  const made = JSON.parse(run(["iam", "create-access-key", "--user-name", "catalog"]).out || "{}") as { AccessKey?: { AccessKeyId: string; SecretAccessKey: string } };
  const key = made.AccessKey;
  if (key) aws(server.endpoint, ["s3api", "list-objects-v2", "--bucket", "product-images"], { accessKey: key.AccessKeyId });
  await new Promise((r) => setTimeout(r, 1500));

  async function settle(p: Page) {
    await p.getByTestId("live-status").filter({ hasText: "Live" }).waitFor();
    await p.evaluate(() => document.fonts.ready);
    await p.waitForTimeout(500);
  }

  for (const name of only.split(",") as (keyof typeof engines)[]) {
    const b = await engines[name].launch();
    mkdirSync(`${out}/${name}`, { recursive: true });
    for (const mode of ["light", "dark"] as const) {
      const ctx = await b.newContext({ viewport: { width: 1440, height: 900 }, reducedMotion: "reduce" });
      await ctx.addInitScript((m) => localStorage.setItem("doze-aws.theme", m), mode);
      const p = await ctx.newPage();
      for (const view of ["map", "wire", "both"] as const) {
        await p.goto(`${server.console}/live?view=${view}`);
        await settle(p);
        await p.screenshot({ path: `${out}/${name}/live-${view}.${mode}.png`, fullPage: view === "both" });
      }
      await p.goto(`${server.console}/live?view=wire&service=sqs&outcome=ok`);
      await settle(p);
      await p.screenshot({ path: `${out}/${name}/live-filters.${mode}.png` });
      // The drawer of a call SNS made, with its cause chain.
      await p.goto(`${server.console}/live?view=both`);
      await settle(p);
      const vias = p.locator('[data-slot="data-table"] tbody tr[data-row]').filter({ has: p.getByTestId("via-pill") });
      for (const [file, row] of [["live-drawer-cause", vias.filter({ hasText: "SNS" }).first()], ["live-drawer-rule", vias.filter({ hasText: "EventBridge" }).first()]] as const) {
        await row.click();
        await p.getByTestId("cause-chain").waitFor();
        await p.waitForTimeout(400);
        await p.screenshot({ path: `${out}/${name}/${file}.${mode}.png` });
        await p.keyboard.press("Escape");
        await p.getByTestId("call-sheet").waitFor({ state: "detached" });
      }
      // A run of repeats, opened.
      await p.goto(`${server.console}/live?view=wire`);
      await settle(p);
      await p.getByTestId("repeat-count").first().click();
      await p.waitForTimeout(300);
      await p.screenshot({ path: `${out}/${name}/live-repeats.${mode}.png` });
      // A row's menu.
      await p.getByTestId("row-menu").first().click();
      await p.getByTestId("copy-cli").waitFor();
      await p.waitForTimeout(250);
      await p.screenshot({ path: `${out}/${name}/live-row-menu.${mode}.png` });
      await ctx.close();
    }
    await b.close();
    console.log(`${name}: done`);
  }
} finally {
  await server.stop();
}

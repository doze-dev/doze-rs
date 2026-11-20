// Screenshots of Home, Connect, Access, Lab and Settings for a person to look at: a real doze-aws of its own (soft IAM; ports
// from the environment, else 42410 and 42411) and one enforcing (the next two ports), called with the real AWS CLI as an app
// and as an IAM user with no policy; then each page, dialog and state, light and dark, in each browser.
//   bun e2e/space-shots.ts <out dir> [firefox,webkit,chromium]
import { mkdirSync } from "node:fs";
import { chromium, firefox, type Page, webkit } from "@playwright/test";
import { aws, type Running, start } from "./server";

const [out = "space-shots", only = "firefox,webkit,chromium"] = process.argv.slice(2);
const listen = Number(process.env.DOZE_AWS_SHOTS_PORT ?? 42410);
const engines = { firefox, webkit, chromium };
const APP = "AKIALARDERAPP0000001";

function run(server: Running, args: string[], accessKey = APP) {
  const r = aws(server.endpoint, args, { accessKey });
  if (r.code !== 0 && accessKey === APP) console.warn(`aws ${args.join(" ")}: ${r.out.slice(0, 200)}`);
  return r;
}

/** An IAM user with no policy, and its key. */
function nobody(server: Running, name: string): string {
  run(server, ["iam", "create-user", "--user-name", name]);
  const made = run(server, ["iam", "create-access-key", "--user-name", name]);
  return (JSON.parse(made.out) as { AccessKey: { AccessKeyId: string } }).AccessKey.AccessKeyId;
}

function seed(server: Running) {
  const acct = server.account;
  run(server, ["sqs", "create-queue", "--queue-name", "pick-lists-dlq"]);
  const policy = JSON.stringify({ deadLetterTargetArn: `arn:aws:sqs:us-east-1:${acct}:pick-lists-dlq`, maxReceiveCount: 5 });
  run(server, ["sqs", "create-queue", "--queue-name", "pick-lists", "--attributes", JSON.stringify({ RedrivePolicy: policy })]);
  run(server, ["sqs", "send-message", "--queue-url", `${server.endpoint}/${acct}/pick-lists-dlq`, "--message-body", '{"order_id":"ord-0371"}']);
  run(server, ["sqs", "send-message", "--queue-url", `${server.endpoint}/${acct}/pick-lists`, "--message-body", '{"order_id":"ord-0372"}']);
  run(server, ["s3api", "create-bucket", "--bucket", "product-images"]);
  for (let i = 0; i < 3; i++) run(server, ["sqs", "get-queue-url", "--queue-name", "pick-list"]);
  const report = nobody(server, "report-job");
  run(server, ["sqs", "list-queues"], report);
  run(server, ["s3api", "list-objects-v2", "--bucket", "product-images"], report);
  run(server, ["sqs", "send-message", "--queue-url", `${server.endpoint}/${acct}/pick-lists`, "--message-body", "x"], report);
}

async function settle(p: Page, title: string) {
  await p.locator("h1", { hasText: title }).first().waitFor();
  await p.evaluate(() => document.fonts.ready);
  await p.waitForTimeout(700);
}

const soft = await start(listen, listen + 1);
const enforce = await start(listen + 2, listen + 3, { args: ["--iam", "enforce"] });
try {
  seed(soft);
  seed(enforce);
  for (const name of only.split(",") as (keyof typeof engines)[]) {
    const b = await engines[name].launch();
    mkdirSync(`${out}/${name}`, { recursive: true });
    for (const mode of ["light", "dark"] as const) {
      const ctx = await b.newContext({ viewport: { width: 1440, height: 900 }, reducedMotion: "reduce" });
      await ctx.addInitScript((m) => localStorage.setItem("doze-aws.theme", m), mode);
      const p = await ctx.newPage();
      const shot = (file: string, fullPage = false) => p.screenshot({ path: `${out}/${name}/${file}.${mode}.png`, fullPage });
      const url = soft.console;
      await p.goto(`${url}/`);
      await settle(p, "Home");
      await p.getByTestId("signal").first().waitFor();
      await shot("home", true);
      await p.goto(`${url}/connect`);
      await settle(p, "Connect your app");
      await shot("connect", true);
      await p.getByTestId("tab-code").click();
      await p.waitForTimeout(300);
      await shot("connect-code", true);
      await p.goto(`${url}/access`);
      await settle(p, "Access");
      await p.getByTestId("denial-group").first().waitFor();
      await shot("access", true);
      await p.getByTestId("make-pass").first().click();
      await p.getByTestId("pass-dialog").waitFor();
      await p.waitForTimeout(350);
      await shot("access-pass");
      await p.goto(`${enforce.console}/access`);
      await settle(p, "Access");
      await p.getByTestId("denial-group").first().waitFor();
      await shot("access-enforce");
      await p.goto(`${url}/lab?api=sqs&op=CreateQueue`);
      await settle(p, "Lab");
      await p.getByTestId("lab-input").fill('{\n  "QueueName": "from-the-lab"\n}');
      await p.getByTestId("lab-send").click();
      await p.getByTestId("lab-answer").waitFor();
      await shot("lab", true);
      await p.goto(`${url}/settings`);
      await settle(p, "Settings");
      await shot("settings", true);
      await p.getByTestId("reset-open").click();
      await p.getByTestId("reset-dialog").waitFor();
      await p.waitForTimeout(500);
      await shot("settings-reset");
      await p.goto(`${url}/live?view=wire`);
      await settle(p, "Live");
      await p.waitForTimeout(500);
      await shot("live-callers");
      await ctx.close();
    }
    await b.close();
  }
} finally {
  await soft.stop();
  await enforce.stop();
}
console.log("shots in", out);

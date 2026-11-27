// What the third audit found of Home, Live and Connect, each against a real doze-aws of its own (one per browser) called with the real AWS CLI:
// the wire's columns, the map fitting its box and the node picked kept in view, By domain as framed columns with lines that pass over them,
// the tabs of Connect, its callouts and its Waiting state, the Home subtitle and the Activities crumb.
import { aws, start, type Running } from "../server";
import { expectCrumbsEnd } from "./crumbs-kit";
import { expect, test } from "./fixtures";

const BASE = Number(process.env.E2E_BASE_PORT ?? 46100);
const at = (b: string) => BASE + ({ firefox: 10, webkit: 20, chromium: 30 }[b] ?? 40);
const KEY = "AKIAFIXLIVE0000000001";

test.describe.configure({ mode: "serial" });

let server: Running;
let fresh: Running;
const cli = (args: string[]) => {
  const r = aws(server.endpoint, args, { accessKey: KEY });
  expect(r.code, `${args.join(" ")}: ${r.out}`).toBe(0);
  return r.out;
};

test.beforeAll(async ({ browserName }) => {
  server = await start(at(browserName), at(browserName) + 1);
  fresh = await start(at(browserName) + 2, at(browserName) + 3);
  // An app that reads and writes a queue with a dead-letter queue, a bucket, and a refusal and an operation not built.
  const q = `${server.endpoint}/${server.account}`;
  cli(["sqs", "create-queue", "--queue-name", "pick-lists-dlq"]);
  const dlq = JSON.parse(cli(["sqs", "get-queue-attributes", "--queue-url", `${q}/pick-lists-dlq`, "--attribute-names", "QueueArn"])).Attributes.QueueArn as string;
  cli(["sqs", "create-queue", "--queue-name", "pick-lists", "--attributes", JSON.stringify({ RedrivePolicy: JSON.stringify({ deadLetterTargetArn: dlq, maxReceiveCount: "5" }) })]);
  cli(["sqs", "send-message", "--queue-url", `${q}/pick-lists`, "--message-body", "hello"]);
  // A topic that feeds the queue: the chain app, topic, queue, dead-letter queue is four columns, more than a 1440 px window holds at full size.
  const topic = JSON.parse(cli(["sns", "create-topic", "--name", "orders-created"])).TopicArn as string;
  const queueArn = JSON.parse(cli(["sqs", "get-queue-attributes", "--queue-url", `${q}/pick-lists`, "--attribute-names", "QueueArn"])).Attributes.QueueArn as string;
  cli(["sns", "subscribe", "--topic-arn", topic, "--protocol", "sqs", "--notification-endpoint", queueArn]);
  cli(["sns", "publish", "--topic-arn", topic, "--message", "m"]);
  cli(["s3api", "create-bucket", "--bucket", "product-images"]);
  cli(["s3api", "put-object", "--bucket", "product-images", "--key", "a.jpg", "--body", import.meta.filename]);
  expect(aws(server.endpoint, ["s3api", "get-object", "--bucket", "no-bucket", "--key", "k", "/dev/null"], { accessKey: KEY }).code).not.toBe(0);
  expect(aws(server.endpoint, ["ssm", "describe-instance-information"], { accessKey: KEY }).code).not.toBe(0);
});
test.afterAll(async () => {
  await fresh.stop();
  await server.stop();
});

const live = async (page: import("@playwright/test").Page, query: string) => {
  await page.goto(`${server.console}/live${query}`);
  await expect(page.getByTestId("live-status")).toHaveText("Live");
};

test("the wire has the board's columns and a title, and its results and resources are whole at 1440", async ({ page }) => {
  await live(page, "?view=wire");
  await expect(page.getByTestId("wire-title")).toHaveText("Wire newest first");
  await expect(page.getByRole("table", { name: "Calls, newest first" }).locator("th")).toHaveText(["Time", "Caller", "Operation", "Resource", "Result", "Latency", "Actions"]);
  const row = (op: string) => page.locator("tbody tr[data-row]").filter({ has: page.getByRole("button", { name: new RegExp(`: ${op}$`) }) }).first();
  const whole = (td: import("@playwright/test").Locator) => td.evaluate((e) => Math.max(0, e.scrollWidth - e.clientWidth));
  // The words of a result and the name of a resource are not cut.
  const refused = row("GetObject");
  await expect(refused.locator("td").nth(4)).toContainText(/REFUSED\s*404\s*NoSuchBucket/);
  expect(await whole(refused.locator("td").nth(4))).toBe(0);
  const notBuilt = row("DescribeInstanceInformation");
  await expect(notBuilt.locator("td").nth(4)).toContainText(/NOT BUILT\s*501\s*NotImplemented/);
  expect(await whole(notBuilt.locator("td").nth(4))).toBe(0);
  const put = row("PutObject");
  await expect(put.locator("td").nth(3)).toContainText("product-images/a.jpg");
  expect(await whole(put.locator("td").nth(3))).toBe(0);
});

test("the map fits its box at 1440 with every node on screen, and the node picked is whole beside its panel", async ({ page }) => {
  await live(page, "?view=map");
  await expect(page.getByTestId("map-resource").first()).toBeVisible();
  const scroll = page.getByTestId("map-scroll");
  expect(await scroll.evaluate((e) => e.scrollWidth - e.clientWidth)).toBeLessThanOrEqual(0);
  const edge = (await scroll.boundingBox())!;
  // Four columns (app, topic, queue, dead-letter queue): the cards are shrunk to fit, not left to run past the edge.
  const x = async (node: string) => (await page.locator(`[data-testid="map-resource"][data-node="${node}"]`).boundingBox())!;
  const [topic, queue, dead] = [await x(`sns/topics/orders-created`), await x("sqs/queues/pick-lists"), await x("sqs/queues/pick-lists-dlq")];
  expect(topic.x).toBeLessThan(queue.x);
  expect(queue.x).toBeLessThan(dead.x);
  expect(queue.width).toBeLessThan(190);
  for (const card of await page.locator('[data-testid="map-resource"], [data-testid="map-app"]').all()) {
    const b = (await card.boundingBox())!;
    expect(b.x + b.width, "a card is cut at the right edge").toBeLessThanOrEqual(edge.x + edge.width + 0.5);
  }
  await page.locator('[data-testid="map-resource"][data-node="sqs/queues/pick-lists"]').click();
  const panel = page.getByTestId("node-panel");
  await expect(panel).toBeVisible();
  await expect(page.getByTestId("panel-title")).toHaveText("pick-lists");
  // Docked at the right: the card is whole inside the map's box and ends before the panel begins.
  await expect.poll(async () => {
    const node = (await page.locator('[data-testid="map-resource"][data-node="sqs/queues/pick-lists"]').boundingBox())!;
    const box = (await scroll.boundingBox())!;
    const p = (await panel.boundingBox())!;
    return node.x >= box.x - 0.5 && node.x + node.width <= box.x + box.width + 0.5 && node.x + node.width <= p.x + 0.5;
  }).toBe(true);
  // A name in the panel is not broken inside the name.
  const link = page.getByTestId("panel-link").filter({ hasText: "pick-lists-dlq" });
  await expect(link).toBeVisible();
  expect((await link.boundingBox())!.height).toBeLessThan(26);
});

test("By domain: a framed panel for each column, and a line that skips a column runs over it, not behind its cards", async ({ page }) => {
  await live(page, "?layout=domain");
  // The columns this record needs (Other holds what is none of the three: the Systems Manager call that is not built).
  await expect(page.getByTestId("domain-head")).toContainText(["Apps", "Messaging and events", "Storage and compute", "Other"]);
  await expect(page.getByTestId("domain-panel")).toHaveCount(4);
  const routed = await page.evaluate(() => {
    const svg = document.querySelector('[data-testid="resource-map"] svg')!;
    const top = svg.getBoundingClientRect().top;
    const cards = [...document.querySelectorAll('[data-testid="map-resource"]')].map((c) => c.getBoundingClientRect().top - top);
    const paths = [...document.querySelectorAll('path[data-edge]')].filter((p) => (p.getAttribute("d") ?? "").includes("Q"));
    return { cardsTop: Math.min(...cards), paths: paths.map((p) => (p as SVGPathElement).getBBox().y) };
  });
  expect(routed.paths.length, "the app's call to the bucket skips the middle column").toBeGreaterThan(0);
  for (const y of routed.paths) expect(y).toBeLessThan(routed.cardsTop);
});

test("Home does not say a placeholder version, and the Activities list ends its crumb on Activities", async ({ page }) => {
  await page.goto(`${server.console}/`);
  await expect(page.locator("h1").first()).toHaveText("Home");
  await expect(page.getByTestId("home-sub")).toContainText(`account ${server.account}`);
  await expect(page.getByTestId("home-sub")).not.toContainText("0.0.0");
  await page.goto(`${server.console}/resources/stepfunctions/activities`);
  await expect(page.locator("h1").first()).toHaveText("Activities");
  // The service's crumb comes from the catalogue, a moment after the title in a browser that is slow to read it.
  await expect(page.getByRole("navigation", { name: "Breadcrumb" }).locator("li")).toHaveCount(4);
  await expectCrumbsEnd(page, ["Step Functions", "Activities"]);
});

test("Connect: the subtitle and the crumb follow the tab, the hints are callouts, and Waiting is the board's with a Back button", async ({ page }) => {
  await page.goto(`${server.console}/connect`);
  await expect(page.locator("h1").first()).toHaveText("Connect your app");
  await expectCrumbsEnd(page, ["Home", "Connect your app"]);
  // The two callouts are side by side, then the check (its table, then a footer row with the buttons), then the closing callout.
  const keys = (await page.getByTestId("connect-keys").boundingBox())!;
  const print = (await page.getByTestId("connect-env-cmd").boundingBox())!;
  expect(Math.abs(keys.y - print.y)).toBeLessThan(2);
  expect(print.x).toBeGreaterThan(keys.x + keys.width - 1);
  for (const id of ["connect-one", "connect-keys", "connect-env-cmd", "connect-follows"]) await expect(page.getByTestId(id)).toHaveAttribute("data-slot", "callout");
  const check = (await page.getByTestId("connect-check").boundingBox())!;
  expect(check.y).toBeGreaterThan(keys.y);
  expect((await page.getByTestId("connect-follows").boundingBox())!.y).toBeGreaterThan(check.y);
  const table = (await page.getByTestId("connect-check").getByRole("table").boundingBox())!;
  const again = (await page.getByTestId("check-again").boundingBox())!;
  expect(again.y).toBeGreaterThan(table.y + table.height - 1);

  await page.getByTestId("tab-code").click();
  await expect(page.getByText("The same call, list the queues, from each client.", { exact: true })).toBeVisible();
  await expectCrumbsEnd(page, ["Home", "Connect your app", "Code"]);
  await expect(page.getByTestId("connect-s3")).toHaveAttribute("data-slot", "callout");
  await page.getByTestId("tab-services").click();
  await expect(page.getByText("Where each service answers, on IP and port.", { exact: true })).toBeVisible();
  await expectCrumbsEnd(page, ["Home", "Connect your app", "By service"]);
  await expect(page.getByTestId("services-lede")).toHaveAttribute("data-slot", "callout");
  // A service's name is a link to its pages; the variable has no copy button of its own (the address does).
  const sqs = page.getByTestId("service-link").first();
  await expect(sqs).toHaveText("SQS");
  await expect(sqs).toHaveAttribute("href", "/resources/sqs");
  const row = page.getByRole("row").filter({ hasText: "AWS_ENDPOINT_URL_SQS" });
  await expect(row.getByRole("button")).toHaveCount(1);
  await expect(row.locator("td").nth(2).getByRole("button")).toHaveCount(0);

  // An instance nothing has called: Waiting, with one row and the command, and Back to Connect a button once the check was started over.
  await page.goto(`${fresh.console}/connect`);
  await expect(page.getByTestId("connect-waiting")).toBeVisible();
  await expect(page.getByTestId("connect-check")).toContainText("Waiting for the first call");
  await expect(page.getByTestId("connect-probe")).toContainText(`--endpoint-url ${fresh.endpoint}`);
  await expect(page.getByTestId("check-back")).toHaveCount(0);
  await page.getByTestId("check-again").click();
  await expect(page.getByRole("button", { name: "Back to Connect" })).toBeVisible();
  await page.getByRole("button", { name: "Back to Connect" }).click();
  await expect(page.getByRole("button", { name: "Back to Connect" })).toHaveCount(0);
});

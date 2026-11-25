// What the third audit found of the shell and the shared parts, each against a real doze-aws of its own (one per browser) called with the real AWS CLI:
// the Recent calls table, the rail above its footer, IAM and STS under Access, and the call panel of a form whose JSON is not JSON yet.
import { aws, start, type Running } from "../server";
import { expectCrumbsEnd } from "./crumbs-kit";
import { advance, expect, expectNoErrorAnswers, test } from "./fixtures";

const BASE = Number(process.env.E2E_BASE_PORT ?? 45900);
const at = (b: string) => BASE + ({ firefox: 10, webkit: 20, chromium: 30 }[b] ?? 40);

test.describe.configure({ mode: "serial" });

let server: Running;
const cli = (args: string[], key = "AKIAFIXSHELL000000001") => {
  const r = aws(server.endpoint, args, { accessKey: key });
  expect(r.code, `${args.join(" ")}: ${r.out}`).toBe(0);
  return r.out;
};
const go = async (page: import("@playwright/test").Page, path: string, title: string | RegExp) => {
  await page.goto(`${server.console}${path}`);
  await expect(page.locator("h1").first()).toHaveText(title);
};

test.beforeAll(async ({ browserName }) => {
  server = await start(at(browserName), at(browserName) + 1);
});
test.afterAll(async () => {
  await server.stop();
});

test("Recent calls: the board's columns, this console's reads hidden and its writes kept, and a box that ends on a row", async ({ page }) => {
  // A queue made by the console is a write of its own, in view at once; the app's calls on another queue are many.
  await go(page, "/resources/sqs/queues", "Queues");
  await page.getByTestId("create-open").click();
  await page.getByTestId("queue-name").fill("made-here");
  await advance(page);
  await expect(page.locator("h1")).toHaveText("made-here");
  const calls = page.getByTestId("recent-calls");
  await expect(calls).toContainText("CreateQueue");
  await expect(calls.getByTestId("call-caller").first()).toContainText("This console");
  // The page's own reads are in the record by its next look: hidden, said, and shown on request.
  await page.reload();
  await expect(page.locator("h1")).toHaveText("made-here");
  await expect(page.getByTestId("calls-console-hidden")).toContainText(/\d+ made by this console (is|are) hidden\./);
  const table = page.getByRole("table", { name: "Recent calls" });
  await expect(table.locator("th")).toHaveText(["Time", "Caller", "Call", "Result"]);
  const reads = table.locator("tbody tr").filter({ hasText: /(List|Describe|Get)[A-Z]\w+/ });
  await expect(reads).toHaveCount(0);
  await expect(table.locator("tbody tr").filter({ hasText: "CreateQueue" })).toHaveCount(1);
  await page.getByTestId("calls-console-toggle").click();
  await expect(page.getByTestId("calls-console-hidden")).toContainText(/\d+ made by this console (is|are) listed\./);
  await expect(reads.first()).toBeVisible();
  await page.getByTestId("calls-console-toggle").click();
  await expect(reads).toHaveCount(0);

  // Twelve calls of an app: ten rows are in view, the box ends on the tenth, and the table says how many more.
  cli(["sqs", "create-queue", "--queue-name", "busy"]);
  for (let i = 0; i < 12; i++) cli(["sqs", "send-message", "--queue-url", `${server.endpoint}/${server.account}/busy`, "--message-body", `m${i}`]);
  await go(page, "/resources/sqs/queues/busy", "busy");
  const box = page.getByTestId("recent-calls").locator('[data-slot="data-table"]');
  await expect(box.locator("tbody tr")).not.toHaveCount(0);
  const m = await box.evaluate((el) => {
    const head = el.querySelector("thead")!.getBoundingClientRect().height;
    const row = el.querySelector("tbody tr")!.getBoundingClientRect().height;
    return { rows: (el.clientHeight - head) / row, scrolls: el.scrollHeight > el.clientHeight };
  });
  expect(m.scrolls).toBe(true);
  expect(m.rows).toBeCloseTo(10, 1);
  await expect(page.getByTestId("calls-more")).toContainText(/^The newest 10 are in view; scroll the table for \d+ more\.$/);
});

test("the rail's Pinned and Recent scroll above the footer and never run under it, at 900 and 700 px tall", async ({ page }) => {
  const names = ["rail-a", "rail-b", "rail-c", "rail-d", "rail-e", "rail-f"];
  for (const n of names) cli(["sqs", "create-queue", "--queue-name", n]);
  for (const n of names) await go(page, `/resources/sqs/queues/${n}`, n);
  await page.getByTestId("pin-toggle").click();
  await go(page, "/resources/sqs/queues/rail-a", "rail-a");
  await page.getByTestId("pin-toggle").click();
  for (const height of [900, 700]) {
    await page.setViewportSize({ width: 1440, height });
    await go(page, "/resources/sqs/queues/rail-b", "rail-b");
    const scroller = page.getByTestId("rail-scroll");
    const footer = page.getByTestId("rail-footer");
    // The rail's own scroller ends where the footer begins.
    const edge = await page.evaluate(() => ({ scroller: document.querySelector("[data-testid=rail-scroll]")!.getBoundingClientRect().bottom, footer: document.querySelector("[data-testid=rail-footer]")!.getBoundingClientRect().top }));
    expect(edge.scroller).toBeLessThanOrEqual(edge.footer + 1);
    // It overflows (the services are listed under Resources), and says so at its bottom edge until it is scrolled to the end.
    const over = await scroller.evaluate((el) => el.scrollHeight - el.clientHeight);
    expect(over, `the rail overflows at ${height} px`).toBeGreaterThan(8);
    await expect.poll(() => scroller.evaluate((el) => (el as HTMLElement).style.getPropertyValue("--fade-bottom"))).not.toBe("0px");
    await scroller.evaluate((el) => (el.scrollTop = el.scrollHeight));
    await expect.poll(() => scroller.evaluate((el) => (el as HTMLElement).style.getPropertyValue("--fade-bottom"))).toBe("0px");
    // Scrolled to the end, every pinned and recent place is whole above the footer.
    const top = (await footer.boundingBox())!.y;
    const places = page.getByTestId("rail-place");
    expect(await places.count()).toBeGreaterThanOrEqual(6);
    for (const box of await places.evaluateAll((els) => els.map((e) => e.getBoundingClientRect().bottom))) expect(box).toBeLessThanOrEqual(top + 0.5);
  }
});

test("IAM and STS are Access: the rail marks it, the crumbs begin Access, and the cards of Access lead there", async ({ page, refusals }) => {
  cli(["iam", "create-user", "--user-name", "fixer"]);
  const key = JSON.parse(cli(["iam", "create-access-key", "--user-name", "fixer"])).AccessKey.AccessKeyId as string;
  const marked = async () => {
    await expect(page.getByTestId("space-access")).toHaveAttribute("aria-current", "page");
    await expect(page.getByTestId("space-resources")).not.toHaveAttribute("aria-current", "page");
    // The Resources space is not opened up with the services listed: IAM is not one of them.
    await expect(page.getByTestId("rail-services")).toHaveCount(0);
  };
  const first = async () => expect(page.getByRole("navigation", { name: "Breadcrumb" }).locator("li").first()).toHaveText("Access");
  await go(page, "/access", "Access");
  await page.getByTestId("access-iam").click();
  await expect(page.locator("h1").first()).toHaveText("IAM Dashboard");
  await marked();
  await first();
  await expectCrumbsEnd(page, ["Access", "IAM"]);
  await go(page, "/resources/iam/users", "Users");
  await marked();
  await expectCrumbsEnd(page, ["Access", "IAM", "Users"]);
  for (const [path, title, end] of [["/resources/iam/groups", "User groups", ["Access", "IAM", "User groups"]], ["/resources/iam/roles", "Roles", ["Access", "IAM", "Roles"]], ["/resources/iam/policies", "Policies", ["Access", "IAM", "Policies"]], ["/resources/iam/users/fixer", "fixer", ["Access", "IAM", "Users", "fixer"]]] as const) {
    await go(page, path, title);
    await marked();
    await expectCrumbsEnd(page, [...end]);
  }
  await go(page, "/access", "Access");
  await page.getByTestId("access-sts").click();
  await expect(page.locator("h1").first()).toHaveText("Switch Role");
  await marked();
  await expectCrumbsEnd(page, ["Access", "STS"]);

  // Deleting a user that holds a key reads nothing of the gone key: no 404 reaches the browser.
  await go(page, "/resources/iam/users", "Users");
  await expect(page.getByRole("row", { name: new RegExp(`fixer .*Active - ${key.slice(0, 4)}`) })).toBeVisible();
  await page.getByTestId("select-fixer").click();
  await page.getByTestId("action-delete").click();
  await expect(page.getByTestId("delete-dialog")).toContainText(`1 access key will be deleted`);
  await page.getByTestId("delete-dialog-input").fill("fixer");
  await page.getByTestId("delete-dialog-submit").click();
  await expect(page.getByTestId("deleted-banner")).toContainText("User fixer deleted.");
  await expectNoErrorAnswers(page, refusals);
});

test("a JSON field that is not JSON yet is no member of the call, Save is off, and it is said once; no JSON editor has a Format button", async ({ page }) => {
  cli(["sqs", "create-queue", "--queue-name", "policed"]);
  await go(page, "/resources/sqs/queues/policed?view=edit", "Edit policed");
  const editor = page.getByTestId("policy-json");
  await editor.fill('{ "Version": "2012-10-17", "Stat');
  await expect(page.getByTestId("form-submit")).toBeDisabled();
  await expect(page.getByText("Not JSON")).toHaveCount(1);
  await expect(page.getByTestId("disabled-why")).toContainText("Fix the JSON policy to save.");
  // Whatever the panel last drew, it holds no Policy once the pause is over.
  await expect(page.getByTestId("call-panel-code")).not.toContainText("Policy", { timeout: 6_000 });
  await expect(page.getByTestId("policy-json-format")).toHaveCount(0);
  await editor.fill('{"Version":"2012-10-17","Statement":[]}');
  await expect(page.getByTestId("form-submit")).toBeEnabled();
  await expect(page.getByTestId("call-panel-code")).toContainText('"Policy"');

  // The dead-letter switch on before a queue is chosen sends no RedrivePolicy.
  await go(page, "/resources/sqs/queues", "Queues");
  await page.getByTestId("create-open").click();
  await expect(page.locator("h1").first()).toHaveText("Create queue");
  await page.getByTestId("queue-name").fill("early-dlq");
  await page.getByTestId("dlq-on").click();
  await expect(page.getByTestId("call-panel-code")).toContainText("CreateQueue");
  await expect(page.getByTestId("call-panel-code")).not.toContainText("RedrivePolicy");

  // The same for a topic's policy.
  await go(page, "/resources/sns/topics?create=yes", "Create topic");
  await page.getByTestId("topic-name").fill("half-policy");
  await page.getByTestId("policy-json").fill('{ "Version": ');
  await expect(page.getByTestId("form-submit")).toBeDisabled();
  await expect(page.getByText("Not JSON")).toHaveCount(1);
  await expect(page.getByTestId("call-panel-code")).not.toContainText("Policy", { timeout: 6_000 });
  await expect(page.getByTestId("policy-json-format")).toHaveCount(0);
});

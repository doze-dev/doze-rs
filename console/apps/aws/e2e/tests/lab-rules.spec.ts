// Fault rules in the Lab page, against a real doze-aws of the test's own and the real AWS CLI: a rule is made in the dialog (its sentence and
// its problems are the server's), the CLI is bent by it, Live tags the calls, and Stop all lets them through.
import { aws } from "../server";
import { expect, test, unique } from "./fixtures";
import { addTo, own, sentence } from "./lab-kit";

test("a throttle rule made in the dialog fails the AWS CLI with its retries, Live tags each try, the switch and Stop all let the call through", async ({ page, browserName }) => {
  await own(page, async (server, cli) => {
    const queue = unique("rule", browserName);
    const made = cli(["sqs", "create-queue", "--queue-name", queue]);
    expect(made.code, made.out).toBe(0);
    const url = (JSON.parse(made.out) as { QueueUrl: string }).QueueUrl;
    await page.goto(`${server.console}/lab`);
    await expect(page.getByTestId("lab-in-force")).toContainText("No rules in force");

    // The dialog: a kind, the calls it fits, the sentence it would be, made.
    await page.getByTestId("rehearsal-throttle").getByTestId("rule-start").click();
    await expect(page.getByTestId("rule-dialog")).toBeVisible();
    await expect(page.getByTestId("rule-kind-throttle")).toHaveAttribute("aria-checked", "true");
    await expect(page.getByTestId("rule-lesson")).toBeVisible();
    await addTo(page, "Services", "sqs");
    await addTo(page, "Operations", "SendMessage");
    await addTo(page, "Resources", queue);
    await expect.poll(() => sentence(page)).toMatch(/Throttle.*SendMessage.*10 minutes/);
    expect(await sentence(page)).toContain(queue);
    await expect(page.getByTestId("rule-sentence")).toContainText(/SQS answers \d{3} RequestThrottled/);
    await page.getByTestId("rule-make").click();
    await expect(page.getByTestId("rule-dialog")).toHaveCount(0);
    const row = page.getByTestId("rule-row");
    await expect(row).toHaveCount(1);
    await expect(row).toContainText("Throttle");
    const id = await row.getAttribute("data-rule");
    await expect(row.getByTestId("rule-hits")).toHaveText("0");
    await expect(page.getByTestId("lab-stop-all")).toBeEnabled();

    // The AWS CLI retries a throttle as every SDK does (three attempts), then gives up with the service's code.
    const refused = cli(["sqs", "send-message", "--queue-url", url, "--message-body", "hi"], { attempts: 3 });
    expect(refused.code).not.toBe(0);
    expect(refused.out).toContain("reached max retries: 2");
    // The count moves when the page is read again.
    await page.getByRole("button", { name: /Refresh/ }).first().click();
    await expect(row.getByTestId("rule-hits")).toHaveText("3");
    // A call the rule does not fit is not touched.
    expect(cli(["sqs", "list-queues"]).code).toBe(0);

    // The rule's own page, and its calls on Live, each try tagged.
    await row.getByTestId("rule-open").click();
    await expect(page.getByTestId("rule-sheet")).toContainText("3");
    await expect(page.getByTestId("rule-calls").locator("li")).toHaveCount(3);
    await page.keyboard.press("Escape");
    await row.getByTestId("rule-watch").click();
    await expect(page).toHaveURL(new RegExp(`/live\\?.*lab=${id}`));
    const tags = page.locator('[data-slot="lab-tag"]');
    await expect(tags).toHaveCount(3);
    await expect(tags.first()).toHaveAttribute("data-effect", "failed");
    await page.goBack();

    // Switched off, it does nothing; on, it bends again.
    await row.getByTestId("rule-switch").click();
    await expect(row).toHaveAttribute("data-enabled", "false");
    expect(cli(["sqs", "send-message", "--queue-url", url, "--message-body", "off"]).code).toBe(0);
    await row.getByTestId("rule-switch").click();
    await expect(row).toHaveAttribute("data-enabled", "true");
    expect(cli(["sqs", "send-message", "--queue-url", url, "--message-body", "on"]).code).not.toBe(0);

    // Stop all: nothing in force, it is in the ended list with how it ended, and the call works.
    await page.getByTestId("lab-stop-all").click();
    await expect(page.getByTestId("rule-row")).toHaveCount(0);
    await expect(page.getByTestId("lab-in-force")).toContainText("No rules in force");
    await expect(page.getByTestId("ended-row")).toHaveCount(1);
    await expect(page.getByTestId("ended-how")).toHaveText("Stopped");
    expect(cli(["sqs", "send-message", "--queue-url", url, "--message-body", "after"]).code).toBe(0);
    // Clearing the list forgets it.
    await page.getByTestId("lab-ended-clear").click();
    await expect(page.getByTestId("lab-ended")).toHaveCount(0);
  });
});

test("a slow rule delays the call and it is served; removing it ends it; a drop rule closes the connection with no answer", async ({ page }) => {
  await own(page, async (server, cli) => {
    await page.goto(`${server.console}/lab`);
    await page.getByTestId("rehearsal-slow").getByTestId("rule-start").click();
    await addTo(page, "Services", "sqs");
    await addTo(page, "Operations", "ListQueues");
    await page.getByTestId("rule-delay").fill("700");
    await expect.poll(() => sentence(page)).toMatch(/700 ms/);
    await page.getByTestId("rule-make").click();
    await expect(page.getByTestId("rule-row")).toHaveCount(1);
    const began = Date.now();
    const listed = cli(["sqs", "list-queues"]);
    expect(listed.code, listed.out).toBe(0);
    const slowTook = Date.now() - began;
    expect(slowTook).toBeGreaterThanOrEqual(650);
    await page.getByTestId("rule-remove").click();
    await expect(page.getByTestId("rule-row")).toHaveCount(0);
    const quick = Date.now();
    expect(cli(["sqs", "list-queues"]).code).toBe(0);
    // A process launch is not a fixed time on a shared runner: what is asserted is the rule's effect, not a clock.
    expect(Date.now() - quick).toBeLessThan(slowTook - 300);

    await page.getByTestId("rehearsal-drop").getByTestId("rule-start").click();
    await expect(page.getByTestId("rule-kind-drop")).toHaveAttribute("aria-checked", "true");
    await expect(page.getByTestId("rule-delay")).toHaveCount(0);
    await addTo(page, "Services", "sqs");
    await addTo(page, "Operations", "ListQueues");
    await expect.poll(() => sentence(page)).toMatch(/Close the connection/);
    await page.getByTestId("rule-make").click();
    await expect(page.getByTestId("rule-row")).toHaveCount(1);
    const closed = cli(["sqs", "list-queues"]);
    expect(closed.code).not.toBe(0);
    await page.goto(`${server.console}/live?lab=any`);
    const none = page.locator('[data-slot="result"]', { hasText: "NO ANSWER" });
    await expect(none.first()).toBeVisible();
    await expect(none.first().locator('[data-slot="lab-tag"]')).toHaveAttribute("data-effect", "dropped");
  });
});

test("what is wrong with a rule is said in words, by the field, and the rule is not made until it is right", async ({ page }) => {
  await own(page, async (server) => {
    await page.goto(`${server.console}/lab`);
    await page.getByTestId("rehearsal-slow").getByTestId("rule-start").click();
    await page.getByTestId("rule-ends").fill("ten");
    await expect(page.getByTestId("problem-ends_in_secs")).toBeVisible();
    await expect(page.getByTestId("rule-make")).toBeDisabled();
    await page.getByTestId("rule-ends").fill("10");
    await page.getByTestId("rule-delay").fill("0");
    await expect(page.getByTestId("problem-effect")).toBeVisible();
    await expect(page.getByTestId("rule-make")).toBeDisabled();
    await page.getByTestId("rule-delay").fill("200");
    await expect(page.getByTestId("problem-effect")).toHaveCount(0);
    await expect(page.getByTestId("rule-make")).toBeEnabled();
    // A service that is not one, named by mistake.
    await addTo(page, "Services", "sqz");
    await expect(page.getByTestId("problem-services")).toBeVisible();
    await expect(page.getByTestId("rule-make")).toBeDisabled();
    // Cancel leaves nothing behind.
    await page.getByRole("button", { name: "Cancel" }).click();
    await expect(page.getByTestId("rule-dialog")).toHaveCount(0);
    await expect(page.getByTestId("rule-row")).toHaveCount(0);
  });
});

test("a recipe opens the rule dialog on its own calls, and a rule for it bends them and nothing else", async ({ page, browserName }) => {
  await own(page, async (server, cli) => {
    const queue = unique("rr", browserName);
    const other = unique("ro", browserName);
    const made = cli(["sqs", "create-queue", "--queue-name", queue]);
    expect(made.code, made.out).toBe(0);
    const url = (JSON.parse(made.out) as { QueueUrl: string }).QueueUrl;
    const otherUrl = (JSON.parse(cli(["sqs", "create-queue", "--queue-name", other]).out) as { QueueUrl: string }).QueueUrl;
    await page.goto(`${server.console}/lab`);
    // The recipe names the first queue; a rule for just it.
    const title = (await page.getByTestId("lab-recipe-sqs-send").getByTestId("lab-recipe-title").textContent()) ?? "";
    const first = title.replace("Send a message to ", "");
    await page.getByTestId("lab-recipe-sqs-send").getByTestId("lab-recipe-rehearse").click();
    await expect(page.getByTestId("rule-dialog")).toBeVisible();
    await expect(page.getByTestId("rule-match").getByTestId("multi-chips").first()).toContainText("SQS");
    await expect(page.getByTestId("rule-dialog")).toContainText("SendMessage");
    expect(await sentence(page)).toContain(first);
    await page.getByTestId("rule-kind-unavailable").click();
    await expect.poll(() => sentence(page)).toMatch(/unavailable/i);
    await page.getByTestId("rule-make").click();
    await expect(page.getByTestId("rule-row")).toHaveCount(1);
    const [bent, free] = first === queue ? [url, otherUrl] : [otherUrl, url];
    expect(cli(["sqs", "send-message", "--queue-url", bent, "--message-body", "x"]).code).not.toBe(0);
    expect(cli(["sqs", "send-message", "--queue-url", free, "--message-body", "x"]).code).toBe(0);
  });
});

test("with the Lab off, the page says why, will not make a rule, and Send a call still works", async ({ page }) => {
  await own(
    page,
    async (server) => {
      await page.goto(`${server.console}/lab`);
      await expect(page.getByTestId("lab-off")).toBeVisible();
      await expect(page.getByTestId("lab-off")).toContainText("--lab");
      await expect(page.getByTestId("rehearsal-throttle").getByTestId("rule-start")).toBeDisabled();
      await expect(page.getByTestId("lab-stop-all")).toBeDisabled();
      await page.getByTestId("lab-tab-send").click();
      await page.goto(`${server.console}/lab?tab=send&api=sqs&op=ListQueues`);
      await page.getByTestId("lab-send").click();
      await expect(page.getByTestId("lab-answer")).toContainText("Answered");
    },
    ["--lab=false"],
  );
});

test("a rule that ends after its hits moves to the ended list, saying how it ended, and a rule for one caller leaves the others alone", async ({ page, accessKey }) => {
  await own(page, async (server, cli) => {
    const made = cli(["sqs", "create-queue", "--queue-name", "spent"]);
    const url = (JSON.parse(made.out) as { QueueUrl: string }).QueueUrl;
    await page.goto(`${server.console}/lab`);
    await page.getByTestId("rehearsal-internal").getByTestId("rule-start").click();
    await addTo(page, "Services", "sqs");
    await addTo(page, "Operations", "SendMessage");
    await addTo(page, "Callers", accessKey);
    await page.getByTestId("rule-max-hits").fill("1");
    await expect.poll(() => sentence(page)).toContain(accessKey);
    await page.getByTestId("rule-make").click();
    await expect(page.getByTestId("rule-row")).toHaveCount(1);
    // Another caller is not touched; this caller is bent once, and the second call goes through.
    expect(cli(["sqs", "send-message", "--queue-url", url, "--message-body", "a"]).code).toBe(0);
    expect(aws(server.endpoint, ["sqs", "send-message", "--queue-url", url, "--message-body", "b"], { accessKey }).code).not.toBe(0);
    expect(aws(server.endpoint, ["sqs", "send-message", "--queue-url", url, "--message-body", "c"], { accessKey }).code).toBe(0);
    await page.getByRole("button", { name: /Refresh/ }).first().click();
    await expect(page.getByTestId("rule-row")).toHaveCount(0);
    await expect(page.getByTestId("ended-how")).toHaveText("Reached its number of hits");
    await page.getByTestId("ended-row").getByTestId("rule-open").click();
    await expect(page.getByTestId("rule-details")).toContainText("1 of 1");
  });
});

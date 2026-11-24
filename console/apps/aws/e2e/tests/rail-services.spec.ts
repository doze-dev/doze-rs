// The rail: the services under Resources (each with whether it is awake), the one whose pages are open marked, and the counts on the
// right of Live and Resources, drawn as the Kafka console draws its own.
import { expect, open, test } from "./fixtures";

test("the services are under Resources when it is open, the open one is marked, and a service is one click away", async ({ page }) => {
  await open(page, "/resources/sqs/queues", "Queues");
  const services = page.getByTestId("rail-services");
  await expect(services).toBeVisible();
  // Every service of the build but IAM and STS (they are under Access): in their groups, in the catalogue's order.
  expect(await services.getByTestId("rail-service").count()).toBeGreaterThanOrEqual(14);
  await expect(services.locator('[data-service="iam"]')).toHaveCount(0);
  await expect(services.locator('[data-service="sqs"]')).toHaveAttribute("aria-current", "page");
  await expect(services.locator('[data-service="sns"]')).not.toHaveAttribute("aria-current", "page");
  // Running or resting, in words for a screen reader and a state for the mark.
  await expect(services.locator('[data-service="sqs"]')).toHaveAttribute("data-state", /^(awake|asleep)$/);
  await expect(services.locator('[data-service="sqs"]')).toContainText(/Running|Resting/);
  await services.locator('[data-service="sns"]').click();
  await expect(page).toHaveURL(/\/resources\/sns/);
  await expect(page.getByTestId("rail-services").locator('[data-service="sns"]')).toHaveAttribute("aria-current", "page");
});

test("the services are folded away off Resources, and the rail's counts are the apps that called and the services awake", async ({ page }) => {
  await open(page, "/live", "Live");
  await expect(page.getByTestId("rail-services")).toHaveCount(0);
  // Not the calls: this console's own calls are most of the record, and Live hides them.
  await expect(page.getByTestId("count-live")).toHaveText(/^\d+$/);
  await expect(page.getByTestId("count-live")).toHaveAttribute("title", /apps? (have|has) called/);
  await expect(page.getByTestId("count-resources")).toHaveText(/^\d+$/);
  await expect(page.getByTestId("count-resources")).toHaveAttribute("title", /\d+ of \d+ services running/);
});

test("the search jumps to a service by its name, and says which are services", async ({ page }) => {
  await open(page, "/", "Home");
  await page.getByTestId("palette-trigger").click();
  await page.getByTestId("palette-input").fill("step functions");
  const choice = page.getByRole("option", { name: /Step Functions/ }).first();
  await expect(choice).toBeVisible();
  // Under its own heading, apart from the spaces and the resources.
  await expect(page.getByRole("group", { name: "Services" })).toContainText("Step Functions");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/resources\/stepfunctions/);
});

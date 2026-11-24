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
  // Awake or asleep, in words for a screen reader and a state for the dot.
  await expect(services.locator('[data-service="sqs"]')).toHaveAttribute("data-state", /^(awake|asleep)$/);
  await expect(services.locator('[data-service="sqs"]')).toContainText(/Awake|Asleep/);
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
  await expect(page.getByTestId("count-resources")).toHaveAttribute("title", /\d+ of \d+ services awake/);
});

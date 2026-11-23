import { describe, expect, test } from "bun:test";
import { activityWords, type Touch } from "./activity";

const NOW = Date.UTC(2026, 10, 22, 8, 13, 0);
const t = (action: string, resource: string, who: string, at: number, console = false): Touch => ({ action, resource, who, at, console });

describe("the line over the list of secrets", () => {
  const names = ["larder/prod/stripe-key", "larder/prod/db-credentials"];
  test("says who read which secret and when, and that neither was changed today", () => {
    const touches = [t("GetSecretValue", "larder/prod/stripe-key", "payments (go)", NOW - 120_000), t("GetSecretValue", "larder/prod/db-credentials", "orders (go)", Date.UTC(2026, 10, 22, 8, 11, 0)), t("GetSecretValue", "larder/prod/stripe-key", "old (py)", NOW - 9_000_000)];
    expect(activityWords(touches, names, NOW).text).toBe("payments (go) read larder/prod/stripe-key 2 min ago, and orders (go) read larder/prod/db-credentials at 08:11. Neither secret was changed today.");
  });
  test("a secret changed today is named; the console's own reads are not readers, and an ARN names its secret", () => {
    const touches = [t("PutSecretValue", "arn:aws:secretsmanager:us-east-1:111122223333:secret:larder/prod/db-credentials-AbCdEf", "ci", NOW - 60_000), t("GetSecretValue", "larder/prod/stripe-key", "This console", NOW - 1_000, true), t("GetSecretValue", "larder/prod/stripe-key", "payments (go)", NOW - 30_000)];
    expect(activityWords(touches, names, NOW).text).toBe("payments (go) read larder/prod/stripe-key 30 s ago. larder/prod/db-credentials was changed today.");
  });
  test("a change from yesterday is not today's, and nothing read means no line", () => {
    expect(activityWords([t("GetSecretValue", "larder/prod/stripe-key", "p", NOW - 10_000), t("PutSecretValue", "larder/prod/stripe-key", "p", NOW - 86_400_000)], ["larder/prod/stripe-key"], NOW).text).toBe("p read larder/prod/stripe-key 10 s ago. larder/prod/stripe-key was not changed today.");
    expect(activityWords([], names, NOW).text).toBe("");
  });
});

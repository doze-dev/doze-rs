import { describe, expect, test } from "bun:test";
import { ago, clientName, who } from "./people";

describe("who and when", () => {
  test("a user is shown by name with its key on hover; any other key as itself", () => {
    const user = who({ key: "AKIA1", kind: "user", name: "worker", arn: "arn:aws:iam::1:user/worker" });
    expect(user.label).toBe("worker");
    expect(user.title).toContain("AKIA1");
    expect(who({ key: "test", kind: "root", name: null, arn: "arn:aws:iam::1:root" }).label).toBe("test");
    expect(who({ key: "ASIA1", kind: "session", name: null, arn: null }).title).toContain("session token");
  });

  test("a client is named without its version", () => {
    expect(clientName("AWS CLI 2.15.30")).toBe("AWS CLI");
    expect(clientName("boto3 1.34.69")).toBe("boto3");
    expect(clientName("curl")).toBe("curl");
  });

  test("times read as how long ago", () => {
    expect(ago(1_000, 1_500)).toBe("just now");
    expect(ago(0, 2_000)).toBe("2 s ago");
    expect(ago(0, 4 * 60_000)).toBe("4 min ago");
    expect(ago(0, 3 * 3_600_000)).toBe("3 h ago");
    expect(ago(0, 72 * 3_600_000)).toBe("3 d ago");
    expect(ago(5_000, 1_000)).toBe("just now");
  });
});

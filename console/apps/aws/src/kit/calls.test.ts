import { describe, expect, test } from "bun:test";
import type { KeyPrincipal } from "../api/types/KeyPrincipal";
import { callerWords, namedKeys, tookWords } from "./calls";

const titles = new Map([["events", "EventBridge"], ["sns", "SNS"]]);
const user: KeyPrincipal = { key: "AKIAUSER", kind: "user", name: "admin-cli", arn: "arn:aws:iam::1:user/admin-cli" };
const names = new Map([["AKIAUSER", user]]);

describe("callerWords", () => {
  test("a call a service made names the service and what it made it for", () => {
    const c = { caller: "AKIAX", sdk: null, via: { by: "events", name: "rule payment-settled", request_id: null } };
    expect(callerWords(c, titles, names)).toMatchObject({ label: "EventBridge rule payment-settled", key: null });
    expect(callerWords({ ...c, via: { by: "sns", name: null, request_id: null } }, titles, names).label).toBe("SNS");
    expect(callerWords({ ...c, via: { by: null, name: null, request_id: null } }, titles, names).label).toBe("A service, as the caller's role");
  });
  test("an IAM user is shown by name, any other key by its client with the key beside it", () => {
    expect(callerWords({ caller: "AKIAUSER", sdk: "AWS CLI 2.15.30", via: null }, titles, names)).toMatchObject({ label: "admin-cli", key: null });
    expect(callerWords({ caller: "AKIAOTHER", sdk: "boto3 1.34.69", via: null }, titles, names)).toMatchObject({ label: "boto3", key: "AKIAOTHER" });
    expect(callerWords({ caller: "AKIAOTHER", sdk: null, via: null }, titles, names)).toMatchObject({ label: "AKIAOTHER", key: null });
  });
  test("the console and a call with no key say so", () => {
    expect(callerWords({ caller: "console", sdk: null, via: null }, titles, names).label).toBe("This console");
    expect(callerWords({ caller: null, sdk: null, via: null }, titles, names).label).toBe("–");
  });
});

test("a call between services has no time of its own; a wire call says how long it took", () => {
  expect(tookWords({ duration_us: 2400, request_id: "r-1" })).toBe("2.4 ms");
  expect(tookWords({ duration_us: 800, request_id: "" })).toBe("");
});

test("only the access keys of the wire are asked about", () => {
  const via = { by: "sns", name: null, request_id: null };
  expect(namedKeys([{ caller: "AKIAB", via: null }, { caller: "AKIAA", via: null }, { caller: "AKIAB", via: null }, { caller: "console", via: null }, { caller: "AKIAC", via }])).toBe("AKIAA,AKIAB");
});

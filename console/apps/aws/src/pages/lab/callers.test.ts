import { describe, expect, test } from "bun:test";
import type { CallVia } from "../../api/types/CallVia";
import { callerOptions } from "./callers";

const key = (UserName: string, AccessKeyId: string, Status: "Active" | "Inactive" = "Active") => ({ UserName, AccessKeyId, Status, CreateDate: 1 });
const call = (caller: string | null, sdk: string | null = null, via: CallVia | null = null) => ({ caller, sdk, via });

describe("the callers a rule can name", () => {
  test("IAM's keys come first, by user, each with the user beside it; an inactive key says so", () => {
    const options = callerOptions([key("ops", "AKIAOPS000000000002", "Inactive"), key("app", "AKIAAPP000000000001"), key("app", "AKIAAPP000000000000")], []);
    expect(options).toEqual([
      { value: "AKIAAPP000000000000", label: "AKIAAPP000000000000", hint: "app" },
      { value: "AKIAAPP000000000001", label: "AKIAAPP000000000001", hint: "app" },
      { value: "AKIAOPS000000000002", label: "AKIAOPS000000000002", hint: "ops (inactive)" },
    ]);
  });

  test("a caller Live has seen that IAM does not list is offered once, with its client beside it", () => {
    const options = callerOptions(
      [key("app", "AKIAAPP000000000000")],
      [call("AKIAAPP000000000000", "AWS CLI 2.15.30"), call("test", "boto3 1.34.69"), call("test", null), call("ASIATEMP", null), call("test", "boto3 1.34.69")],
    );
    expect(options.map((o) => [o.value, o.hint])).toEqual([
      ["AKIAAPP000000000000", "app"],
      ["ASIATEMP", "seen on Live"],
      ["test", "boto3"],
    ]);
  });

  test("the console's own key, a call made for a service and a call with no key are not callers a rule can fit", () => {
    const options = callerOptions([], [call("console"), call("AKIAFORROLE", null, { by: "events", name: "rule", request_id: null }), call(null), call("")]);
    expect(options).toEqual([]);
  });
});

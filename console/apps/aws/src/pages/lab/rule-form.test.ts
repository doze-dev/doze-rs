import { describe, expect, test } from "bun:test";
import { END_WORDS, endsInSecs, formFor, isRuleTab, newForm, RULE_TABS, toBody } from "./rule-form";

describe("the rule form", () => {
  test("a new form is every call for ten minutes, and its body is what the server takes", () => {
    const body = toBody(newForm("throttle"));
    expect(body).toEqual({ name: "", matcher: { services: [], operations: [], regions: [], callers: [], resources: [] }, effect: { kind: "fail", error: "throttle" }, share: { kind: "every" }, ends_in_secs: 600 });
  });

  test("each rehearsal is the effect it names", () => {
    expect(toBody(newForm("unavailable")).effect).toEqual({ kind: "fail", error: "unavailable" });
    expect(toBody(newForm("internal")).effect).toEqual({ kind: "fail", error: "internal" });
    expect(toBody(newForm("access-denied")).effect).toEqual({ kind: "fail", error: "access-denied" });
    expect(toBody(newForm("drop")).effect).toEqual({ kind: "drop" });
    expect(toBody({ ...newForm("slow"), delay_ms: "400", jitter_ms: "50" }).effect).toEqual({ kind: "slow", delay_ms: 400, jitter_ms: 50 });
  });

  test("a share, a lifetime in any unit and a number of hits", () => {
    expect(toBody({ ...newForm(), share: "nth", n: "4" }).share).toEqual({ kind: "nth", n: 4 });
    expect(toBody({ ...newForm(), share: "percent", percent: "30" }).share).toEqual({ kind: "percent", percent: 30 });
    expect(endsInSecs({ ...newForm(), ends: "2", unit: "hours" })).toBe(7200);
    expect(endsInSecs({ ...newForm(), ends: "90", unit: "seconds" })).toBe(90);
    expect(toBody({ ...newForm(), max_hits: "5" }).max_hits).toBe(5);
    expect(toBody(newForm())).not.toHaveProperty("max_hits");
  });

  test("what is not a whole number is sent as 0, so the check says so in words, and a name is trimmed", () => {
    expect(toBody({ ...newForm(), ends: "ten" }).ends_in_secs).toBe(0);
    expect(toBody({ ...newForm(), ends: "1.5" }).ends_in_secs).toBe(0);
    expect(toBody({ ...newForm(), share: "nth", n: "" }).share).toEqual({ kind: "nth", n: 0 });
    expect(toBody({ ...newForm(), max_hits: "x" }).max_hits).toBe(0);
    expect(toBody({ ...newForm(), name: "  slow orders " }).name).toBe("slow orders");
  });

  test("a recipe's form names its service, operation and resource", () => {
    const f = formFor({ service: "sqs", op: "SendMessage", resource: "orders" });
    expect(toBody(f).matcher).toEqual({ services: ["sqs"], operations: ["SendMessage"], regions: [], callers: [], resources: ["orders"] });
    expect(toBody(formFor({ service: "eventbridge", op: "PutEvents" })).matcher.resources).toEqual([]);
  });

  test("the six tabs are the guide's, and every way a rule ends has words", () => {
    expect(RULE_TABS).toHaveLength(6);
    expect(isRuleTab("slow")).toBe(true);
    expect(isRuleTab("custom")).toBe(false);
    for (const e of ["expired", "spent", "cleared", "replaced"] as const) expect(END_WORDS[e].length).toBeGreaterThan(5);
  });
});

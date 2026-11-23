import { describe, expect, test } from "bun:test";
import type { ColumnSpec } from "../../../api/types/ColumnSpec";
import type { ResourceRow } from "../../../api/types/ResourceRow";
import { actionWords, awsSentence, whyNot, aliasLabel, aliasWords, buildPolicy, countSentence, encryptsSentence, keyName, keyRows, kindOf, parsePolicy, pendingSentence, policySections, principalWords, refsOf, stateWords, usedBy, whenWords } from "./model";

const col = (title: string): ColumnSpec => ({ title, format: "text", badge: null });
const COLUMNS = ["Description", "State", "Spec", "Usage", "Managed by", "Created", "Deletion date"].map(col);
const row = (id: string, state: string, spec: string, deletion: string | null = null): ResourceRow => ({ name: id, label: id, id, cells: ["d", state, spec, "ENCRYPT_DECRYPT", "Customer", "1", deletion], link: null });

describe("keys", () => {
  test("a state and a spec are said in words", () => {
    expect(stateWords("PendingDeletion")).toBe("Pending deletion");
    expect(stateWords("Enabled")).toBe("Enabled");
    expect(kindOf("RSA_2048")).toBe("Asymmetric");
    expect(kindOf("HMAC_256")).toBe("HMAC");
    expect(kindOf("SYMMETRIC_DEFAULT")).toBe("Symmetric");
  });
  test("a key has its aliases, the first one names it and the rest are counted", () => {
    const keys = keyRows([row("k1", "Enabled", "SYMMETRIC_DEFAULT"), row("k2", "Disabled", "RSA_2048")], COLUMNS, [{ name: "alias/larder-queues", key: "k1" }, { name: "alias/larder-app", key: "k1" }]);
    expect(keys[0]!.aliases).toEqual(["larder-app", "larder-queues"]);
    expect(aliasWords(keys[0]!)).toBe("larder-app (+1 more)");
    expect(aliasWords(keys[1]!)).toBe("–");
    expect(keyName(keys[1]!)).toBe("k2");
    expect(aliasLabel("alias/x/y")).toBe("x/y");
  });
  test("the list says how many keys are in which state, and when the pending ones go", () => {
    const keys = keyRows([row("a", "Enabled", "SYMMETRIC_DEFAULT"), row("b", "Enabled", "SYMMETRIC_DEFAULT"), row("c", "Disabled", "SYMMETRIC_DEFAULT"), row("d", "PendingDeletion", "RSA_2048", "2026-11-15T08:14:00Z")], COLUMNS, [{ name: "alias/old", key: "d" }]);
    expect(countSentence(keys, "customer managed key")).toBe("4 customer managed keys: 2 enabled, 1 disabled and 1 pending deletion.");
    expect(countSentence(keys.slice(0, 1), "customer managed key")).toBe("1 customer managed key: 1 enabled.");
    expect(pendingSentence(keys)).toBe("old will be deleted on Nov 15, 2026 at 08:14 unless you cancel.");
    // A key the schedule page has just scheduled is said to have changed.
    expect(pendingSentence(keys, [keys[3]!.id])).toBe("old changed to Pending deletion and will be deleted on Nov 15, 2026 at 08:14 unless you cancel.");
    expect(whenWords("not a time")).toBe("not a time");
  });
});

describe("what uses a key", () => {
  const uses = [{ service: "sqs", kind: "queues", name: "pick-lists", noun: "SQS queue", key: "alias/larder-app" }, { service: "secretsmanager", kind: "secrets", name: "s", noun: "secret", key: "arn:aws:kms:us-east-1:1:key/k1" }, { service: "ssm", kind: "parameters", name: "/p", noun: "parameter", key: null }] as const;
  test("a use names its key by alias, id or ARN", () => {
    const refs = refsOf({ id: "k1", aliases: ["larder-app"] });
    expect(usedBy([...uses], refs).map((u) => u.name)).toEqual(["pick-lists", "s"]);
    expect(usedBy([...uses], refsOf({ id: "zz", aliases: [] }))).toEqual([]);
  });
  test("the sentence names what a key encrypts", () => {
    expect(encryptsSentence("alias/larder-app", [{ service: "sqs", kind: "queues", name: "pick-lists", noun: "" }])).toBe("alias/larder-app encrypts the queue pick-lists.");
    expect(encryptsSentence("k", [])).toBeNull();
    expect(encryptsSentence("k", ["a", "b", "c", "d"].map((n) => ({ service: "sqs" as const, kind: "queues" as const, name: n, noun: "" })))).toBe("k encrypts the queue a, queue b and queue c and 1 more.");
  });
});

describe("the key policy", () => {
  const o = { account: "123456789012", admins: ["arn:aws:iam::123456789012:role/larder-admin"], canDelete: true, users: ["arn:aws:iam::123456789012:role/lambda-exec", "arn:aws:iam::123456789012:role/report-job"], usage: "ENCRYPT_DECRYPT" };
  test("the account statement is always there; administrators and users add theirs", () => {
    expect(buildPolicy({ ...o, admins: [], users: [] }).Statement).toHaveLength(1);
    const p = buildPolicy(o);
    expect(p.Statement!.map((s) => s.Sid)).toEqual(["Enable IAM User Permissions", "Allow access for Key Administrators", "Allow use of the key", "Allow attachment of persistent resources"]);
    expect(p.Statement![1]!.Principal).toEqual({ AWS: "arn:aws:iam::123456789012:role/larder-admin" });
    expect(p.Statement![2]!.Principal).toEqual({ AWS: o.users });
  });
  test("without the delete permission the administrators statement leaves out its two actions", () => {
    const admin = buildPolicy({ ...o, canDelete: false }).Statement![1]!.Action as string[];
    expect(admin).not.toContain("kms:ScheduleKeyDeletion");
    expect(admin).not.toContain("kms:CancelKeyDeletion");
    expect(admin).toContain("kms:RotateKeyOnDemand");
  });
  test("a signing key gets sign and verify, and no grants for AWS services", () => {
    const p = buildPolicy({ ...o, usage: "SIGN_VERIFY" });
    expect(p.Statement).toHaveLength(3);
    expect(p.Statement![2]!.Action).toContain("kms:Sign");
  });
  test("a policy the console wrote reads back as its sections; any other is not read", () => {
    const text = JSON.stringify(buildPolicy(o));
    expect(policySections(text)).toEqual({ admins: ["arn:aws:iam::123456789012:role/larder-admin"], canDelete: true, users: o.users });
    expect(policySections(JSON.stringify(buildPolicy({ ...o, canDelete: false })))!.canDelete).toBe(false);
    expect(policySections(JSON.stringify({ Statement: [{ Sid: "mine", Effect: "Allow" }] }))).toBeNull();
    expect(policySections(JSON.stringify({ Statement: [{ Sid: "Enable IAM User Permissions" }, { Sid: "custom" }] }))).toBeNull();
    expect(policySections("nope")).toBeNull();
    expect(parsePolicy("[]")).toBeNull();
  });
  test("a principal is named by its role or user", () => {
    expect(principalWords("arn:aws:iam::1:role/path/larder-admin")).toEqual({ name: "larder-admin", kind: "role" });
    expect(principalWords("arn:aws:iam::123:root")).toEqual({ name: "123", kind: "account" });
    expect(principalWords("*")).toEqual({ name: "*", kind: "other" });
  });
});

describe("key actions", () => {
  const keys = keyRows([row("a", "Disabled", "SYMMETRIC_DEFAULT"), row("b", "PendingDeletion", "SYMMETRIC_DEFAULT", "2026-11-15T08:14:00Z")], COLUMNS, [{ name: "alias/larder-legacy", key: "a" }]);
  test("the words under the table say what is selected", () => {
    expect(actionWords(keys.slice(0, 1)).startsWith("larder-legacy is selected and Disabled. Key actions are")).toBe(true);
    expect(actionWords([]).startsWith("Key actions are")).toBe(true);
    expect(actionWords(keys).startsWith("larder-legacy and b are selected and Disabled and Pending deletion.")).toBe(true);
  });
  test("an action the service would refuse is off, and says why", () => {
    expect(whyNot("enable", keys.slice(0, 1))).toBeNull();
    expect(whyNot("cancel", keys.slice(0, 1))).toMatch(/pending deletion/);
    expect(whyNot("cancel", keys.slice(1))).toBeNull();
    expect(whyNot("disable", keys)).toMatch(/cannot be disabled/);
    expect(whyNot("schedule", [])).toBe("Select a key first.");
  });
});

test("the AWS managed keys say they are all enabled when they are", () => {
  const keys = keyRows([row("a", "Enabled", "SYMMETRIC_DEFAULT"), row("b", "Enabled", "SYMMETRIC_DEFAULT")], COLUMNS, []);
  expect(awsSentence(keys)).toBe("2 AWS managed keys, all Enabled. Each alias has the form aws/<service-name>.");
  expect(awsSentence(keys.slice(0, 1))).toBe("1 AWS managed key, all Enabled. Each alias has the form aws/<service-name>.");
  expect(awsSentence([{ ...keys[0]!, state: "Disabled" }])).toBe("1 AWS managed key: 1 disabled. Each alias has the form aws/<service-name>.");
});

import { countsSentence, keySentence } from "./model";
describe("what a key says of itself", () => {
  test("its aliases and tags are counted in words", () => {
    expect(countsSentence(2, 3)).toBe("Two aliases and three tags.");
    expect(countsSentence(1, 0)).toBe("One alias and no tags.");
    expect(countsSentence(0, null)).toBe("No aliases.");
    expect(countsSentence(12, 1)).toBe("12 aliases and one tag.");
  });
  test("the sentence says the state and, for a symmetric key, its rotation", () => {
    const key = { state: "Enabled", stateWords: "Enabled", deletion: null, kind: "Symmetric" as const };
    expect(keySentence("alias/larder-app", key, { enabled: false, days: null, last: { type: "ON_DEMAND", at: "2026-11-07T09:10:00Z" }, count: 1 })).toBe("alias/larder-app is Enabled. Its key material was rotated on demand on Nov 7, 2026, and automatic rotation is off.");
    expect(keySentence("k", key, { enabled: true, days: 180, last: null, count: 0 })).toBe("k is Enabled. Its key material has not been rotated, and automatic rotation is on, every 180 days.");
    expect(keySentence("k", { ...key, kind: "Asymmetric" }, null)).toBe("k is Enabled.");
    expect(keySentence("k", { ...key, state: "PendingDeletion", stateWords: "Pending deletion", deletion: "2026-11-15T08:14:00Z" }, null)).toBe("k is Pending deletion and will be deleted on Nov 15, 2026 at 08:14 unless you cancel.");
  });
});

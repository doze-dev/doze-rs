import { describe, expect, test } from "bun:test";
import { groupSentence, groupsSentence, inheritsSentence, policiesSentence, policySentence, roleSentence, rolesSentence, userSentence, usersSentence } from "./sentences";

describe("lists", () => {
  test("the users say who holds a key, who used it today and who has none", () => {
    const users = [{ name: "dev", active: 1, usedToday: true }, { name: "ci-bot", active: 1, usedToday: true }, { name: "temp-auditor", active: 0, usedToday: false }];
    expect(usersSentence(users)).toBe("3 users: dev and ci-bot each hold 1 active access key and both used it today; temp-auditor has no access keys.");
    expect(usersSentence([{ name: "dev", active: 2, usedToday: false }])).toBe("1 user: dev holds 2 active access keys and none used a key today.");
    expect(usersSentence([{ name: "a", active: 1, usedToday: true }, { name: "b", active: 1, usedToday: false }])).toContain("a used it today");
    // A user with two keys: the sentence names a key, not "it".
    expect(usersSentence([{ name: "dev", active: 2, usedToday: true }, { name: "ci-bot", active: 1, usedToday: true }])).toBe("2 users: dev holds 2 active access keys, ci-bot holds 1 active access key and both used a key today.");
    expect(usersSentence([])).toBe("There are no users in this account.");
  });
  test("the groups say their members and policies, the busy ones first", () => {
    expect(groupsSentence([{ name: "contractors", members: [], policies: [] }, { name: "developers", members: ["dev"], policies: ["PowerUserAccess"] }])).toBe("2 groups: developers has 1 member, dev, and one attached policy, PowerUserAccess; contractors has no members and no attached policy.");
    expect(groupsSentence([{ name: "developers", members: ["dev"], policies: ["PowerUserAccess"] }])).toBe("1 group: developers has 1 member, dev, and one attached policy, PowerUserAccess.");
  });
  test("the roles say which were assumed, with whose key, and which are for a service", () => {
    const roles = [{ name: "lambda-exec", trusted: ["AWS service: lambda"], assumed: 0, inLastHour: 0, assumedBy: [] }, { name: "report-job", trusted: ["AWS account: 1"], assumed: 7, inLastHour: 7, assumedBy: ["ci-bot"] }];
    expect(rolesSentence(roles)).toBe("2 roles. report-job was assumed 7 times in the last hour, each time with ci-bot's key; lambda-exec is for Lambda functions.");
    expect(rolesSentence([{ name: "x", trusted: [], assumed: 2, inLastHour: 1, assumedBy: [] }])).toContain("in the Live record");
  });
  test("the policies say what each customer managed one is attached to", () => {
    const s = policiesSentence([{ name: "report-job-readonly", attached: [{ kind: "role", name: "report-job" }] }, { name: "larder-queues-rw", attached: [{ kind: "user", name: "ci-bot" }] }], "Local");
    expect(s).toBe("2 customer managed policies are attached: report-job-readonly to the role report-job, larder-queues-rw to the user ci-bot. AWS managed policies are not counted here.");
    expect(policiesSentence([{ name: "p", attached: [] }], "Local")).toContain("1 customer managed policy not attached to anything: p");
    expect(policiesSentence([], "AWS")).toContain("No AWS managed policy is attached to anything yet");
  });
});

describe("a resource", () => {
  test("a user says its groups, what they give and its key", () => {
    expect(userSentence({ name: "dev", groups: ["developers"], viaGroups: ["PowerUserAccess"], direct: [], key: { id: "AKIA...7DEV2K", usedAt: Date.UTC(2026, 10, 8, 8, 9) / 1000, service: "sqs", region: "us-east-1" } })).toBe("dev belongs to 1 group and gets PowerUserAccess through it. Its key AKIA...7DEV2K was last used at 08:09 with sqs in us-east-1.");
    expect(userSentence({ name: "tester", groups: [], viaGroups: [], direct: [], key: null })).toBe("tester belongs to no group and has no policy, so it is allowed to do nothing.");
    expect(userSentence({ name: "u", groups: [], viaGroups: [], direct: ["p"], key: { id: "K", usedAt: null, service: null, region: null } })).toBe("u belongs to no group and has p attached directly. Its key K has never been used.");
    expect(userSentence({ name: "dev", groups: [], viaGroups: [], direct: ["p"], key: { id: "AKIA...7DEV2K", usedAt: Date.UTC(2026, 10, 8, 8, 9) / 1000, service: "sqs", region: "us-east-1" }, others: [{ id: "AKIA...2ND", usedAt: null, service: null, region: null }] })).toBe("dev belongs to no group and has p attached directly. Of its keys, AKIA...7DEV2K was last used at 08:09 with sqs in us-east-1 and AKIA...2ND has never been used.");
  });
  test("a group says what its members can do", () => {
    expect(groupSentence("developers", ["dev"], ["PowerUserAccess"])).toBe("developers has 1 member and 1 attached policy. Everyone in it can do what PowerUserAccess allows: every service except IAM, Organizations and Account.");
    expect(groupSentence("contractors", [], [])).toBe("contractors has no members and no attached policy, so its members get nothing from it.");
  });
  test("a role says its sessions and its one permission", () => {
    expect(roleSentence({ name: "report-job", assumed: 7, inLastHour: 7, by: ["ci-bot"], policies: ["report-job-readonly"], only: { action: "s3:GetObject", resource: "reports/*" } })).toBe("report-job was assumed 7 times in the last hour, all with ci-bot's key. Its only permission is s3:GetObject on reports/*, through report-job-readonly.");
    expect(roleSentence({ name: "r", assumed: 0, inLastHour: 0, by: [], policies: [], only: null })).toBe("r has not been assumed in the Live record. It has no permissions policy, so a session of it is allowed to do nothing.");
  });
  test("a policy says what it is attached to and whether it was edited", () => {
    expect(policySentence("report-job-readonly", { users: [], roles: ["report-job"], groups: [] }, "v1", false, false)).toBe("report-job-readonly is attached to 1 role, report-job. Its default version is v1; the policy has not been edited since it was created.");
    expect(policySentence("p", { users: [], roles: [], groups: [] }, "v2", true, false)).toBe("p is not attached to anything. Its default version is v2; the policy has been edited since it was created.");
  });
});

describe("keys", () => {
  test("the Security credentials tab says how many keys and which are active", async () => {
    const { keysSentence } = await import("./sentences");
    expect(keysSentence("dev", [{ Status: "Active" }])).toBe("dev has 1 access key, and it is active.");
    expect(keysSentence("dev", [{ Status: "Inactive" }])).toBe("dev has 1 access key, and it is inactive.");
    expect(keysSentence("tester", [])).toBe("tester has no access keys.");
    expect(keysSentence("dev", [{ Status: "Active" }, { Status: "Inactive" }])).toBe("dev has 2 access keys, and 1 is active and 1 inactive.");
  });
});

test("a user says what the one policy it inherits allows", () => {
  expect(inheritsSentence("dev-intern", ["PowerUserAccess"], ["developers"])).toBe("dev-intern inherits PowerUserAccess through developers: every service except IAM, Organizations and Account.");
  expect(inheritsSentence("dev", ["PowerUserAccess", "PowerUserAccess"], ["a", "b"])).toBe("dev inherits PowerUserAccess through a and b: every service except IAM, Organizations and Account.");
  expect(inheritsSentence("dev", ["Custom"], ["developers"])).toBe("dev inherits Custom through developers.");
  expect(inheritsSentence("dev", ["PowerUserAccess", "ReadOnlyAccess"], ["developers"])).toBe("dev inherits PowerUserAccess and ReadOnlyAccess through developers.");
});

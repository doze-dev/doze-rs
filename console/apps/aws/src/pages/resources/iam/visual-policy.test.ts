import { describe, expect, test } from "bun:test";
import { catalogue, prefixOf, readsOnly } from "./actions";
import { actionsOf, blockProblem, blocksOf, documentOf, newBlock, type Block } from "./visual-policy";

const SQS = "arn:aws:sqs:us-east-1:000000000000:pick-lists";
const doc = (statements: unknown[]) => JSON.stringify({ Version: "2012-10-17", Statement: statements });
const blocks = (text: string): Block[] => {
  const r = blocksOf(text);
  if (!("blocks" in r)) throw new Error(r.why);
  return r.blocks;
};

describe("a policy as blocks", () => {
  test("the board's policy is one block, and writes back as itself", () => {
    const text = doc([{ Effect: "Allow", Action: ["sqs:SendMessage"], Resource: SQS }]);
    const b = blocks(text);
    expect(b).toEqual([{ sid: null, effect: "Allow", service: "sqs", all: false, actions: ["SendMessage"], anyResource: false, resources: [SQS] }]);
    expect(JSON.parse(documentOf(b))).toEqual(JSON.parse(text));
  });
  test("a statement over two services is a block each, and says the same", () => {
    const b = blocks(doc([{ Effect: "Allow", Action: ["sqs:SendMessage", "s3:GetObject", "sqs:*", "s3:ListBucket"], Resource: "*" }]));
    expect(b.map((x) => [x.service, x.all, x.actions])).toEqual([["sqs", true, ["SendMessage"]], ["s3", false, ["GetObject", "ListBucket"]]]);
    expect(b.every((x) => x.anyResource)).toBe(true);
    expect(actionsOf(b[0]!)).toEqual(["sqs:*"]);
    expect(JSON.parse(documentOf(b)).Statement).toEqual([
      { Effect: "Allow", Action: ["sqs:*"], Resource: "*" },
      { Effect: "Allow", Action: ["s3:GetObject", "s3:ListBucket"], Resource: "*" },
    ]);
  });
  test("every action is one block, and a Deny and a Sid are kept", () => {
    const b = blocks(doc([{ Sid: "nothing", Effect: "Deny", Action: "*", Resource: ["arn:aws:s3:::a", "arn:aws:s3:::b"] }]));
    expect(b).toEqual([{ sid: "nothing", effect: "Deny", service: "*", all: true, actions: [], anyResource: false, resources: ["arn:aws:s3:::a", "arn:aws:s3:::b"] }]);
    expect(JSON.parse(documentOf(b)).Statement[0]).toEqual({ Sid: "nothing", Effect: "Deny", Action: ["*"], Resource: ["arn:aws:s3:::a", "arn:aws:s3:::b"] });
  });
  test("what the blocks cannot say is said, not dropped", () => {
    const why = (s: object) => (blocksOf(doc([{ Effect: "Allow", Action: "s3:*", Resource: "*", ...s }])) as { why: string }).why;
    expect(why({ Condition: { Bool: { "aws:SecureTransport": "true" } } })).toContain("Condition");
    expect(why({ NotAction: "s3:*" })).toContain("NotAction");
    expect(why({ Principal: "*" })).toContain("Principal");
    expect(why({ Effect: "Maybe" })).toContain("Effect");
    expect(why({ Action: ["nonsense"] })).toContain("not service:Action");
    expect((blocksOf("{") as { why: string }).why).toContain("not valid");
  });
  test("a block that is not finished leaves its element out, and says what is missing", () => {
    expect(JSON.parse(documentOf([newBlock()])).Statement[0]).toEqual({ Effect: "Allow" });
    const half = { ...newBlock("sqs"), actions: ["SendMessage"] };
    expect(JSON.parse(documentOf([half])).Statement[0]).toEqual({ Effect: "Allow", Action: ["sqs:SendMessage"] });
    expect(blockProblem(newBlock())).toBe("Choose a service.");
    expect(blockProblem(newBlock("sqs"))).toBe("Choose at least one action, or all of the service's.");
    expect(blockProblem(half)).toBe("Add the ARN of a resource, or choose all resources.");
    expect(blockProblem({ ...half, resources: ["pick-lists"] })).toContain("is not an ARN");
    expect(blockProblem({ ...half, resources: [SQS] })).toBeNull();
    expect(blockProblem({ ...half, anyResource: true })).toBeNull();
    expect(blockProblem({ ...newBlock("sqs"), all: true, anyResource: true })).toBeNull();
  });
});

describe("the actions offered", () => {
  const t = (target: string, operations: string[], served = true) => ({ target, title: target.toUpperCase(), served, operations });
  test("are the built operations by IAM's name, a service once, with a few renamed", () => {
    const c = catalogue([t("sqs", ["SendMessage", "SendMessageBatch", "ReceiveMessage"]), t("s3", ["GetObject", "HeadObject", "ListObjectsV2", "PutObject"]), t("monitoring", ["PutMetricData"]), t("apigateway", ["GetRestApi"]), t("apigatewayv2", ["GetApi"]), t("sns", [], true), t("kms", ["Encrypt"], false)]);
    expect(c.map((s) => s.prefix)).toEqual(["apigateway", "cloudwatch", "s3", "sqs"]);
    expect(c.find((s) => s.prefix === "sqs")?.actions).toEqual(["ReceiveMessage", "SendMessage"]);
    expect(c.find((s) => s.prefix === "s3")?.actions).toEqual(["GetObject", "ListBucket", "PutObject"]);
    expect(c.find((s) => s.prefix === "apigateway")?.actions).toEqual(["GetApi", "GetRestApi"]);
    expect(prefixOf("monitoring")).toBe("cloudwatch");
  });
  test("read only is what gets, lists and describes", () => {
    expect(["GetObject", "ListBucket", "DescribeTable", "Query", "PutObject", "DeleteQueue", "SendMessage"].filter(readsOnly)).toEqual(["GetObject", "ListBucket", "DescribeTable", "Query"]);
  });
});

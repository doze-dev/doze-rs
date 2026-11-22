import { describe, expect, test } from "bun:test";
import { colourLine } from "./template-colour";

const kinds = (line: string, json = false) => colourLine(line, json).filter((t) => t.kind !== "plain").map((t) => `${t.kind}:${t.text}`);

describe("a template line, coloured", () => {
  test("YAML: a key, a string, a number, a word", () => {
    expect(kinds("  QueueName: \"pick-lists\"")).toEqual(["key:QueueName", 'string:"pick-lists"']);
    expect(kinds("    MaximumMessageSize: 262144")).toEqual(["key:MaximumMessageSize", "number:262144"]);
    expect(kinds("    Enabled: true")).toEqual(["key:Enabled", "word:true"]);
    expect(kinds("  - Key: env")).toEqual(["key:Key"]);
  });
  test("YAML: the intrinsic functions and comments", () => {
    expect(kinds("      Value: !Ref SearchTable")).toEqual(["key:Value", "tag:!Ref"]);
    expect(kinds("      Fn::Sub: arn:${AWS::Region}")).toEqual(["key:Fn::Sub"]);
    expect(kinds("# the queue")).toEqual(["comment:# the queue"]);
    expect(kinds("  Type: AWS::SQS::Queue # a queue")).toEqual(["key:Type", "comment:# a queue"]);
    expect(kinds("  Url: https://x.example/#top")).toEqual(["key:Url"]);
  });
  test("JSON: a string before a colon is a key, the rest are values", () => {
    expect(kinds('    "BillingMode": "PAY_PER_REQUEST",', true)).toEqual(["key:\"BillingMode\"", 'string:"PAY_PER_REQUEST"']);
    expect(kinds('      "Ref": "Table"', true)).toEqual(['key:"Ref"', 'string:"Table"']);
    expect(kinds('  "Retries": 3, "On": false', true)).toEqual(['key:"Retries"', "number:3", 'key:"On"', "word:false"]);
  });
  test("the parts put back together are the line", () => {
    const line = '  Properties: { Name: "a b", N: 3 } # c';
    expect(colourLine(line, false).map((t) => t.text).join("")).toBe(line);
  });
});

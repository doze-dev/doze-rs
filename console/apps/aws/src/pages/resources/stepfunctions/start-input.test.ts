import { describe, expect, test } from "bun:test";
import type { AslState } from "./graph";
import { inputSkeleton } from "./start-input";

const def = (state: AslState) => ({ StartAt: "A", States: { A: state } });

describe("what a new execution starts with", () => {
  test("is a skeleton of the paths the first state reads, none of them empty", () => {
    const text = inputSkeleton(def({ Type: "Task", Parameters: { "Id.$": "$.order_id", "Who.$": "$.customer.email", "At.$": "$$.Execution.StartTime", Fixed: 1 }, ResultPath: "$.done" }));
    expect(JSON.parse(text)).toEqual({ order_id: "order_id", customer: { email: "email" } });
  });

  test("the machine whose first state reads a DynamoDB key does not start with an empty string, which DynamoDB refuses", () => {
    const text = inputSkeleton(def({ Type: "Task", Resource: "arn:aws:states:::dynamodb:getItem", Parameters: { TableName: "orders-table", Key: { order_id: { "S.$": "$.order_id" }, created_at: { "S.$": "$.created_at" }, qty: { "N.$": "$.qty" } } } }));
    const input = JSON.parse(text) as Record<string, string>;
    expect(input).toEqual({ created_at: "created_at", order_id: "order_id", qty: "1" });
    for (const v of Object.values(input)) expect(v).not.toBe("");
  });

  test("is {} when the first state reads nothing, or the definition is not there", () => {
    expect(inputSkeleton(def({ Type: "Pass", ResultPath: "$.out" }))).toBe("{}");
    expect(inputSkeleton(null)).toBe("{}");
  });

  test("reads a Choice's Variable with the value it is compared to, and an InputPath", () => {
    expect(JSON.parse(inputSkeleton(def({ Type: "Choice", Choices: [{ Variable: "$.level", StringEquals: "gold", Next: "A" }, { Variable: "$.qty", NumericGreaterThan: 5, Next: "A" }, { Variable: "$.paid", BooleanEquals: false, Next: "A" }] })))).toEqual({ level: "gold", qty: 6, paid: false });
    expect(JSON.parse(inputSkeleton(def({ Type: "Task", InputPath: "$.order" })))).toEqual({ order: "order" });
  });
});

describe("a Choice that compares two members", () => {
  test("gives both a number, so the comparison runs", () => {
    const text = inputSkeleton(def({ Type: "Choice", Choices: [{ Variable: "$.on_hand", NumericLessThanPath: "$.reorder_point", Next: "A" }] }));
    expect(JSON.parse(text)).toEqual({ on_hand: 1, reorder_point: 1 });
  });
});

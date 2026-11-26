import { describe, expect, test } from "bun:test";
import { inTemplateOrder, resourceOrder } from "./template-order";

const yaml = `AWSTemplateFormatVersion: "2010-09-09"
Parameters:
  Stage:
    Type: String
Resources:
  PickListsQueue:
    Type: AWS::SQS::Queue
    Properties:
      QueueName: pick-lists
  OrdersCreatedTopic:
    Type: AWS::SNS::Topic
  "OrdersTable":
    Type: AWS::DynamoDB::Table
Outputs:
  Zed:
    Value: x
`;

describe("the order a template writes its resources in", () => {
  test("is read from YAML, under Resources only", () => {
    expect(resourceOrder(yaml)).toEqual(["PickListsQueue", "OrdersCreatedTopic", "OrdersTable"]);
  });

  test("is read from JSON", () => {
    expect(resourceOrder(JSON.stringify({ Resources: { B: { Type: "x" }, A: { Type: "y" } } }))).toEqual(["B", "A"]);
  });

  test("puts what a stack owns in that order, and a resource the template does not name last", () => {
    const rows = ["OrdersCreatedTopic", "OrdersTable", "PickListsQueue", "Zzz"].map((LogicalResourceId) => ({ LogicalResourceId }));
    expect(inTemplateOrder(rows, resourceOrder(yaml)).map((r) => r.LogicalResourceId)).toEqual(["PickListsQueue", "OrdersCreatedTopic", "OrdersTable", "Zzz"]);
    expect(inTemplateOrder(rows, []).map((r) => r.LogicalResourceId)).toEqual(["OrdersCreatedTopic", "OrdersTable", "PickListsQueue", "Zzz"]);
  });
});

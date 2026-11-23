import { describe, expect, test } from "bun:test";
import { emptyMethod, methodCalls, methodProblem, settingsSummary } from "./method-model";

const w = { api: "k3x", resource: "r8w", region: "us-east-1", account: "111122223333" };

describe("a method's calls", () => {
  test("a Lambda proxy is PutMethod and PutIntegration (the function is always called with POST), as the board's calls show", () => {
    const calls = methodCalls({ ...emptyMethod, method: "POST", fn: "resize-product-image" }, w);
    expect(calls.map((c) => c.operation)).toEqual(["PutMethod", "PutIntegration"]);
    expect(calls[0]!.input).toEqual({ restApiId: "k3x", resourceId: "r8w", httpMethod: "POST", authorizationType: "NONE" });
    expect(calls[1]!.input).toEqual({ restApiId: "k3x", resourceId: "r8w", requestHttpMethod: "POST", type: "AWS_PROXY", httpMethod: "POST", uri: "arn:aws:apigateway:us-east-1:lambda:path/2015-03-31/functions/arn:aws:lambda:us-east-1:111122223333:function:resize-product-image/invocations" });
  });
  test("a non-proxy integration gets the 200 responses the console adds", () => {
    expect(methodCalls({ ...emptyMethod, kind: "mock" }, w).map((c) => c.operation)).toEqual(["PutMethod", "PutIntegration", "PutMethodResponse", "PutIntegrationResponse"]);
    expect(methodCalls({ ...emptyMethod, kind: "lambda", proxy: false, fn: "f" }, w)[1]!.input).toMatchObject({ type: "AWS" });
  });
  test("HTTP uses the method's own verb unless told otherwise; an AWS service names its action", () => {
    expect(methodCalls({ ...emptyMethod, kind: "http", url: "https://catalog.larder.example.com/v1/products" }, w)[1]!.input).toMatchObject({ type: "HTTP_PROXY", httpMethod: "GET" });
    expect(methodCalls({ ...emptyMethod, kind: "aws", service: "sqs", action: "SendMessage", role: "gw" }, w)[1]!.input).toMatchObject({ type: "AWS", uri: "arn:aws:apigateway:us-east-1:sqs:action/SendMessage", credentials: "arn:aws:iam::111122223333:role/gw" });
  });
  test("what is wrong, first", () => {
    expect(methodProblem(emptyMethod, ["GET"])).toBe("The resource already has a GET method.");
    expect(methodProblem({ ...emptyMethod, method: "POST" }, ["GET"])).toBe("Choose the Lambda function.");
    expect(methodProblem({ ...emptyMethod, kind: "http", url: "x" }, [])).toContain("Enter the endpoint URL");
    expect(methodProblem({ ...emptyMethod, kind: "mock" }, [])).toBeNull();
    expect(settingsSummary({ ...emptyMethod, authorization: "AWS_IAM", apiKey: true })).toBe("IAM, API key required.");
  });
});

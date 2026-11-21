import { describe, expect, test } from "bun:test";
import { emptyHttp, emptyRest, httpAuthorizerInput, httpAuthorizerProblem, identitySource, restAuthorizerInput, restAuthorizerProblem } from "./authorizer-model";

const o = { api: "k3x", region: "us-east-1", account: "111122223333" };

describe("a REST authorizer", () => {
  test("a token authorizer names its header, its pattern and its cache, as the board's call shows", () => {
    const f = { ...emptyRest, name: "partner-token-auth", fn: "token-auth", validation: "^Bearer [A-Za-z0-9-_.]+$" };
    expect(restAuthorizerInput(f, o)).toEqual({
      restApiId: "k3x", name: "partner-token-auth", type: "TOKEN",
      authorizerUri: "arn:aws:apigateway:us-east-1:lambda:path/2015-03-31/functions/arn:aws:lambda:us-east-1:111122223333:function:token-auth/invocations",
      identitySource: "method.request.header.Authorization", identityValidationExpression: "^Bearer [A-Za-z0-9-_.]+$", authorizerResultTtlInSeconds: 300,
    });
  });
  test("choosing Request makes it REQUEST; Cognito names the pools", () => {
    expect(restAuthorizerInput({ ...emptyRest, name: "a", fn: "f", payload: "REQUEST", source: "Authorization, stageVariables.env" }, o)).toMatchObject({ type: "REQUEST", identitySource: "method.request.header.Authorization,stageVariables.env" });
    expect(restAuthorizerInput({ ...emptyRest, name: "a", kind: "cognito", pools: "arn:aws:cognito-idp:us-east-1:1:userpool/p" }, o)).toMatchObject({ type: "COGNITO_USER_POOLS", providerARNs: ["arn:aws:cognito-idp:us-east-1:1:userpool/p"] });
  });
  test("what is missing is said first", () => {
    expect(restAuthorizerProblem(emptyRest)).toBe("Enter an authorizer name.");
    expect(restAuthorizerProblem({ ...emptyRest, name: "a" })).toBe("Choose the Lambda function.");
    expect(restAuthorizerProblem({ ...emptyRest, name: "a", fn: "f", validation: "(" })).toBe("The token validation is not a regular expression.");
    expect(restAuthorizerProblem({ ...emptyRest, name: "a", fn: "f", ttl: "4000" })).toBe("The cache TTL is 0 to 3,600 seconds.");
    expect(identitySource("method.request.header.X", "TOKEN")).toBe("method.request.header.X");
  });
});

describe("an HTTP authorizer", () => {
  test("a Lambda one with simple responses, as the board's call shows", () => {
    expect(httpAuthorizerInput({ ...emptyHttp, name: "orders-request-auth", fn: "orders-auth" }, o)).toEqual({
      ApiId: "k3x", name: "orders-request-auth", authorizerType: "REQUEST",
      authorizerUri: "arn:aws:apigateway:us-east-1:lambda:path/2015-03-31/functions/arn:aws:lambda:us-east-1:111122223333:function:orders-auth/invocations",
      authorizerPayloadFormatVersion: "2.0", enableSimpleResponses: true, identitySource: ["$request.header.Authorization"], authorizerResultTtlInSeconds: 300,
    });
  });
  test("a JWT one names its issuer and audiences", () => {
    const f = { ...emptyHttp, name: "jwt", kind: "jwt" as const, issuer: "https://idp.example.com/", audience: "a, b" };
    expect(httpAuthorizerInput(f, o)).toMatchObject({ authorizerType: "JWT", jwtConfiguration: { issuer: "https://idp.example.com/", audience: ["a", "b"] } });
    expect(httpAuthorizerProblem(f)).toBeNull();
    expect(httpAuthorizerProblem({ ...f, issuer: "idp" })).toBe("The issuer is a URL, such as https://idp.example.com/.");
    expect(httpAuthorizerProblem({ ...emptyHttp, name: "a", fn: "f", sources: [""], ttl: "300" })).toBe("Caching needs at least one identity source.");
  });
});

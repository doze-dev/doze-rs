// A method's form and the calls it makes: PutMethod, PutIntegration, and for a non-proxy integration a 200 method response and a 200
// integration response (what the console adds to every method it creates, so the method can answer).
import { restLambdaUri } from "./words";

export type IntegrationKind = "lambda" | "http" | "mock" | "aws";
export type MethodForm = {
  method: string;
  kind: IntegrationKind;
  proxy: boolean;
  fn: string | null;
  url: string;
  httpMethod: string;
  service: string;
  action: string;
  role: string | null;
  authorization: "NONE" | "AWS_IAM" | "CUSTOM";
  authorizer: string | null;
  apiKey: boolean;
  operation: string;
};
export const METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "ANY"] as const;
/** The services an AWS service integration can call here, by the prefix in the integration's URI. */
export const SERVICES = ["sqs", "sns", "dynamodb", "kinesis", "states", "events"] as const;
export const emptyMethod: MethodForm = { method: "GET", kind: "lambda", proxy: true, fn: null, url: "", httpMethod: "", service: "sqs", action: "SendMessage", role: null, authorization: "NONE", authorizer: null, apiKey: false, operation: "" };

export function methodProblem(f: MethodForm, taken: readonly string[]): string | null {
  if (taken.includes(f.method)) return `The resource already has a ${f.method} method.`;
  if (f.kind === "lambda" && !f.fn) return "Choose the Lambda function.";
  if (f.kind === "http" && !/^https?:\/\/\S+$/.test(f.url.trim())) return "Enter the endpoint URL, starting with https:// or http://.";
  if (f.kind === "aws" && f.action.trim() === "") return "Enter the action to call.";
  if (f.authorization === "CUSTOM" && !f.authorizer) return "Choose the authorizer.";
  return null;
}

export type Where = { api: string; resource: string; region: string; account: string };
export type Call = { operation: string; input: Record<string, unknown> };

/** The method's own request: what a caller must carry. */
export function methodInput(f: MethodForm, w: Where): Record<string, unknown> {
  return {
    restApiId: w.api,
    resourceId: w.resource,
    httpMethod: f.method,
    authorizationType: f.authorization,
    ...(f.authorization === "CUSTOM" && f.authorizer ? { authorizerId: f.authorizer } : {}),
    ...(f.apiKey ? { apiKeyRequired: true } : {}),
    ...(f.operation.trim() ? { operationName: f.operation.trim() } : {}),
  };
}

/** What the integration is, by kind: a Lambda function is always called with POST, whatever the method is. */
export function integrationInput(f: MethodForm, w: Where): Record<string, unknown> {
  const at = { restApiId: w.api, resourceId: w.resource, requestHttpMethod: f.method };
  switch (f.kind) {
    case "lambda":
      return { ...at, type: f.proxy ? "AWS_PROXY" : "AWS", httpMethod: "POST", uri: restLambdaUri(w.region, f.fn ? `arn:aws:lambda:${w.region}:${w.account}:function:${f.fn}` : "<function>") };
    case "http":
      return { ...at, type: f.proxy ? "HTTP_PROXY" : "HTTP", httpMethod: f.httpMethod || (f.method === "ANY" ? "POST" : f.method), uri: f.url.trim() || "<endpoint URL>" };
    case "mock":
      return { ...at, type: "MOCK", requestTemplates: { "application/json": '{"statusCode": 200}' } };
    case "aws":
      return { ...at, type: "AWS", httpMethod: "POST", uri: `arn:aws:apigateway:${w.region}:${f.service}:action/${f.action.trim() || "<action>"}`, ...(f.role ? { credentials: `arn:aws:iam::${w.account}:role/${f.role}` } : {}) };
  }
}

/** Every call the form makes, in order. */
export function methodCalls(f: MethodForm, w: Where): Call[] {
  const calls: Call[] = [{ operation: "PutMethod", input: methodInput(f, w) }, { operation: "PutIntegration", input: integrationInput(f, w) }];
  const proxy = (f.kind === "lambda" || f.kind === "http") && f.proxy;
  if (!proxy) {
    const at = { restApiId: w.api, resourceId: w.resource, httpMethod: f.method, statusCode: "200" };
    calls.push({ operation: "PutMethodResponse", input: at }, { operation: "PutIntegrationResponse", input: at });
  }
  return calls;
}

export const settingsSummary = (f: MethodForm): string => `${f.authorization === "NONE" ? "None" : f.authorization === "AWS_IAM" ? "IAM" : "Lambda authorizer"}, API key ${f.apiKey ? "required" : "not required"}.`;

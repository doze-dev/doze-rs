import { describe, expect, test } from "bun:test";
import type { ServiceRow } from "../../api/types/ServiceRow";
import { endpointVar, serviceUrls } from "./service-urls";

const svc = (name: string, title: string, sdk: string[], served = true): ServiceRow => ({ name, title, group: "g", served, built: 1, total: 1, apis: sdk.map((sdk_id) => ({ sdk_id, signing_name: name, built: 1, total: 1 })) });

describe("endpointVar", () => {
  test("is the SDK's: the service id in capitals, one underscore for each run of other characters", () => {
    expect(endpointVar("SQS")).toBe("AWS_ENDPOINT_URL_SQS");
    expect(endpointVar("API Gateway")).toBe("AWS_ENDPOINT_URL_API_GATEWAY");
    expect(endpointVar("CloudWatch Logs")).toBe("AWS_ENDPOINT_URL_CLOUDWATCH_LOGS");
    expect(endpointVar("Secrets Manager")).toBe("AWS_ENDPOINT_URL_SECRETS_MANAGER");
    expect(endpointVar("ApiGatewayV2")).toBe("AWS_ENDPOINT_URL_APIGATEWAYV2");
  });
});

describe("serviceUrls", () => {
  const rows = serviceUrls([svc("sqs", "SQS", ["SQS"]), svc("lambda", "Lambda", ["Lambda"]), svc("dynamodb", "DynamoDB", ["DynamoDB", "DynamoDB Streams"]), svc("kms", "KMS", ["KMS"], false)], "http://127.0.0.1:4566", "123456789012", "us-east-1");
  test("a service this run does not serve has no row", () => {
    expect(rows.map((r) => r.name)).toEqual(["sqs", "lambda", "dynamodb"]);
  });
  test("every service is at the one address", () => {
    expect(new Set(rows.map((r) => r.address))).toEqual(new Set(["127.0.0.1:4566"]));
  });
  test("a service with two APIs has a variable for each", () => {
    expect(rows[2]?.vars).toEqual(["AWS_ENDPOINT_URL_DYNAMODB", "AWS_ENDPOINT_URL_DYNAMODB_STREAMS"]);
  });
  test("the detail is what is true of its URLs here", () => {
    expect(rows[0]?.detail).toBe("Queue URLs: /123456789012/<queue>");
    expect(rows[1]?.detail).toBe("Function URLs: /_aws/lambda-url/us-east-1/<id>/");
    expect(rows[1]?.more?.drawer).toBe("invoke");
    expect(rows[2]?.detail).toBeNull();
  });
});

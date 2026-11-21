// The world the API Gateway boards are drawn in ("larder"), made with the real AWS CLI against a doze-aws: the Lambda functions the
// APIs call, a REST API (larder-public: a catalog and a receipts resource, an authorizer, the stage prod with its variables and
// access log, a usage plan with three keys and one without), a second REST API never deployed (larder-internal), an HTTP API
// (larder-admin: three routes, two integrations, a Lambda authorizer, an automatic `$default` stage and a manual one) and an HTTP API
// with nothing (larder-hooks). `bun e2e/tests/apigateway-seed.ts http://127.0.0.1:45950 [account]` seeds a running one by hand.
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { aws } from "../server";

type Cli = (args: string[]) => string;

function runner(endpoint: string): Cli {
  return (args) => {
    const r = aws(endpoint, args);
    if (r.code !== 0) throw new Error(`aws ${args.join(" ")}\n${r.out}`);
    return r.out;
  };
}

const PLAIN = `def lambda_handler(event, context):\n    return {"statusCode": 200, "body": "ok"}\n`;
const NODE = `export const handler = async (event) => {\n  return { statusCode: 200, body: "ok" };\n};\n`;

function zipOf(file: string, source: string): string {
  const dir = mkdtempSync(join(tmpdir(), "apigateway-seed-"));
  writeFileSync(join(dir, file), source);
  const zip = join(dir, "fn.zip");
  const made = Bun.spawnSync(["zip", "-q", "-j", zip, join(dir, file)]);
  if (made.exitCode !== 0) throw new Error("zip failed");
  return zip;
}

export const lambdaArn = (account: string, name: string) => `arn:aws:lambda:us-east-1:${account}:function:${name}`;

/** What the seed made, by the ids a spec needs. */
export type Larder = {
  rest: string;
  restRoot: string;
  products: string;
  receipts: string;
  internal: string;
  http: string;
  hooks: string;
  plan: string;
  trial: string;
  tokenAuth: string;
  requestAuth: string;
  deployment: string;
  getRoute: string;
  ordersRoute: string;
  lambdaIntegration: string;
  httpIntegration: string;
};

const json = <T>(out: string) => JSON.parse(out) as T;

export function seedLarder(endpoint: string, account: string): Larder {
  const cli = runner(endpoint);
  const trust = JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Principal: { Service: "lambda.amazonaws.com" }, Action: "sts:AssumeRole" }] });
  cli(["iam", "create-role", "--role-name", "lambda-exec", "--assume-role-policy-document", trust]);
  const role = `arn:aws:iam::${account}:role/lambda-exec`;
  const make = (name: string, runtime: string, handler: string, file: string, source: string, extra: string[] = []) =>
    cli(["lambda", "create-function", "--function-name", name, "--runtime", runtime, "--role", role, "--handler", handler, "--zip-file", `fileb://${zipOf(file, source)}`, ...extra]);
  make("export-report", "python3.13", "lambda_function.lambda_handler", "lambda_function.py", PLAIN, ["--description", "Nightly sales export to the reports bucket"]);
  make("resize-product-image", "nodejs22.x", "index.handler", "index.mjs", NODE, ["--description", "Resize product photos into thumbnails", "--memory-size", "512"]);
  make("token-auth", "python3.13", "lambda_function.lambda_handler", "lambda_function.py", PLAIN, ["--description", "Checks a partner token"]);
  make("admin-auth", "python3.13", "lambda_function.lambda_handler", "lambda_function.py", PLAIN, ["--description", "Checks an admin session"]);
  cli(["logs", "create-log-group", "--log-group-name", "/larder/api"]);

  // REST: larder-public.
  const rest = json<{ id: string; rootResourceId: string }>(cli(["apigateway", "create-rest-api", "--name", "larder-public", "--description", "Public storefront: products and receipts", "--endpoint-configuration", "types=REGIONAL"]));
  const resource = (part: string) => json<{ id: string }>(cli(["apigateway", "create-resource", "--rest-api-id", rest.id, "--parent-id", rest.rootResourceId, "--path-part", part])).id;
  const products = resource("products");
  const receipts = resource("receipts");
  const a = ["--rest-api-id", rest.id];
  cli(["apigateway", "put-method", ...a, "--resource-id", products, "--http-method", "GET", "--authorization-type", "NONE", "--operation-name", "ListProducts", "--request-parameters", "method.request.querystring.category=false,method.request.querystring.page=false"]);
  cli(["apigateway", "put-integration", ...a, "--resource-id", products, "--http-method", "GET", "--type", "HTTP", "--integration-http-method", "GET", "--uri", "https://catalog.larder.example.com/v1/products", "--request-parameters", "integration.request.querystring.category=method.request.querystring.category,integration.request.querystring.page=method.request.querystring.page"]);
  cli(["apigateway", "put-method-response", ...a, "--resource-id", products, "--http-method", "GET", "--status-code", "200", "--response-models", "application/json=Empty"]);
  cli(["apigateway", "put-integration-response", ...a, "--resource-id", products, "--http-method", "GET", "--status-code", "200"]);
  cli(["apigateway", "put-method", ...a, "--resource-id", products, "--http-method", "OPTIONS", "--authorization-type", "NONE"]);
  cli(["apigateway", "put-integration", ...a, "--resource-id", products, "--http-method", "OPTIONS", "--type", "MOCK", "--request-templates", '{"application/json":"{\\"statusCode\\": 200}"}']);
  cli(["apigateway", "put-method-response", ...a, "--resource-id", products, "--http-method", "OPTIONS", "--status-code", "200"]);
  cli(["apigateway", "put-integration-response", ...a, "--resource-id", products, "--http-method", "OPTIONS", "--status-code", "200"]);
  cli(["apigateway", "put-method", ...a, "--resource-id", receipts, "--http-method", "POST", "--authorization-type", "NONE"]);
  cli(["apigateway", "put-integration", ...a, "--resource-id", receipts, "--http-method", "POST", "--type", "AWS_PROXY", "--integration-http-method", "POST", "--uri", `arn:aws:apigateway:us-east-1:lambda:path/2015-03-31/functions/${lambdaArn(account, "export-report")}/invocations`]);
  const tokenAuth = json<{ id: string }>(cli(["apigateway", "create-authorizer", ...a, "--name", "larder-token-auth", "--type", "TOKEN", "--authorizer-uri", `arn:aws:apigateway:us-east-1:lambda:path/2015-03-31/functions/${lambdaArn(account, "token-auth")}/invocations`, "--identity-source", "method.request.header.Authorization", "--authorizer-result-ttl-in-seconds", "300"])).id;
  const deployment = json<{ id: string }>(cli(["apigateway", "create-deployment", ...a, "--stage-name", "prod", "--description", "First deploy", "--variables", "catalogHost=catalog.larder.example.com,thumbSize=256"])).id;
  cli(["apigateway", "update-account", "--patch-operations", `op=replace,path=/cloudwatchRoleArn,value=arn:aws:iam::${account}:role/lambda-exec`]);
  cli(["apigateway", "update-stage", ...a, "--stage-name", "prod", "--patch-operations", "op=replace,path=/description,value=Production", `op=replace,path=/accessLogSettings/destinationArn,value=arn:aws:logs:us-east-1:${account}:log-group:/larder/api`, "op=replace,path=/accessLogSettings/format,value=$context.requestId $context.httpMethod $context.path $context.status", "op=replace,path=/*/*/logging/loglevel,value=ERROR"]);

  // A key for each storefront, the plans they are in.
  const plan = json<{ id: string }>(cli(["apigateway", "create-usage-plan", "--name", "partners-basic", "--description", "Partner storefronts, basic tier", "--throttle", "rateLimit=50,burstLimit=100", "--quota", "limit=10000,period=MONTH", "--api-stages", `apiId=${rest.id},stage=prod`])).id;
  for (const n of ["storefront-berlin", "storefront-lyon", "storefront-oslo"]) {
    const key = json<{ id: string }>(cli(["apigateway", "create-api-key", "--name", n, "--enabled"])).id;
    cli(["apigateway", "create-usage-plan-key", "--usage-plan-id", plan, "--key-id", key, "--key-type", "API_KEY"]);
  }
  const trial = json<{ id: string }>(cli(["apigateway", "create-usage-plan", "--name", "partners-trial", "--description", "Trial storefronts, 100 a day", "--throttle", "rateLimit=5,burstLimit=10", "--quota", "limit=100,period=DAY"])).id;

  const internal = json<{ id: string }>(cli(["apigateway", "create-rest-api", "--name", "larder-internal", "--description", "Back-office tools, not deployed yet"])).id;

  // HTTP: larder-admin.
  const v2 = (...args: string[]) => cli(["apigatewayv2", ...args]);
  const http = json<{ ApiId: string }>(v2("create-api", "--name", "larder-admin", "--protocol-type", "HTTP", "--description", "Admin console backend: reports and orders")).ApiId;
  const lambdaIntegration = json<{ IntegrationId: string }>(v2("create-integration", "--api-id", http, "--integration-type", "AWS_PROXY", "--integration-uri", lambdaArn(account, "export-report"), "--payload-format-version", "2.0")).IntegrationId;
  const httpIntegration = json<{ IntegrationId: string }>(v2("create-integration", "--api-id", http, "--integration-type", "HTTP_PROXY", "--integration-method", "ANY", "--integration-uri", "https://orders.larder.example.com/{proxy}", "--payload-format-version", "1.0")).IntegrationId;
  const requestAuth = json<{ AuthorizerId: string }>(v2("create-authorizer", "--api-id", http, "--name", "admin-request-auth", "--authorizer-type", "REQUEST", "--authorizer-uri", `arn:aws:apigateway:us-east-1:lambda:path/2015-03-31/functions/${lambdaArn(account, "admin-auth")}/invocations`, "--authorizer-payload-format-version", "2.0", "--enable-simple-responses", "--identity-source", "$request.header.Authorization", "--authorizer-result-ttl-in-seconds", "300")).AuthorizerId;
  const getRoute = json<{ RouteId: string }>(v2("create-route", "--api-id", http, "--route-key", "GET /reports", "--target", `integrations/${lambdaIntegration}`, "--authorization-type", "CUSTOM", "--authorizer-id", requestAuth)).RouteId;
  v2("create-route", "--api-id", http, "--route-key", "POST /reports/run", "--target", `integrations/${lambdaIntegration}`, "--authorization-type", "CUSTOM", "--authorizer-id", requestAuth);
  const ordersRoute = json<{ RouteId: string }>(v2("create-route", "--api-id", http, "--route-key", "ANY /orders/{proxy+}")).RouteId;
  v2("create-stage", "--api-id", http, "--stage-name", "$default", "--auto-deploy", "--description", "Production");
  v2("create-stage", "--api-id", http, "--stage-name", "beta", "--description", "Next release");
  const hooks = json<{ ApiId: string }>(v2("create-api", "--name", "larder-hooks", "--protocol-type", "HTTP", "--description", "Inbound webhooks")).ApiId;

  return { rest: rest.id, restRoot: rest.rootResourceId, products, receipts, internal: internal, http, hooks, plan, trial, tokenAuth, requestAuth, deployment, getRoute, ordersRoute, lambdaIntegration, httpIntegration };
}

if (import.meta.main) {
  const [endpoint = "http://127.0.0.1:45950", account = "111122223333"] = process.argv.slice(2);
  console.log(JSON.stringify(seedLarder(endpoint, account), null, 2));
}

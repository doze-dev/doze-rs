// Go parity: which operations the Go edition serves (tier F or C in its SUPPORT.md) does this binary refuse (501)?
//   doze-aws --listen 127.0.0.1:4590 --data-dir <tmp> &   then
//   GO_SUPPORT=/path/to/doze-aws/docs/SUPPORT.md bun tools/aws-parity/parity.ts 127.0.0.1:4590
// Compares operation NAMES only; behaviour inside an operation is the conformance scoreboard's business. JSON and Query services are
// probed by target or action; REST services (S3, Lambda, API Gateway v1 and v2) by the operation's own HTTP method and path, with
// every label filled in. A refusal by name is a 501 carrying `X-Doze-Not-Built` or the `NotImplemented` code.
import { readFileSync } from "fs";
const support = readFileSync(process.env.GO_SUPPORT ?? "../doze-aws/docs/SUPPORT.md", "utf8");
// Go: section -> op -> tier
const sections: Record<string, Record<string, string>> = {};
let cur = "";
for (const line of support.split("\n")) {
  const h = line.match(/^## (.+?) — API support/);
  if (h) { cur = h[1]; sections[cur] = {}; continue; }
  if (!cur) continue;
  // The first cell may name several operations ("A / B"), wrap them in prose or parentheses ("PartiQL (ExecuteStatement, ...)"):
  // every capitalised word in it is a candidate, and only the model's own operation names are kept below.
  const r = line.match(/^\| ([^|]+?) \| (F|C|S|—) \|/);
  if (r && r[1] !== "Operation") for (const op of r[1].match(/[A-Z][A-Za-z0-9]+/g) ?? []) sections[cur][op] = r[2];
}
const svcs: any[] = [
  { go: "SQS", model: "sqs", sign: "sqs", kind: "json", ver: "1.0", target: "AmazonSQS" },
  { go: "SSM", model: "ssm", sign: "ssm", kind: "json", ver: "1.1", target: "AmazonSSM" },
  { go: "Secrets Manager", model: "secrets-manager", sign: "secretsmanager", kind: "json", ver: "1.1", target: "secretsmanager" },
  { go: "KMS", model: "kms", sign: "kms", kind: "json", ver: "1.1", target: "TrentService" },
  { go: "DynamoDB", model: "dynamodb", sign: "dynamodb", kind: "json", ver: "1.0", target: "DynamoDB_20120810" },
  { go: "IAM", model: "iam", sign: "iam", kind: "query", version: "2010-05-08" },
  { go: "STS", model: "sts", sign: "sts", kind: "query", version: "2011-06-15" },
  { go: "CloudWatch Logs", model: "cloudwatch-logs", sign: "logs", kind: "json", ver: "1.1", target: "Logs_20140328" },
  { go: "Kinesis", model: "kinesis", sign: "kinesis", kind: "json", ver: "1.1", target: "Kinesis_20131202" },
  { go: "CloudWatch", model: "cloudwatch", sign: "monitoring", kind: "json", ver: "1.0", target: "GraniteServiceVersion20100801" },
  { go: "EventBridge", model: "eventbridge", sign: "events", kind: "json", ver: "1.1", target: "AWSEvents" },
  { go: "SNS", model: "sns", sign: "sns", kind: "query", version: "2010-03-31" },
  { go: "Step Functions", model: "sfn", sign: "states", kind: "json", ver: "1.0", target: "AWSStepFunctions" },
  { go: "CloudFormation", model: "cloudformation", sign: "cloudformation", kind: "query", version: "2010-05-15" },
  { go: "S3", model: "s3", sign: "s3", kind: "rest" },
  { go: "Lambda", model: "lambda", sign: "lambda", kind: "rest" },
  { go: "API Gateway", model: "api-gateway", sign: "apigateway", kind: "rest" },
  { go: "API Gateway v2 (HTTP APIs)", model: "apigatewayv2", sign: "apigateway", kind: "rest" },
];
const addr = process.argv[2];
function refused(r: Response, text: string): boolean {
  return r.status === 501 && (r.headers.has("x-doze-not-built") || text.includes("NotImplemented") || text.includes("not built"));
}
async function probe(s: any, op: string, shape: any): Promise<boolean> {
  const headers: Record<string, string> = {
    authorization: `AWS4-HMAC-SHA256 Credential=test/20261112/us-east-1/${s.sign}/aws4_request, SignedHeaders=host, Signature=00`,
    "x-amz-date": "20261112T000000Z",
  };
  let url = `http://${addr}/`, method = "POST", body: string | undefined;
  if (s.kind === "json") { headers["content-type"] = `application/x-amz-json-${s.ver}`; headers["x-amz-target"] = `${s.target}.${op}`; body = "{}"; }
  else if (s.kind === "query") { headers["content-type"] = "application/x-www-form-urlencoded"; body = `Action=${op}&Version=${s.version}`; }
  else {
    const http = shape.traits?.["smithy.api#http"];
    if (!http) return true;
    method = http.method;
    // Labels get a placeholder; a greedy label one segment. S3's bucket label becomes a path-style bucket.
    const path = String(http.uri).replace(/\{[^}]+\}/g, "doze-parity");
    url = `http://${addr}${path}`;
    if (method !== "GET" && method !== "HEAD" && method !== "DELETE") { body = "{}"; headers["content-type"] = "application/json"; }
  }
  const r = await fetch(url, { method, headers, body });
  const text = method === "HEAD" ? "" : await r.text();
  return !refused(r, text);
}
const rows: any[] = [];
for (const s of svcs) {
  const model = JSON.parse(readFileSync(`models/${s.model}.json`, "utf8"));
  const opShapes = Object.entries<any>(model.shapes).filter(([, v]) => v.type === "operation");
  const ops = opShapes.map(([k]) => k.split("#")[1]);
  const go = sections[s.go] ?? {};
  const built = new Set<string>();
  for (const [k, v] of opShapes) if (await probe(s, k.split("#")[1], v)) built.add(k.split("#")[1]);
  const goServed = Object.entries(go).filter(([, t]) => t === "F" || t === "C").map(([o]) => o).filter(o => ops.includes(o));
  const missing = goServed.filter(o => !built.has(o));
  const extra = [...built].filter(o => !goServed.includes(o));
  rows.push({ svc: s.go, modelOps: ops.length, goServed: goServed.length, rustBuilt: built.size, goServedMissingInRust: missing, rustBeyondGo: extra.length });
}
for (const r of rows) console.log(`${r.svc.slice(0, 16).padEnd(16)} model ${String(r.modelOps).padStart(3)} | Go F/C ${String(r.goServed).padStart(3)} | Rust built ${String(r.rustBuilt).padStart(3)} | Go-served but Rust refuses: ${r.goServedMissingInRust.length} ${r.goServedMissingInRust.length ? "[" + r.goServedMissingInRust.join(", ") + "]" : ""} | Rust beyond Go: ${r.rustBeyondGo}`);

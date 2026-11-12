// Go parity: which operations the Go edition serves (tier F or C in its SUPPORT.md) does this binary refuse (501)?
//   doze-aws --listen 127.0.0.1:4590 --data-dir <tmp> &   then
//   GO_SUPPORT=/path/to/doze-aws/docs/SUPPORT.md bun tools/aws-parity/parity.ts 127.0.0.1:4590
// Compares operation NAMES only; behaviour inside an operation is the conformance scoreboard's business. Query and JSON services are
// probed; REST services (S3) are not (read their stage note). Add a service to the table below when it is built.
import { readFileSync } from "fs";
const support = readFileSync(process.env.GO_SUPPORT ?? "../doze-aws/docs/SUPPORT.md", "utf8");
// Go: section -> op -> tier
const sections: Record<string, Record<string, string>> = {};
let cur = "";
for (const line of support.split("\n")) {
  const h = line.match(/^## (.+?) — API support/);
  if (h) { cur = h[1]; sections[cur] = {}; continue; }
  if (!cur) continue;
  const r = line.match(/^\| ([A-Za-z0-9 /]+?) \| (F|C|S|—) \|/);
  if (r && r[1] !== "Operation") for (const op of r[1].split("/").map(s => s.trim()).filter(Boolean)) sections[cur][op] = r[2];
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
];
const addr = process.argv[2];
async function probe(s: any, op: string): Promise<boolean> {
  const headers: Record<string, string> = {
    authorization: `AWS4-HMAC-SHA256 Credential=test/20261112/us-east-1/${s.sign}/aws4_request, SignedHeaders=host, Signature=00`,
    "x-amz-date": "20261112T000000Z",
  };
  let body: string;
  if (s.kind === "json") { headers["content-type"] = `application/x-amz-json-${s.ver}`; headers["x-amz-target"] = `${s.target}.${op}`; body = "{}"; }
  else { headers["content-type"] = "application/x-www-form-urlencoded"; body = `Action=${op}&Version=${s.version}`; }
  const r = await fetch(`http://${addr}/`, { method: "POST", headers, body });
  await r.text();
  return r.status !== 501;
}
const rows: any[] = [];
for (const s of svcs) {
  const model = JSON.parse(readFileSync(`models/${s.model}.json`, "utf8"));
  const ops = Object.entries<any>(model.shapes).filter(([, v]) => v.type === "operation").map(([k]) => k.split("#")[1]);
  const go = sections[s.go] ?? {};
  const built = new Set<string>();
  for (const op of ops) if (await probe(s, op)) built.add(op);
  const goServed = Object.entries(go).filter(([, t]) => t === "F" || t === "C").map(([o]) => o).filter(o => ops.includes(o));
  const missing = goServed.filter(o => !built.has(o));
  const extra = [...built].filter(o => !goServed.includes(o));
  rows.push({ svc: s.go, modelOps: ops.length, goServed: goServed.length, rustBuilt: built.size, goServedMissingInRust: missing, rustBeyondGo: extra.length });
}
for (const r of rows) console.log(`${r.svc.padEnd(16)} model ${String(r.modelOps).padStart(3)} | Go F/C ${String(r.goServed).padStart(3)} | Rust built ${String(r.rustBuilt).padStart(3)} | Go-served but Rust refuses: ${r.goServedMissingInRust.length} ${r.goServedMissingInRust.length ? "[" + r.goServedMissingInRust.join(", ") + "]" : ""} | Rust beyond Go: ${r.rustBeyondGo}`);

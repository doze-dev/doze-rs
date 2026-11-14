// Fetch the reference first: bun permission-audit.ts --fetch (writes svcref/, git-ignored). Then: bun permission-audit.ts
// Every boto3 call the conformance scenarios make -> the IAM actions AWS authorizes for it (Service Reference "Operations") ->
// each action's resource types -> a sample doze-rec-* ARN of each type, or "*" when the action has no resource. Then: does an
// Allow statement of the recorder policy cover it? Prints what is not covered.
import { readFileSync, readdirSync } from "fs";
const ref = process.env.SVCREF_DIR ?? `${import.meta.dir}/svcref/`;
const root = `${import.meta.dir}/../../`;
if (process.argv.includes("--fetch")) {
  // AWS's Service Reference: one JSON per service, the actions, their resource types and which actions each API operation needs.
  const { mkdirSync, writeFileSync } = await import("fs");
  mkdirSync(ref, { recursive: true });
  const index = await (await fetch("https://servicereference.us-east-1.amazonaws.com/")).json();
  for (const svc of ["sqs", "sns", "ssm", "secretsmanager", "kms", "dynamodb", "s3", "logs", "kinesis", "cloudwatch", "events", "scheduler",
    "lambda", "states", "apigateway", "execute-api", "cloudformation", "iam", "sts"]) {
    const e = index.find((x: any) => x.service === svc);
    if (e) writeFileSync(ref + svc + ".json", await (await fetch(e.url)).text());
  }
  console.log("fetched into " + ref);
  process.exit(0);
}
const policy = JSON.parse(readFileSync(root + "tools/aws-rec/recorder-permission-set.json", "utf8"));
const glob = (p: string) => new RegExp("^" + p.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*").replace(/\?/g, ".") + "$", "i");
const allows = (action: string, arn: string) =>
  policy.Statement.some((s: any) => s.Effect === "Allow" && [].concat(s.Action).some((a: string) => glob(a).test(action)) &&
    [].concat(s.Resource).some((r: string) => glob(r).test(arn)));
const refs: Record<string, any> = {};
for (const f of readdirSync(ref).filter((f) => f.endsWith(".json") && f !== "index.json")) { const j = JSON.parse(readFileSync(ref + f, "utf8")); refs[j.Name] = j; }
const tests = readdirSync(root + "conformance/tests").filter((f) => f.endsWith(".py")).map((f) => readFileSync(root + "conformance/tests/" + f, "utf8")).join("\n");
const called = new Set([...tests.matchAll(/\.([a-z][a-z0-9_]+)\(/g)].map((m) => m[1]));
const sample = (fmt: string) => fmt.replace("${Partition}", "aws").replace("${Region}", "us-east-1").replace("${Account}", "123456789012").replace(/\$\{[A-Za-z]+\}/g, "doze-rec-x");
const out = new Set<string>();
const testsByService: Record<string, string> = { sqs: "sqs", sns: "sns", ssm: "ssm", secretsmanager: "secretsmanager", kms: "kms", dynamodb: "dynamodb", s3: "s3", logs: "logs", kinesis: "kinesis", cloudwatch: "monitoring", events: "events", scheduler: "scheduler", lambda: "lambda", states: "states", apigateway: "apigateway", "execute-api": "execute-api", cloudformation: "cloudformation", iam: "iam", sts: "sts" };
for (const [svc, j] of Object.entries(refs)) {
  for (const op of j.Operations ?? []) {
    const methods = (op.SDK ?? []).filter((x: any) => x.Package === "Boto3").map((x: any) => x.Method);
    if (!methods.some((m: string) => called.has(m))) continue;
    for (const a of op.AuthorizedActions ?? []) {
      const rj = refs[a.Service]; if (!rj) continue;
      const act = rj.Actions.find((x: any) => x.Name === a.Name); if (!act) continue;
      const action = `${a.Service}:${a.Name}`;
      const res = act.Resources ?? [];
      if (res.length === 0) { if (!allows(action, "*")) out.add(`${action}  on *   (via ${svc}.${methods[0]})`); continue; }
      const covered = res.some((r: any) => {
        const def = rj.Resources.find((x: any) => x.Name === r.Name);
        return (def?.ARNFormats ?? []).some((f: string) => allows(action, sample(f)));
      });
      if (!covered) out.add(`${action}  on ${res.map((r: any) => r.Name).join("|")}   (via ${svc}.${methods[0]})`);
    }
  }
}
console.log([...out].sort().join("\n") || "none");

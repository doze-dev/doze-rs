// The world the IAM boards are drawn in ("larder"), made with the real AWS CLI against a doze-aws: three users (dev with a key and the
// group developers, ci-bot with a key and a policy of its own, temp-auditor with neither), a user (tester) and a group (contractors) with
// nothing, roles report-job / lambda-exec / larder-admin / dispatch-role, two customer managed policies, an alias, and the calls the
// boards read as "used": dev's key reads a queue, ci-bot's key assumes report-job. `bun e2e/tests/iam-seed.ts http://127.0.0.1:45190
// [account]` seeds a running one by hand.
import { aws } from "../server";

type Cli = (args: string[], key?: string) => string;

function runner(endpoint: string): Cli {
  return (args, key) => {
    const r = aws(endpoint, args, { accessKey: key });
    if (r.code !== 0) throw new Error(`aws ${args.join(" ")}\n${r.out}`);
    return r.out;
  };
}

export const trust = (principal: Record<string, unknown>) => JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Principal: principal, Action: "sts:AssumeRole" }] });
const doc = (action: string | string[], resource: string | string[]) => JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Action: action, Resource: resource }] });

/** The keys the world made, by user. */
export type World = { dev: string; ciBot: string };

/** Makes the larder world; the account is the one the doze-aws serves. */
export function seedLarder(endpoint: string, account: string): World {
  const cli = runner(endpoint);
  const key = (user: string) => (JSON.parse(cli(["iam", "create-access-key", "--user-name", user])) as { AccessKey: { AccessKeyId: string } }).AccessKey.AccessKeyId;
  cli(["sqs", "create-queue", "--queue-name", "pick-lists"]);
  cli(["s3api", "create-bucket", "--bucket", "reports"]);

  for (const u of ["dev", "ci-bot", "temp-auditor", "tester"]) cli(["iam", "create-user", "--user-name", u]);
  cli(["iam", "create-group", "--group-name", "developers"]);
  cli(["iam", "create-group", "--group-name", "contractors"]);
  cli(["iam", "add-user-to-group", "--group-name", "developers", "--user-name", "dev"]);
  cli(["iam", "attach-group-policy", "--group-name", "developers", "--policy-arn", "arn:aws:iam::aws:policy/PowerUserAccess"]);

  const readonly = (JSON.parse(cli(["iam", "create-policy", "--policy-name", "report-job-readonly", "--description", "Read objects under reports/ in the reports bucket", "--policy-document", doc("s3:GetObject", "arn:aws:s3:::reports/*")])) as { Policy: { Arn: string } }).Policy.Arn;
  const queues = (JSON.parse(cli(["iam", "create-policy", "--policy-name", "larder-queues-rw", "--description", "Send, receive and delete on the larder queues", "--policy-document", doc(["sqs:SendMessage", "sqs:ReceiveMessage", "sqs:DeleteMessage"], `arn:aws:sqs:us-east-1:${account}:*`)])) as { Policy: { Arn: string } }).Policy.Arn;
  cli(["iam", "attach-user-policy", "--user-name", "ci-bot", "--policy-arn", queues]);

  cli(["iam", "create-role", "--role-name", "report-job", "--description", "Read-only exports for the nightly report job.", "--assume-role-policy-document", trust({ AWS: [`arn:aws:iam::${account}:user/ci-bot`, `arn:aws:iam::${account}:user/dev`] })]);
  cli(["iam", "attach-role-policy", "--role-name", "report-job", "--policy-arn", readonly]);
  cli(["iam", "create-role", "--role-name", "lambda-exec", "--assume-role-policy-document", trust({ Service: "lambda.amazonaws.com" })]);
  cli(["iam", "create-role", "--role-name", "larder-admin", "--assume-role-policy-document", trust({ AWS: `arn:aws:iam::${account}:root` })]);
  cli(["iam", "attach-role-policy", "--role-name", "larder-admin", "--policy-arn", "arn:aws:iam::aws:policy/AdministratorAccess"]);
  cli(["iam", "create-role", "--role-name", "dispatch-role", "--assume-role-policy-document", trust({ Service: "states.amazonaws.com" })]);
  cli(["iam", "create-account-alias", "--account-alias", "larder-dev"]);

  const dev = key("dev");
  const ciBot = key("ci-bot");
  // Used: dev's key reads a queue; ci-bot's key assumes report-job (twice).
  cli(["sqs", "get-queue-attributes", "--queue-url", `${endpoint}/${account}/pick-lists`, "--attribute-names", "All"], dev);
  for (let i = 0; i < 2; i++) cli(["sts", "assume-role", "--role-arn", `arn:aws:iam::${account}:role/report-job`, "--role-session-name", "report-job-py"], ciBot);
  return { dev, ciBot };
}

if (import.meta.main) {
  const [endpoint = "http://127.0.0.1:45190", account = "111122223333"] = process.argv.slice(2);
  const w = seedLarder(endpoint, account);
  console.log(`seeded ${endpoint}: dev's key ${w.dev}, ci-bot's key ${w.ciBot}`);
}

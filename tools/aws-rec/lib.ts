// Shared helpers for the recorder tools. Everything goes through the AWS CLI v2 with the SSO profile; no keys are ever read or stored.
export const BIN = process.env.AWS_BIN ?? "/opt/homebrew/bin/aws";
export const PROFILE = process.env.DOZE_REC_PROFILE ?? "doze-rec-main";
export const REGION = "us-east-1";
export const EXPECT_ACCOUNT = "821712915674";
export const EXPECT_ROLE = "doze-recorder-limited";
export const PREFIX = "doze-rec-";

/** Runs one CLI call and returns parsed JSON (null for an empty reply). Throws with the CLI's last stderr line. */
export async function aws(args: string[]): Promise<any> {
  const p = Bun.spawn([BIN, ...args, "--profile", PROFILE, "--region", REGION, "--output", "json", "--no-cli-pager"], {
    stdout: "pipe",
    stderr: "pipe",
    env: { ...process.env, AWS_PAGER: "" },
  });
  const [out, err] = await Promise.all([new Response(p.stdout).text(), new Response(p.stderr).text()]);
  const code = await p.exited;
  if (code !== 0) {
    const last = err.trim().split("\n").filter(Boolean).pop() ?? `exit ${code}`;
    throw new Error(`aws ${args.slice(0, 2).join(" ")}: ${last}`);
  }
  return out.trim() ? JSON.parse(out) : null;
}

/** Refuses to run unless we are the limited recorder role in the expected account. */
export async function assertRecorder(): Promise<string> {
  const who = await aws(["sts", "get-caller-identity"]);
  if (who.Account !== EXPECT_ACCOUNT) throw new Error(`wrong account ${who.Account}, expected ${EXPECT_ACCOUNT}`);
  if (!String(who.Arn).includes(EXPECT_ROLE)) throw new Error(`wrong role ${who.Arn}, expected ${EXPECT_ROLE}`);
  return who.Arn as string;
}

/** True if the name is one of ours. Leading slashes and the Lambda log-group path are ignored. */
export const ours = (name: string): boolean => name.replace(/^\/aws\/lambda\//, "").replace(/^\//, "").startsWith(PREFIX);

// Create key, step 4: under the table of key users, one line for each user or role chosen that says what its own IAM policies let it
// do (kms__key-create-users: "report-job reads reports/* in S3 through its report-job-readonly policy"). Pure: the step reads the
// policies, this words them.
import { type AttachedPolicy, type Doc, isAwsManaged, list, permissions } from "../iam/model";

export type Holds = { name: string; attached: readonly AttachedPolicy[]; inline: readonly string[]; docs: ReadonlyMap<string, Doc> };

const VERBS: Record<string, string> = { "s3:GetObject": "reads", "s3:PutObject": "writes" };

/** A resource the way a person says it: a bucket path for S3, the name part of any other ARN. */
export function resourceWords(resource: string): string {
  if (resource.startsWith("arn:aws:s3:::")) return resource.slice("arn:aws:s3:::".length);
  const parts = resource.split(":");
  return parts[0] === "arn" && parts.length >= 6 ? parts.slice(5).join(":") : resource;
}

/** What a chosen principal's own policies say, in one sentence. `null` while the policies are still being read. */
export function principalLine(h: Holds): string {
  const names = [...h.attached.map((p) => p.PolicyName), ...h.inline];
  if (names.length === 0) return `${h.name} has no IAM policy of its own, so the key policy is all that lets it use this key.`;
  const one = h.attached.length === 1 && h.inline.length === 0 ? h.attached[0]! : null;
  const doc = one && !isAwsManaged(one.PolicyArn) ? h.docs.get(one.PolicyArn) : undefined;
  const grants = doc === undefined ? [] : permissions(doc);
  if (one && grants.length === 1 && !grants[0]!.actions.includes(",")) {
    const { actions, resource } = grants[0]!;
    const where = resourceWords(resource);
    const verb = VERBS[actions];
    return verb ? `${h.name} ${verb} ${where} in S3 through its ${one.PolicyName} policy.` : `${h.name} is allowed ${actions} on ${where} through its ${one.PolicyName} policy.`;
  }
  return `${h.name} holds ${list(names)}, ${names.length === 1 ? "which says" : "which say"} what it can do beside this key.`;
}

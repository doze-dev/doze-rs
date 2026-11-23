// What a call acted on, as a name the Resources space can open: the record keeps the value as the caller wrote it (a queue URL,
// an ARN, a name, an S3 path); this reads the name out of it. Nothing is looked up: a value it cannot read is shown as it is.
import type { CallRecord } from "../../api/types/CallRecord";

/** The name in `value`: the last segment of a URL's path; the resource of an ARN (`arn:aws:sns:…:orders` is `orders`,
 * `arn:aws:lambda:…:function:resize` is `resize`, `arn:aws:iam::…:role/path/ci` is `ci`); anything else as it is. */
export function nameIn(value: string): string {
  if (/^https?:\/\//i.test(value)) {
    try {
      const segs = new URL(value).pathname.split("/").filter(Boolean);
      return segs[segs.length - 1] ?? value;
    } catch {
      return value;
    }
  }
  if (value.startsWith("arn:")) {
    const parts = value.split(":");
    if (parts.length < 6) return value;
    const rest = parts.slice(5);
    // `type:name[:qualifier]` (Lambda, Step Functions, Logs) or `type/…/name` (IAM, KMS, EventBridge) or the name alone (SNS, SQS).
    if (rest.length >= 2) return rest[1] || value;
    const one = rest[0] ?? "";
    const slash = one.split("/").filter(Boolean);
    return slash.length > 1 ? slash[slash.length - 1]! : one || value;
  }
  return value;
}

/** The name of what `call` acted on, or null when it named nothing. An S3 path names its bucket; a subscription is named by its
 * whole ARN (its last part alone names nothing); a secret's ARN ends in the six characters Secrets Manager adds to its name. */
export function resourceName(call: Pick<CallRecord, "resource" | "member" | "service">): string | null {
  const r = call.resource;
  if (!r) return null;
  if (call.member === null && call.service === "s3") return r.split("/")[0] || null;
  if (call.member === "SubscriptionArn") return r;
  if (call.service === "secretsmanager" && r.startsWith("arn:")) return nameIn(r).replace(/-[A-Za-z0-9]{6}$/, "");
  return nameIn(r);
}

/** What the wire shows of the resource: an S3 path whole (its key says which object), anything else by its name; the whole value
 * is in the drawer. */
export function resourceShown(call: Pick<CallRecord, "resource" | "member" | "service">): string {
  if (!call.resource) return "";
  return call.member === null ? call.resource : nameIn(call.resource);
}

/** Where the Resources space shows it: `/resources/<service>/<name>` (with {@link resourceSearchOf}, which says its kind); null
 * without a service or a name. */
export function resourceHref(call: Pick<CallRecord, "resource" | "member" | "service">): string | null {
  const name = resourceName(call);
  if (!name || !call.service) return null;
  return `/resources/${encodeURIComponent(call.service)}/${encodeURIComponent(name)}`;
}

/** The search of {@link resourceHref}: the input member that named the resource, by which the Resources space knows its kind. */
export function resourceSearchOf(call: Pick<CallRecord, "member">): { m?: string } {
  return call.member ? { m: call.member } : {};
}

// How a recorded call reads in a resource's Recent calls: who made it, in words a person knows, and how it ended. Pure; the
// table that draws them is `recent-calls.tsx`. The record holds the access key, the client's User-Agent, the status, the error and
// how long the call took; it does not hold the answer's body, so a result says only what the record knows.
import { micros } from "@doze/ui/lib/format";
import type { CallRecord } from "../api/types/CallRecord";
import type { KeyPrincipal } from "../api/types/KeyPrincipal";
import { isConsole } from "../pages/live/search";
import { clientName, who } from "../people";
import { serviceTitle } from "../words";

export type CallerWords = {
  /** The name to show: an IAM user, `EventBridge rule payment-settled` for a call a service made, else the client (`AWS CLI`). */
  label: string;
  /** The access key, shown small beside a label that is not the key's own name; `null` when the label is the key. */
  key: string | null;
  /** The whole story, for hover. */
  title: string;
};

/** Who made `call`. A call between services names the service and what it made the call for (`EventBridge rule payment-settled`);
 * an IAM user is shown by name; any other key by the client that signed with it (`AWS CLI`, `boto3`), the key beside it. */
export function callerWords(call: Pick<CallRecord, "caller" | "via" | "sdk">, titles: ReadonlyMap<string, string>, names: ReadonlyMap<string, KeyPrincipal>): CallerWords {
  const key = call.caller;
  // The console's own calls go over the services' bus, so they carry a `via` that names no service: they are the console's, not "a service".
  if (key === "console" && call.via && !call.via.by) return { label: "This console", key: null, title: who({ key, kind: "console", name: null, arn: null }).title };
  if (call.via) {
    const by = call.via.by ? (serviceTitle(call.via.by, titles) ?? call.via.by) : "A service, as the caller's role";
    const label = call.via.name ? `${by} ${call.via.name}` : by;
    return { label, key: null, title: `${label}, made for ${key ?? "no key"}` };
  }
  if (!key) return { label: "–", key: null, title: "The request carried no access key." };
  if (key === "console") return { label: "This console", key: null, title: who({ key, kind: "console", name: null, arn: null }).title };
  const p = names.get(key);
  if (p?.kind === "user" && p.name) return { label: p.name, key: null, title: who(p).title };
  const client = call.sdk ? clientName(call.sdk) : null;
  if (client) return { label: client, key, title: `${client}, signing with ${key}${p ? `. ${who(p).title}` : ""}` };
  return { label: key, key: null, title: p ? who(p).title : key };
}

/** What the record knows of how a call went beyond its status: how long it took, as `3 ms`. Empty for a call between services,
 * which has no request of its own to time. */
export function tookWords(call: Pick<CallRecord, "duration_us" | "request_id">): string {
  if (call.request_id === "") return "";
  return micros(call.duration_us);
}

/** The keys of `calls` IAM may know by name: the access keys of the wire (the console's and a service's are not IAM's). */
export function namedKeys(calls: readonly Pick<CallRecord, "caller" | "via">[]): string {
  return [...new Set(calls.filter((c) => c.via === null && c.caller?.startsWith("AKIA")).map((c) => c.caller as string))].sort().join(",");
}

/** The object an S3 call acted on, beside the bucket its page is about (`a/one.txt` of `pics/a/one.txt`); null for any call that
 * named no key. A call from the wire names it in its path; a call this console made names it in its input; both are recorded as
 * `bucket/key` with no member. */
export function targetKey(call: Pick<CallRecord, "service" | "member" | "resource">): string | null {
  if (call.service !== "s3" || call.member !== null || !call.resource) return null;
  const at = call.resource.indexOf("/");
  return at < 0 || at === call.resource.length - 1 ? null : call.resource.slice(at + 1);
}

/** An operation that only looks and changes nothing (`ListQueues`, `DescribeTable`, `GetFunction`, `HeadObject`, a `Query`). The
 * console makes dozens of them to draw a page; a write (`CreateQueue`, `PutObject`, `StartMessageMoveTask`) is something a
 * person did, whoever made it. `ReceiveMessage` is not here: it changes what a queue holds. */
const READS = /^(List|Describe|Get|Head|BatchGet|Query|Scan|Filter|Search|Lookup|Validate)/;
export const isReadCall = (c: Pick<CallRecord, "action">): boolean => c.action !== null && READS.test(c.action);

/** This console's own read, which a resource's Recent calls hides until asked: a page makes dozens of them to draw itself, and
 * they bury the calls of an app and the console's own writes. `pin` names reads that stay in view (the `GetSecretValue` a page
 * has just made when the person chose Show). */
export const isConsoleRead = (c: Pick<CallRecord, "caller" | "action">, pin: readonly string[] = []): boolean => isConsole(c) && isReadCall(c) && !(c.action !== null && pin.includes(c.action));

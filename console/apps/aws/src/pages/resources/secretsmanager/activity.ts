// What the Live record says of the secrets lately, for the line over the list: who read which secret, and whether anything was changed
// today. Pure here; `useSecretActivity` reads the record and the callers' names.
import { useQuery } from "@tanstack/react-query";
import { useAbout } from "../../../api/queries";
import { client, PATHS } from "../../../api/client";
import { usePrincipals } from "../../../api/spaces";
import type { CallsPage } from "../../../api/types/CallsPage";
import { callerWords, namedKeys } from "../../../kit/calls";
import { ago } from "../../../people";
import { isConsole } from "../../live/search";

export type Touch = { action: string | null; resource: string | null; who: string; at: number; /** The call was this console's own. */ console: boolean };

const READS = ["GetSecretValue", "BatchGetSecretValue"];
const CHANGES = ["PutSecretValue", "UpdateSecret", "CreateSecret", "RotateSecret", "DeleteSecret", "RestoreSecret", "UpdateSecretVersionStage", "PutResourcePolicy", "TagResource", "UntagResource"];

/** The call is about this secret: its `SecretId` is the name or an ARN of it. */
const about = (resource: string | null, name: string): boolean => resource === name || (resource?.includes(":secret:") === true && new RegExp(`:secret:${name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}(-[A-Za-z0-9]{6})?$`).test(resource));

/** `payments (go) read larder/prod/stripe-key 2 minutes ago, and orders (go) read larder/prod/db-credentials at 08:11. Neither secret was changed today.`
 * `touches` are the calls that were not the console's, newest first; `names` are the secrets listed. Empty when nothing read any of them. */
export function activityWords(touches: readonly Touch[], names: readonly string[], now: number): { text: string; names: string[] } {
  const startOfDay = new Date(now).setUTCHours(0, 0, 0, 0);
  const reads: { name: string; who: string; at: number }[] = [];
  for (const name of names) {
    const t = touches.find((c) => !c.console && c.action !== null && READS.includes(c.action) && about(c.resource, name));
    if (t) reads.push({ name, who: t.who, at: t.at });
  }
  reads.sort((a, b) => b.at - a.at);
  const shown = reads.slice(0, 2);
  const said = shown.map((r, i) => {
    const when = i === 0 ? ago(r.at, now) : `at ${new Date(r.at).toISOString().slice(11, 16)}`;
    return `${r.who} read ${r.name} ${when}`;
  });
  const changed = names.filter((n) => touches.some((c) => c.action !== null && CHANGES.includes(c.action) && about(c.resource, n) && c.at >= startOfDay));
  const tail = names.length === 0 ? "" : changed.length === 0 ? (names.length === 1 ? ` ${names[0]} was not changed today.` : names.length === 2 ? " Neither secret was changed today." : " No secret was changed today.") : ` ${changed.join(" and ")} ${changed.length === 1 ? "was" : "were"} changed today.`;
  if (said.length === 0) return { text: "", names: [] };
  return { text: `${said.join(", and ")}.${tail}`, names: [...shown.map((r) => r.name), ...shown.map((r) => r.who)] };
}

/** The line over the list of secrets, from the Live record (the newest 200 calls). */
export function useSecretActivity(names: readonly string[], now: number): { text: string; names: string[] } {
  const calls = useQuery({ queryKey: ["secretsmanager", "calls"], queryFn: ({ signal }) => client.api<CallsPage>(`${PATHS.calls}?limit=200`, {}, signal), retry: false, enabled: names.length > 0 });
  const mine = (calls.data?.calls ?? []).filter((c) => c.service === "secretsmanager").reverse();
  const about = useAbout().data;
  const principals = usePrincipals(namedKeys(mine));
  const titles = new Map((about?.services ?? []).map((s) => [s.name, s.title]));
  const keys = new Map((principals.data?.principals ?? []).map((p) => [p.key, p]));
  const touches: Touch[] = mine.map((c) => ({ action: c.action, resource: c.resource, who: callerWords(c, titles, keys).label, at: c.at_ms, console: isConsole(c) }));
  return activityWords(touches, names, now);
}

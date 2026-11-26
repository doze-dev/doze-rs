// The callers a rule can name: the access keys the account has (each IAM user's keys, read through the console's own IAM lists) and the callers
// Live has seen, so a key an app uses that IAM does not list can still be chosen. A key not in either list is typed in. Names stand beside keys as
// Live's Caller column words them: an IAM user by name, any other key by the client that signed with it.
import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import type { Option } from "@doze/ui/components/app/combobox";
import { client, PATHS } from "../../api/client";
import { useAbout } from "../../api/queries";
import type { CallRecord } from "../../api/types/CallRecord";
import type { CallsPage } from "../../api/types/CallsPage";
import { clientName } from "../../people";
import { useAllRows } from "../resources/cfg-api";
import type { AccessKey } from "../resources/iam/model";
import { useUserKeys } from "../resources/iam/user-data";

/** How many of the newest calls Live is asked for to find who called. */
const SEEN = 500;

/** The options: IAM's keys (by user, then key; an inactive one says so), then the other callers Live has seen (by key; the client beside it).
 * The console's own key and the keys of a call made on a service's behalf are not callers a rule can fit. */
export function callerOptions(keys: readonly AccessKey[], calls: readonly Pick<CallRecord, "caller" | "via" | "sdk">[]): Option[] {
  const issued = [...keys].sort((a, b) => a.UserName.localeCompare(b.UserName) || a.AccessKeyId.localeCompare(b.AccessKeyId));
  const known = new Set(issued.map((k) => k.AccessKeyId));
  const out: Option[] = issued.map((k) => ({ value: k.AccessKeyId, label: k.AccessKeyId, hint: k.Status === "Active" ? k.UserName : `${k.UserName} (inactive)` }));
  const seen = new Map<string, string>();
  for (const c of calls) {
    if (c.via !== null || !c.caller || c.caller === "console" || known.has(c.caller)) continue;
    const client = c.sdk ? clientName(c.sdk) : "";
    if (!seen.has(c.caller) || (client && !seen.get(c.caller))) seen.set(c.caller, client);
  }
  for (const key of [...seen.keys()].sort()) out.push({ value: key, label: key, hint: seen.get(key) || "seen on Live" });
  return out;
}

/** What the Callers picker lists, read when the rule form is open: IAM's users and each one's keys, and the newest calls on Live. */
export function useCallerOptions(): Option[] {
  const served = useAbout().data?.services.some((s) => s.name === "iam" && s.served) ?? false;
  const users = useAllRows("iam", "users", { enabled: served });
  const names = useMemo(() => users.rows.map((r) => r.name), [users.rows]);
  const { byUser } = useUserKeys(names);
  const calls = useQuery({ queryKey: ["lab", "callers"], queryFn: ({ signal }) => client.api<CallsPage>(`${PATHS.calls}?limit=${SEEN}`, {}, signal), retry: false, staleTime: 5_000 });
  const keys = [...byUser.values()].flatMap((u) => u.keys);
  const stamp = keys.map((k) => `${k.AccessKeyId}${k.Status}`).join();
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `stamp` stands for the keys, which change identity on each render
  return useMemo(() => callerOptions(keys, calls.data?.calls ?? []), [stamp, calls.data]);
}

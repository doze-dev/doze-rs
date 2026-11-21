// What the user pages read from IAM beyond a user's own detail: each user's access keys and when each was last used, and the
// policies a user gets through its groups.
import { useMemo } from "react";
import { GROUPS, USERS, useEach, useRead } from "./api";
import { type AccessKey, type AttachedPolicy, type KeyUsed, byUse } from "./model";

export type UserKeys = { keys: AccessKey[]; /** Epoch seconds, by key ID; absent for a key never used. */ used: Map<string, KeyUsed["AccessKeyLastUsed"]> };

/** The access keys of each of `names` and the last use of each key: two reads, the second once the first has answered. */
export function useUserKeys(names: readonly string[]): { byUser: Map<string, UserKeys>; ready: boolean; error: unknown } {
  const lists = useEach<{ AccessKeyMetadata: AccessKey[] }>(USERS, "ListAccessKeys", names.map((UserName) => ({ UserName })));
  const keys = lists.flatMap((q) => q.data?.AccessKeyMetadata ?? []);
  const used = useEach<KeyUsed>(USERS, "GetAccessKeyLastUsed", keys.map((k) => ({ AccessKeyId: k.AccessKeyId })));
  const listed = lists.every((q) => q.isSuccess || q.isError);
  const done = listed && used.every((q) => q.isSuccess || q.isError);
  const stamp = lists.map((q) => q.dataUpdatedAt).join() + used.map((q) => q.dataUpdatedAt).join();
  const byUser = useMemo(() => {
    const out = new Map<string, UserKeys>();
    names.forEach((n, i) => out.set(n, { keys: byUse(lists[i]?.data?.AccessKeyMetadata ?? []), used: new Map() }));
    keys.forEach((k, i) => {
      const u = used[i]?.data?.AccessKeyLastUsed;
      if (u) out.get(k.UserName)?.used.set(k.AccessKeyId, u);
    });
    return out;
    // `stamp` stands for the answers: they change identity on each render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [names.join("\u0000"), stamp]);
  return { byUser, ready: names.length === 0 || done, error: lists.find((q) => q.isError)?.error };
}

/** The policies each group of `groups` holds, by group. */
export function useGroupPolicies(groups: readonly string[]): { byGroup: Map<string, AttachedPolicy[]>; ready: boolean } {
  const held = useEach<{ AttachedPolicies: AttachedPolicy[] }>(GROUPS, "ListAttachedGroupPolicies", groups.map((GroupName) => ({ GroupName })));
  const stamp = held.map((q) => q.dataUpdatedAt).join();
  const byGroup = useMemo(() => new Map(groups.map((g, i) => [g, held[i]?.data?.AttachedPolicies ?? []] as const)),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [groups.join("\u0000"), stamp]);
  return { byGroup, ready: groups.length === 0 || held.every((q) => q.isSuccess || q.isError) };
}

export type UserAccess = {
  keys: AccessKey[];
  used: Map<string, KeyUsed["AccessKeyLastUsed"]>;
  groups: string[];
  direct: AttachedPolicy[];
  inline: string[];
  viaGroups: Map<string, AttachedPolicy[]>;
  ready: boolean;
  error: unknown;
};

/** Everything a user's page shows beyond the user: its keys and their last use, its groups, the policies attached to it, the policies its groups give it. */
export function useUserAccess(name: string): UserAccess {
  const { byUser, ready: keysReady, error } = useUserKeys([name]);
  const groups = useRead<{ Groups: { GroupName: string }[] }>(USERS, "ListGroupsForUser", { UserName: name });
  const direct = useRead<{ AttachedPolicies: AttachedPolicy[] }>(USERS, "ListAttachedUserPolicies", { UserName: name });
  const inline = useRead<{ PolicyNames: string[] }>(USERS, "ListUserPolicies", { UserName: name });
  const names = useMemo(() => (groups.data?.Groups ?? []).map((g) => g.GroupName), [groups.data]);
  const via = useGroupPolicies(names);
  const mine = byUser.get(name);
  return {
    keys: mine?.keys ?? [],
    used: mine?.used ?? new Map(),
    groups: names,
    direct: direct.data?.AttachedPolicies ?? [],
    inline: inline.data?.PolicyNames ?? [],
    viaGroups: via.byGroup,
    ready: keysReady && groups.isSuccess && direct.isSuccess && inline.isSuccess && via.ready,
    error: error ?? groups.error ?? direct.error ?? inline.error,
  };
}

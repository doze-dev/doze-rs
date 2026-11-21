// A user's page (iam__user-detail, user-credentials, user-policies-none, access-keys-none, group tab): the header with what the user is
// doing, the strip from its groups to its key, the Summary, and the tabs Permissions, Groups, Tags and Security credentials. `?view=access-key`
// is Create access key, as a page.
import { useSearch } from "@tanstack/react-router";
import { lazy, Suspense, useState } from "react";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useUrlParam } from "@doze/ui/hooks/use-url-param";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { RecentCalls, ResourcePage, TagsTab, WiredStrip, type Wired } from "../../../kit";
import { Entity } from "./entity";
import { DeleteEntity } from "./delete-entity";
import { count, shortKey } from "./model";
import { fieldOf, Say, Summary } from "./parts";
import { userSentence } from "./sentences";
import { useUserAccess } from "./user-data";
import { SecurityCredentials } from "./user-keys";
import { GroupsTab, PermissionsTab } from "./user-tabs";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { useToList } from "./list-parts";

const NewKey = lazy(() => import("./access-key-new").then((m) => ({ default: m.AccessKeyNew })));

export function UserDetail({ name }: { service: string; kind: string; name: string }) {
  const search = useSearch({ strict: false }) as { view?: string };
  if (search.view === "access-key") return <Suspense fallback={<Skeleton className="h-96 rounded-[14px]" />}><NewKey user={name} /></Suspense>;
  return <Entity kind="users" name={name} render={(d, t) => <UserView d={d} {...t} />} />;
}

function UserView({ d, updatedAt, fetching, refresh }: { d: Parameters<Parameters<typeof Entity>[0]["render"]>[0]; updatedAt: number; fetching: boolean; refresh: () => void }) {
  const name = d.name;
  const access = useUserAccess(name);
  const toList = useToList("users");
  const [tab, setTab] = useUrlParam("tab");
  const [deleting, setDeleting] = useState(false);
  const active = access.keys.filter((k) => k.Status === "Active");
  const key = active[0] ?? access.keys[0] ?? null;
  const used = key ? access.used.get(key.AccessKeyId) : undefined;
  const viaNames = access.groups.flatMap((g) => access.viaGroups.get(g) ?? []).map((p) => p.PolicyName);
  const sentence = userSentence({ name, groups: access.groups, viaGroups: viaNames, direct: access.direct.map((p) => p.PolicyName), key: key ? { id: shortKey(key.AccessKeyId), usedAt: used?.LastUsedDate ?? null, service: used?.ServiceName ?? null, region: used?.Region ?? null } : null });
  const created = fieldOf(d, "Created");
  const meta = `${created ? `Created ${created}. ` : ""}${active.length === 0 ? "Holds no active access key." : `Holds ${active.length === 1 ? "one active access key" : `${active.length} active access keys`}.`}`;
  const wired: Wired[] = [
    ...access.groups.map((g) => ({ label: "group", to: { service: "iam", kind: "groups", name: g }, note: null })),
    ...(key ? [{ label: "key", to: { service: "iam", kind: "users", name }, note: null }] : []),
  ];
  const self = access.groups.length;
  const shown = { ...d, sentence: null, wired: [], verbs: [], badges: [{ text: "IAM user", tone: "neutral" as const }] };
  const tabs = [
    { id: "permissions", label: "Permissions", render: () => <PermissionsTab user={name} access={access} boundary={fieldOf(d, "Permissions boundary")} /> },
    { id: "groups", label: "Groups", count: access.groups.length, render: () => <GroupsTab user={name} access={access} /> },
    { id: "tags", label: "Tags", render: () => <TagsTab target={{ service: "iam", kind: "users", name }} noun="tag" explain="Tags are key-value labels on the user. Create access key stores a description as one." /> },
    { id: "security", label: "Security credentials", render: () => <SecurityCredentials user={name} access={access} /> },
  ];
  return (
    <>
      <ResourcePage
        detail={shown}
        meta={meta}
        refresh={<RefreshControl updatedAt={updatedAt} fetching={fetching} onRefresh={refresh} interval={false} />}
        actions={[{ label: "Delete user", onSelect: () => setDeleting(true) }]}
        overview={
          <>
            {access.ready || access.error ? <Say text={sentence} names={[name, ...access.groups, ...viaNames, ...access.direct.map((p) => p.PolicyName)]} bold={[/AKIA\S+/, /\b\d+ groups?\b/, /\b\d\d:\d\d\b/]} className="mb-6" testId="detail-sentence" /> : <Skeleton className="mb-6 h-6 w-2/3" />}
            {wired.length > 0 && <WiredStrip wired={wired} self={{ name, at: self }} chipText={(w) => (w.label === "key" ? `access key ${shortKey(key!.AccessKeyId)}` : `${w.to.name} group`)} chipHref={(w) => (w.label === "key" ? `/resources/iam/users/${encodeURIComponent(name)}?tab=security` : undefined)} />}
            <Summary facts={[
              { label: "ARN", value: <span className="font-mono">{fieldOf(d, "ARN")}</span> },
              { label: "Created", value: created },
              { label: "Access keys", value: key ? <span className="grid leading-5"><span className="flex flex-wrap items-center gap-2"><span className="font-mono">{key.AccessKeyId}</span><StatusDot tone={key.Status === "Active" ? "ok" : "idle"}>{key.Status}</StatusDot>{access.keys.length > 1 && <span className="text-muted-foreground">and {count(access.keys.length - 1, "more")}</span>}</span><span className="text-[12.5px] text-muted-foreground">{used?.LastUsedDate ? `Used ${dateTimeUtc(used.LastUsedDate * 1000)} with ${used.ServiceName} in ${used.Region}` : "Never used"}</span></span> : <span className="text-muted-foreground">None</span> },
            ]} />
          </>
        }
        tabs={tabs}
        tab={tab ?? "permissions"}
        onTab={(id) => setTab(id === "permissions" ? undefined : id)}
        details={false}
        testId="user-page"
      />
      <div className="mt-12"><RecentCalls calls={d.calls} noun="user" /></div>
      <DeleteEntity kind="users" name={name} open={deleting} onOpenChange={setDeleting} onDeleted={() => { setDeleting(false); toList({ gone: name }); }} />
    </>
  );
}

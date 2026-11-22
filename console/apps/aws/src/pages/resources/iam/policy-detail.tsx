// A policy's page (iam__policy-detail): what it is attached to said in a sentence, the strip from its entities to what it reaches, the Summary, and
// the tabs Permissions (the document as a table or JSON), Entities attached, Tags and Policy versions. `?view=version` is Create policy version.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { lazy, Suspense, useEffect, useState } from "react";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useUrlParam } from "@doze/ui/hooks/use-url-param";
import { useAbout } from "../../../api/queries";
import { RecentCalls, ResourcePage, WiredStrip, type Wired } from "../../../kit";
import { POLICIES, useRead } from "./api";
import { AttachTo } from "./attach-to";
import { DeleteEntity } from "./delete-entity";
import { Entity } from "./entity";
import { useToList } from "./list-parts";
import { isAwsManaged, policyArnOfName, type Doc } from "./model";
import { fieldOf, Say, Summary } from "./parts";
import { docResources } from "./policy-data";
import { EntitiesTab, PermissionsTab, TagsTab, VersionsTab, type Entities, type Versions } from "./policy-tabs";
import { policySentence } from "./sentences";

const NewVersion = lazy(() => import("./policy-version-new").then((m) => ({ default: m.PolicyVersionNew })));

export function PolicyDetail({ name }: { service: string; kind: string; name: string }) {
  const search = useSearch({ strict: false }) as { view?: string };
  const navigate = useNavigate();
  const account = useAbout().data?.account;
  const target = account === undefined ? null : policyArnOfName(account, name);
  // A bare name (typed, bookmarked) is taken to the policy's page, which is at its ARN; it is not asked of the API as a name.
  useEffect(() => {
    if (target !== null) void navigate({ to: "/resources/$service/$kind/$name", params: { service: "iam", kind: "policies", name: target }, search, replace: true });
  }, [target, navigate, search]);
  if (policyArnOfName("0", name) !== null) return <Skeleton className="h-96 rounded-[14px]" />;
  if (search.view === "version") return <Suspense fallback={<Skeleton className="h-96 rounded-[14px]" />}><Entity kind="policies" name={name} render={(d) => <NewVersion arn={name} label={fieldOf(d, "Name") ?? name} doc={fieldOf(d, "Document") ?? "{}"} />} /></Suspense>;
  return <Entity kind="policies" name={name} render={(d, t) => <PolicyView d={d} {...t} />} />;
}

function PolicyView({ d, updatedAt, fetching, refresh }: { d: Parameters<Parameters<typeof Entity>[0]["render"]>[0]; updatedAt: number; fetching: boolean; refresh: () => void }) {
  const arn = d.name;
  const navigate = useNavigate();
  const toList = useToList("policies");
  const [tab, setTab] = useUrlParam("tab");
  const [dialog, setDialog] = useState<"delete" | "attach" | null>(null);
  const label = fieldOf(d, "Name") ?? arn;
  const aws = isAwsManaged(arn);
  const entities = useRead<Entities>(POLICIES, "ListEntitiesForPolicy", { PolicyArn: arn });
  const versions = useRead<Versions>(POLICIES, "ListPolicyVersions", { PolicyArn: arn });
  const e = entities.data;
  const roles = (e?.PolicyRoles ?? []).map((r) => r.RoleName);
  const users = (e?.PolicyUsers ?? []).map((u) => u.UserName);
  const groups = (e?.PolicyGroups ?? []).map((g) => g.GroupName);
  const doc: Doc = fieldOf(d, "Document") ?? "{}";
  const version = fieldOf(d, "Default version") ?? "v1";
  const created = fieldOf(d, "Created");
  const updated = fieldOf(d, "Updated");
  const reaches = docResources([doc]).slice(0, 3);
  const wired: Wired[] = [
    ...roles.slice(0, 2).map((n) => ({ label: "role", to: { service: "iam", kind: "roles", name: n }, note: `${n} role` })),
    ...users.slice(0, 2).map((n) => ({ label: "user", to: { service: "iam", kind: "users", name: n }, note: `${n} user` })),
    ...groups.slice(0, 2).map((n) => ({ label: "group", to: { service: "iam", kind: "groups", name: n }, note: `${n} group` })),
    ...reaches.map((r) => ({ label: "resource", to: { service: r.service, kind: r.kind, name: r.name }, note: r.label })),
  ];
  const before = wired.filter((w) => w.label !== "resource").length;
  const shown = { ...d, name: label, sentence: null, wired: [], verbs: [], badges: [{ text: aws ? "AWS managed" : "Customer managed", tone: "neutral" as const }] };
  const total = roles.length + users.length + groups.length;
  const tabs = [
    { id: "permissions", label: "Permissions", render: () => <PermissionsTab doc={doc} version={version} aws={aws} /> },
    { id: "entities", label: "Entities attached", count: total, render: () => <EntitiesTab arn={arn} label={label} roles={roles} users={users} groups={groups} onAttach={() => setDialog("attach")} /> },
    { id: "tags", label: "Tags", render: () => <TagsTab arn={arn} /> },
    { id: "versions", label: "Policy versions", count: versions.data?.Versions.length, render: () => <VersionsTab arn={arn} versions={versions.data?.Versions ?? []} onNew={() => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "iam", kind: "policies", name: arn }, search: { view: "version" } })} /> },
  ];
  return (
    <>
      <ResourcePage
        detail={shown}
        meta={fieldOf(d, "Description") ?? undefined}
        refresh={<RefreshControl updatedAt={updatedAt} fetching={fetching} onRefresh={refresh} interval={false} />}
        actions={[{ label: "Attach to a user, group or role", onSelect: () => setDialog("attach") }, ...(aws ? [] : [{ label: "Delete policy", onSelect: () => setDialog("delete") }])]}
        overview={
          <>
            {entities.isSuccess ? <Say text={policySentence(label, { users, roles, groups }, version, created !== updated, aws)} names={[label, ...roles, ...users, ...groups]} bold={[/\b\d+ (roles?|users?|groups?)\b/, /\bv\d+\b/]} className="mb-6" testId="detail-sentence" /> : <Skeleton className="mb-6 h-6 w-2/3" />}
            {wired.length > 0 && <WiredStrip wired={wired} self={{ name: label, at: before }} chipText={(w) => w.note ?? w.to.name} />}
            <Summary facts={[{ label: "ARN", value: <span className="font-mono break-all">{arn}</span> }, { label: "Created", value: created }, { label: "Last updated", value: updated }]} />
          </>
        }
        tabs={tabs}
        tab={tab ?? "permissions"}
        onTab={(id) => setTab(id === "permissions" ? undefined : id)}
        details={false}
        testId="policy-page"
      />
      <div className="mt-12"><RecentCalls calls={d.calls} noun="policy" title="Recent calls" /></div>
      <AttachTo arn={arn} label={label} open={dialog === "attach"} onOpenChange={(o) => !o && setDialog(null)} />
      <DeleteEntity kind="policies" name={arn} label={label} open={dialog === "delete"} onOpenChange={(o) => !o && setDialog(null)} onDeleted={() => { setDialog(null); toList({ gone: label }); }} />
    </>
  );
}

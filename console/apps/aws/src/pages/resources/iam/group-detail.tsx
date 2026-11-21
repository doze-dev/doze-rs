// A user group's page (iam__group-detail, group-members-none): what it holds said in a sentence, the strip from its users to its policies,
// the Summary, and the tabs Users and Permissions.
import { Alert } from "@doze/ui/components/app/alert";
import { type DataColumn } from "@doze/ui/components/app/data-table";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useUrlParam } from "@doze/ui/hooks/use-url-param";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { ShieldOffIcon, UserPlusIcon, UsersIcon } from "lucide-react";
import { useState } from "react";
import { RecentCalls, ResourcePage, WiredStrip, type Wired } from "../../../kit";
import { GROUPS, useRead, useWrite } from "./api";
import { DeleteEntity } from "./delete-entity";
import { AddUsers, AttachPolicies } from "./dialogs";
import { Entity } from "./entity";
import { useToList } from "./list-parts";
import { Card, fieldOf, Say, Summary, To } from "./parts";
import { groupSentence } from "./sentences";
import type { AttachedPolicy } from "./model";

type Held = { Users: { UserName: string; CreateDate: number }[] };

export function GroupDetail({ name }: { service: string; kind: string; name: string }) {
  return <Entity kind="groups" name={name} render={(d, t) => <GroupView d={d} {...t} />} />;
}

function GroupView({ d, updatedAt, fetching, refresh }: { d: Parameters<Parameters<typeof Entity>[0]["render"]>[0]; updatedAt: number; fetching: boolean; refresh: () => void }) {
  const name = d.name;
  const toList = useToList("groups");
  const [tab, setTab] = useUrlParam("tab");
  const [deleting, setDeleting] = useState(false);
  const [adding, setAdding] = useState<"users" | "policies" | null>(null);
  const held = useRead<Held>(GROUPS, "GetGroup", { GroupName: name });
  const attached = useRead<{ AttachedPolicies: AttachedPolicy[] }>(GROUPS, "ListAttachedGroupPolicies", { GroupName: name });
  const leave = useWrite(GROUPS, "RemoveUserFromGroup");
  const detach = useWrite(GROUPS, "DetachGroupPolicy");
  const [chosen, setChosen] = useState<ReadonlySet<string>>(new Set());
  const [chosenPolicies, setChosenPolicies] = useState<ReadonlySet<string>>(new Set());
  const users = held.data?.Users ?? [];
  const policies = attached.data?.AttachedPolicies ?? [];
  const ready = held.isSuccess && attached.isSuccess;
  const created = fieldOf(d, "Created");
  const wired: Wired[] = [
    ...users.slice(0, 4).map((u) => ({ label: "user", to: { service: "iam", kind: "users", name: u.UserName }, note: null })),
    ...policies.slice(0, 4).map((p) => ({ label: "policy", to: { service: "iam", kind: "policies", name: p.PolicyArn }, note: null })),
  ];
  const nUsers = Math.min(users.length, 4);
  const shown = { ...d, sentence: null, wired: [], verbs: [], badges: [{ text: "User group", tone: "neutral" as const }] };
  const userColumns: DataColumn<{ UserName: string; CreateDate: number }>[] = [
    { id: "name", header: "User name", size: 360, accessorFn: (r) => r.UserName, cell: (c) => <To kind="users" name={c.row.original.UserName} /> },
    { id: "created", header: "Created", accessorFn: (r) => r.CreateDate, cell: (c) => dateTimeUtc(c.row.original.CreateDate * 1000) },
  ];
  const addUsers = <Button type="button" onClick={() => setAdding("users")} data-testid="add-users"><UserPlusIcon /> Add users</Button>;
  const attach = <Button type="button" onClick={() => setAdding("policies")} data-testid="attach-policies"><ShieldOffIcon /> Attach policies</Button>;
  const tabs = [
    {
      id: "users",
      label: "Users",
      count: users.length,
      render: () => users.length === 0 && ready ? (
        <EmptyTab icon={UsersIcon} title="No users in this group" action={addUsers} testId="members-none">{name} has no members. Add users to give them the group&apos;s permissions.</EmptyTab>
      ) : (
        <Card title={`Users in this group (${users.length})`} lede="Members get every permission the group's policies grant." actions={addUsers} testId="members-card">
          <SelectableTable aria-label="Users in this group" data={users} columns={userColumns} rowId={(r) => r.UserName} selected={chosen} onSelectedChange={setChosen} noun="user" selectedLabel={(c) => `${c} selected`} hint="Choose users to remove them from the group." actions={(rs) => <Button type="button" variant="outline" size="sm" className="border-bad/40 bg-card text-bad hover:text-bad" onClick={() => { for (const r of rs) leave.mutate({ UserName: r.UserName, GroupName: name }); setChosen(new Set()); }} data-testid="remove-users">Remove {rs.length === 1 ? "user" : "users"}</Button>} maxHeight={320} />
          {leave.error && <Alert tone="bad" title="RemoveUserFromGroup was refused" testId="leave-refused">{leave.error.message}</Alert>}
        </Card>
      ),
    },
    {
      id: "permissions",
      label: "Permissions",
      count: policies.length,
      render: () => policies.length === 0 && ready ? (
        <EmptyTab icon={ShieldOffIcon} title="No permissions policies" action={attach} testId="policies-none">{name} has no policies attached, so its members get nothing from it.</EmptyTab>
      ) : (
        <Card title={`Permissions policies (${policies.length})`} lede="Every policy attached to the group applies to each user in it." actions={attach} testId="policies-card">
          <SelectableTable aria-label="Permissions policies" data={policies} columns={[{ id: "name", header: "Policy name", size: 360, accessorFn: (r) => r.PolicyName, cell: (c) => <span className="font-mono text-[13px] font-medium text-bright">{c.row.original.PolicyName}</span> }, { id: "type", header: "Type", accessorFn: (r) => (r.PolicyArn.startsWith("arn:aws:iam::aws:") ? "AWS managed" : "Customer managed") }]} rowId={(r) => r.PolicyArn} selected={chosenPolicies} onSelectedChange={setChosenPolicies} noun="policy" selectedLabel={(c) => `${c} selected`} hint="Choose policies to detach them from the group." actions={(rs) => <Button type="button" variant="outline" size="sm" className="border-bad/40 bg-card text-bad hover:text-bad" onClick={() => { for (const r of rs) detach.mutate({ GroupName: name, PolicyArn: r.PolicyArn }); setChosenPolicies(new Set()); }} data-testid="detach-policies">Detach {rs.length === 1 ? "policy" : "policies"}</Button>} maxHeight={320} />
          {detach.error && <Alert tone="bad" title="DetachGroupPolicy was refused" testId="detach-refused">{detach.error.message}</Alert>}
        </Card>
      ),
    },
  ];
  return (
    <>
      <ResourcePage
        detail={shown}
        meta={created ? `Created ${created}.` : undefined}
        refresh={<RefreshControl updatedAt={updatedAt} fetching={fetching} onRefresh={refresh} interval={false} />}
        actions={[{ label: "Delete user group", onSelect: () => setDeleting(true) }]}
        overview={
          <>
            {ready ? <Say text={groupSentence(name, users.map((u) => u.UserName), policies.map((p) => p.PolicyName))} names={[name, ...policies.map((p) => p.PolicyName)]} bold={[/\b\d+ (members?|attached policies|attached policy)\b/]} className="mb-6" testId="detail-sentence" /> : <Skeleton className="mb-6 h-6 w-2/3" />}
            {wired.length > 0 && <WiredStrip wired={wired} self={{ name, at: nUsers }} chipText={(w) => (w.label === "policy" ? `${w.to.name.split("/").pop()} policy` : w.to.name)} chipHref={(w) => (w.label === "policy" ? `/resources/iam/groups/${encodeURIComponent(name)}?tab=permissions` : undefined)} />}
            <Summary facts={[{ label: "User group name", value: <span className="font-mono">{name}</span> }, { label: "ARN", value: <span className="font-mono">{fieldOf(d, "ARN")}</span> }, { label: "Created", value: created }]} />
          </>
        }
        tabs={tabs}
        tab={tab ?? "users"}
        onTab={(id) => setTab(id === "users" ? undefined : id)}
        details={false}
        testId="group-page"
      />
      <div className="mt-12"><RecentCalls calls={d.calls} noun="group" title="Recent calls" /></div>
      <AddUsers group={name} member={users.map((u) => u.UserName)} open={adding === "users"} onOpenChange={(o) => !o && setAdding(null)} />
      <AttachPolicies on="groups" name={name} attached={policies.map((p) => p.PolicyArn)} open={adding === "policies"} onOpenChange={(o) => !o && setAdding(null)} />
      <DeleteEntity kind="groups" name={name} open={deleting} onOpenChange={setDeleting} onDeleted={() => { setDeleting(false); toList({ gone: name }); }} />
    </>
  );
}

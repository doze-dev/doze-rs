// The tabs of a user's page (iam__user-detail, user-policies-none, group tab): the permissions the user has, directly and through its
// groups, the boundary, and the groups it belongs to.
import { Link } from "@tanstack/react-router";
import { ChevronDownIcon, ShieldOffIcon, UsersIcon } from "lucide-react";
import { useState } from "react";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { Button } from "@doze/ui/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@doze/ui/components/ui/dropdown-menu";
import { Alert } from "@doze/ui/components/app/alert";
import { USERS, useWrite } from "./api";
import { AddToGroups, AttachPolicies } from "./dialogs";
import { list } from "./model";
import { Card, Note, PolicyTable, type PolicyRow, To } from "./parts";
import type { UserAccess } from "./user-data";

/** The Add permissions button: a user joins a group, or has policies attached. */
function AddPermissions({ user, access }: { user: string; access: UserAccess }) {
  const [dialog, setDialog] = useState<"group" | "policies" | null>(null);
  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild><Button type="button" variant="outline" className="bg-card" data-testid="add-permissions">Add permissions <ChevronDownIcon /></Button></DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="min-w-52">
          <DropdownMenuItem onSelect={() => setDialog("group")} data-testid="add-to-group">Add user to group</DropdownMenuItem>
          <DropdownMenuItem onSelect={() => setDialog("policies")} data-testid="attach-policies">Attach policies directly</DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      <AddToGroups user={user} member={access.groups} open={dialog === "group"} onOpenChange={(o) => !o && setDialog(null)} />
      <AttachPolicies on="users" name={user} attached={access.direct.map((p) => p.PolicyArn)} open={dialog === "policies"} onOpenChange={(o) => !o && setDialog(null)} />
    </>
  );
}

/** The user's policies: attached to it, embedded in it, and given by its groups. */
export function policyRows(access: UserAccess): PolicyRow[] {
  return [
    ...access.direct.map((p) => ({ ...p, via: "Directly" })),
    ...access.inline.map((n) => ({ PolicyName: n, PolicyArn: `inline:${n}`, via: "Inline" })),
    ...access.groups.flatMap((g) => (access.viaGroups.get(g) ?? []).map((p) => ({ ...p, via: <>Group <span className="font-mono">{g}</span></> }))),
  ];
}

export function PermissionsTab({ user, access, boundary }: { user: string; access: UserAccess; boundary: string | null }) {
  const rows = policyRows(access);
  const only = access.groups.length === 1 ? access.groups[0] : null;
  return (
    <div className="grid gap-6">
      {rows.length === 0 ? (
        <EmptyTab icon={ShieldOffIcon} title="No permissions policies" action={<AddPermissions user={user} access={access} />} testId="permissions-none">
          {user} has no policies attached and belongs to no group, so it is allowed to do nothing.
        </EmptyTab>
      ) : (
        <Card title={`Permissions policies (${rows.length})`} lede="Permissions are defined by policies attached to the user directly or through groups." actions={<AddPermissions user={user} access={access} />} testId="permissions-card">
          <PolicyTable rows={rows} via />
          {access.direct.length === 0 && access.inline.length === 0 && (
            <Note testId="permissions-note">
              Nothing is attached to <b>{user}</b> directly and it has no inline policy, so {only ? <>removing {user} from <b>{only}</b> would leave it with no permissions</> : "leaving its groups would leave it with no permissions"}. <span className="dim">Inline policies are embedded in the user and deleted with it.</span>
            </Note>
          )}
        </Card>
      )}
      <Card title="Permissions boundary" testId="boundary-card" lede={boundary ? <>The boundary is <span className="font-mono text-bright">{boundary}</span>. A permissions boundary controls the maximum permissions this user can have; it grants nothing on its own.</> : "Not set. A permissions boundary controls the maximum permissions this user can have; it grants nothing on its own, so the user still needs a permissions policy."} />
    </div>
  );
}

export function GroupsTab({ user, access }: { user: string; access: UserAccess }) {
  const [chosen, setChosen] = useState<ReadonlySet<string>>(new Set());
  const [adding, setAdding] = useState(false);
  const leave = useWrite(USERS, "RemoveUserFromGroup");
  const rows = access.groups.map((name) => ({ name }));
  const columns: DataColumn<{ name: string }>[] = [
    { id: "name", header: "Group name", size: 320, accessorFn: (r) => r.name, cell: (c) => <To kind="groups" name={c.row.original.name} /> },
    { id: "policies", header: "Attached policies", accessorFn: (r) => (access.viaGroups.get(r.name) ?? []).map((p) => p.PolicyName).join(", "), cell: (c) => { const p = access.viaGroups.get(c.row.original.name) ?? []; return p.length === 0 ? <span className="text-muted-foreground">None</span> : <span className="font-mono text-[13px]">{list(p.map((x) => x.PolicyName))}</span>; } },
  ];
  const addButton = <Button type="button" onClick={() => setAdding(true)} data-testid="add-groups"><UsersIcon /> Add user to groups</Button>;
  return (
    <>
      {rows.length === 0 ? (
        <EmptyTab icon={UsersIcon} title="Not in any group" action={addButton} testId="groups-none">
          {user} belongs to no group, so no group gives it permissions.
        </EmptyTab>
      ) : (
        <Card title={`User groups (${rows.length})`} lede="Every policy attached to a group applies to the users in it." actions={addButton} testId="groups-card">
          <SelectableTable aria-label="Groups" data={rows} columns={columns} rowId={(r) => r.name} selected={chosen} onSelectedChange={setChosen} noun="group" selectedLabel={(c) => `${c} selected`} actions={(rs) => <Button type="button" variant="outline" size="sm" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={leave.isPending} onClick={() => { for (const r of rs) leave.mutate({ UserName: user, GroupName: r.name }); setChosen(new Set()); }} data-testid="leave-groups">Remove from {rs.length === 1 ? "group" : "groups"}</Button>} hint="Choose groups to remove the user from them." maxHeight={320} />
          {leave.error && <Alert tone="bad" title="RemoveUserFromGroup was refused" testId="leave-refused">{leave.error.message}</Alert>}
        </Card>
      )}
      <AddToGroups user={user} member={access.groups} open={adding} onOpenChange={setAdding} />
    </>
  );
}

export { DataTable, Link };

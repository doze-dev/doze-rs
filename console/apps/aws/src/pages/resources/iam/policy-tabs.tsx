// The tabs of a policy's page: its document as a summary or as JSON, what it is attached to, its tags, and its versions.
import { FileTextIcon, GitBranchIcon, Link2OffIcon } from "lucide-react";
import { useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { CodeBlock } from "@doze/ui/components/app/code-block";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { Button } from "@doze/ui/components/ui/button";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { TagsTab as KitTags } from "../../../kit";
import { GROUPS, POLICIES, ROLES, USERS, useWrite } from "./api";
import { type Doc, docText, type Permission, permissions } from "./model";
import { Card, Note, Segmented, To } from "./parts";

export type Entities = { PolicyUsers?: { UserName: string }[]; PolicyRoles?: { RoleName: string }[]; PolicyGroups?: { GroupName: string }[] };
export type Versions = { Versions: { VersionId: string; IsDefaultVersion: boolean; CreateDate: number }[] };

export function PermissionsTab({ doc, version, aws }: { doc: Doc; version: string; aws: boolean }) {
  const [view, setView] = useState<"summary" | "json">("summary");
  const text = docText(doc);
  const pretty = (() => { try { return JSON.stringify(JSON.parse(text), null, 2); } catch { return text; } })();
  const columns: DataColumn<Permission>[] = [
    { id: "service", header: "Service", size: 200, accessorFn: (r) => r.service, cell: (c) => c.row.original.service },
    { id: "actions", header: "Actions", size: 340, accessorFn: (r) => r.actions, cell: (c) => <span className="font-mono text-[13px] break-all">{c.row.original.actions}</span> },
    { id: "resource", header: "Resource", accessorFn: (r) => r.resource, cell: (c) => <span className="font-mono text-[13px] break-all">{c.row.original.resource}</span> },
  ];
  return (
    <Card title="Policy summary" lede={`The default version, ${version}.${aws ? " doze-aws makes this document from the policy's name." : ""}`} actions={<Segmented value={view} onChange={(v) => v && setView(v)} label="Show the policy as" options={[{ value: "summary", label: "Summary" }, { value: "json", label: "JSON" }]} testId="policy-view" />} testId="summary-card">
      {view === "json" ? <CodeBlock text={pretty} wrap={false} testId="policy-json" /> : <DataTable aria-label="Permissions" data={permissions(doc)} columns={columns} rowId={(r) => `${r.service}|${r.actions}|${r.resource}`} maxHeight={420} empty="This policy grants nothing: it has no Allow statement." />}
    </Card>
  );
}

type Row = { type: "role" | "user" | "group"; name: string };

export function EntitiesTab({ arn, label, roles, users, groups, onAttach }: { arn: string; label: string; roles: string[]; users: string[]; groups: string[]; onAttach: () => void }) {
  const [chosen, setChosen] = useState<ReadonlySet<string>>(new Set());
  const detachRole = useWrite(ROLES, "DetachRolePolicy");
  const detachUser = useWrite(USERS, "DetachUserPolicy");
  const detachGroup = useWrite(GROUPS, "DetachGroupPolicy");
  const rows: Row[] = [...roles.map((name) => ({ type: "role" as const, name })), ...users.map((name) => ({ type: "user" as const, name })), ...groups.map((name) => ({ type: "group" as const, name }))];
  const attach = <Button type="button" onClick={onAttach} data-testid="attach-open">Attach to a user, group or role</Button>;
  if (rows.length === 0) return <EmptyTab icon={Link2OffIcon} title="Not attached to anything" action={attach} testId="entities-none">{label} is not attached to any user, group or role, so it grants nothing yet.</EmptyTab>;
  const columns: DataColumn<Row>[] = [
    { id: "name", header: "Name", size: 360, accessorFn: (r) => r.name, cell: (c) => <To kind={`${c.row.original.type}s` as "roles"} name={c.row.original.name} /> },
    { id: "type", header: "Type", accessorFn: (r) => r.type, cell: (c) => ({ role: "Role", user: "User", group: "User group" })[c.row.original.type] },
  ];
  const err = detachRole.error ?? detachUser.error ?? detachGroup.error;
  return (
    <Card title={`Entities attached (${rows.length})`} lede="The users, groups and roles this policy is attached to." actions={attach} testId="entities-card">
      <SelectableTable aria-label="Entities" data={rows} columns={columns} rowId={(r) => `${r.type}/${r.name}`} selected={chosen} onSelectedChange={setChosen} noun="entity" selectedLabel={(c) => `${c} selected`} hint="Choose entities to detach the policy from them." maxHeight={320} actions={(rs) => <Button type="button" variant="outline" size="sm" className="border-bad/40 bg-card text-bad hover:text-bad" onClick={() => { for (const r of rs) (r.type === "role" ? detachRole : r.type === "user" ? detachUser : detachGroup).mutate({ [r.type === "role" ? "RoleName" : r.type === "user" ? "UserName" : "GroupName"]: r.name, PolicyArn: arn }); setChosen(new Set()); }} data-testid="detach-entities">Detach</Button>} />
      {err && <Alert tone="bad" title="The policy was not detached" testId="detach-refused">{err.message}</Alert>}
    </Card>
  );
}

export function TagsTab({ arn }: { arn: string }) {
  return <KitTags target={{ service: "iam", kind: "policies", name: arn }} noun="tag" explain="Tags are key-value labels on the policy." />;
}

export function VersionsTab({ arn, versions, onNew }: { arn: string; versions: Versions["Versions"]; onNew: () => void }) {
  const [chosen, setChosen] = useState<ReadonlySet<string>>(new Set());
  const setDefault = useWrite(POLICIES, "SetDefaultPolicyVersion");
  const remove = useWrite(POLICIES, "DeletePolicyVersion");
  const create = <Button type="button" onClick={onNew} data-testid="version-new"><GitBranchIcon /> Create version</Button>;
  if (versions.length === 0) return <EmptyTab icon={FileTextIcon} title="No versions" action={create} testId="versions-none">IAM answered no version for this policy.</EmptyTab>;
  const columns: DataColumn<Versions["Versions"][number]>[] = [
    { id: "version", header: "Version", size: 160, accessorFn: (r) => r.VersionId, cell: (c) => <span className="font-mono text-[13px] font-medium text-bright">{c.row.original.VersionId}</span> },
    { id: "default", header: "Default", size: 160, accessorFn: (r) => Number(r.IsDefaultVersion), cell: (c) => (c.row.original.IsDefaultVersion ? "Default" : "") },
    { id: "created", header: "Created", accessorFn: (r) => r.CreateDate, cell: (c) => dateTimeUtc(c.row.original.CreateDate * 1000) },
  ];
  const err = setDefault.error ?? remove.error;
  return (
    <Card title={`Policy versions (${versions.length})`} lede="A customer managed policy keeps up to five versions; one is the default, the one that applies." actions={create} testId="versions-card">
      <SelectableTable aria-label="Policy versions" data={versions} columns={columns} rowId={(r) => r.VersionId} selected={chosen} onSelectedChange={setChosen} noun="version" selectedLabel={(c) => `${c} selected`} hint="Choose a version to make it the default or delete it." maxHeight={320}
        actions={(rs) => <>
          <Button type="button" variant="outline" size="sm" className="bg-card" disabled={rs.length !== 1 || rs[0]!.IsDefaultVersion} onClick={() => { setDefault.mutate({ PolicyArn: arn, VersionId: rs[0]!.VersionId }); setChosen(new Set()); }} data-testid="version-default">Set as default</Button>
          <Button type="button" variant="outline" size="sm" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={rs.some((r) => r.IsDefaultVersion)} title={rs.some((r) => r.IsDefaultVersion) ? "The default version cannot be deleted" : undefined} onClick={() => { for (const r of rs) remove.mutate({ PolicyArn: arn, VersionId: r.VersionId }); setChosen(new Set()); }} data-testid="version-delete">Delete</Button>
        </>} />
      {err && <Alert tone="bad" title="IAM refused the change to the versions" testId="versions-refused">{err.message}</Alert>}
      <Note>Changing the default version changes what the policy allows, at once, for everything it is attached to.</Note>
    </Card>
  );
}

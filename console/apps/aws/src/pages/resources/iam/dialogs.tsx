// The small dialogs over an IAM page that change what is attached or joined: attach policies to a user, group or role, add a user to
// groups, add users to a group. Each makes one call per choice, as the console, and says what IAM refused.
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { type DataColumn } from "@doze/ui/components/app/data-table";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { ApiFailure } from "@doze/ui/lib/api";
import { useResourceList } from "../../../api/resources";
import { callKindOp, type Kind } from "../../../kit";
import { GROUPS, ROLES, USERS, useRefresh } from "./api";
import { PolicyChooser } from "./policy-chooser";

type Call = { kind: Kind; operation: string; input: object };

/** A dialog that makes `calls` when its button is pressed, one after the other, and stays open with IAM's refusal when one fails. */
export function ActionDialog({ open, onOpenChange, title, description, children, submit, calls, disabled, onDone, testId = "action-dialog" }: { open: boolean; onOpenChange: (open: boolean) => void; title: string; description?: React.ReactNode; children: React.ReactNode; submit: string; calls: Call[]; disabled?: boolean; onDone?: () => void; testId?: string }) {
  const refresh = useRefresh();
  const [pending, setPending] = useState(false);
  const [failed, setFailed] = useState<{ operation: string; code: string; message: string } | null>(null);
  const run = async () => {
    setPending(true);
    setFailed(null);
    let at = "";
    try {
      for (const c of calls) {
        at = c.operation;
        await callKindOp(c.kind, c.operation, c.input);
      }
      refresh();
      onOpenChange(false);
      onDone?.();
    } catch (e) {
      setFailed({ operation: at, code: e instanceof ApiFailure ? e.code : "Error", message: e instanceof Error ? e.message : "No reason was given." });
    } finally {
      setPending(false);
    }
  };
  return (
    <Dialog open={open} onOpenChange={(o) => { if (!o) setFailed(null); onOpenChange(o); }}>
      <DialogContent className="sm:max-w-[680px]" data-testid={testId}>
        <DialogHeader>
          <DialogTitle className="text-[21px] font-semibold">{title}</DialogTitle>
          <DialogDescription className="text-[14px] leading-[22px] text-foreground">{description}</DialogDescription>
        </DialogHeader>
        {children}
        {failed && <Alert tone="bad" title={`${failed.operation} was refused: ${failed.code}`} testId={`${testId}-refused`}>{failed.message}</Alert>}
        <DialogFooter>
          <Button type="button" variant="outline" className="bg-card" onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button type="button" disabled={disabled || pending || calls.length === 0} onClick={() => void run()} data-testid={`${testId}-submit`}>{pending ? "Working…" : submit}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

const ATTACH: Record<"users" | "groups" | "roles", { kind: Kind; operation: string; member: string; noun: string }> = {
  users: { kind: USERS, operation: "AttachUserPolicy", member: "UserName", noun: "user" },
  groups: { kind: GROUPS, operation: "AttachGroupPolicy", member: "GroupName", noun: "group" },
  roles: { kind: ROLES, operation: "AttachRolePolicy", member: "RoleName", noun: "role" },
};

/** Attach policies to a user, group or role: the policies chosen, minus what is attached already. */
export function AttachPolicies({ on, name, attached, open, onOpenChange }: { on: "users" | "groups" | "roles"; name: string; attached: readonly string[]; open: boolean; onOpenChange: (open: boolean) => void }) {
  const a = ATTACH[on];
  const [chosen, setChosen] = useState<ReadonlySet<string>>(new Set());
  const exclude = useMemo(() => new Set(attached), [attached]);
  return (
    <ActionDialog open={open} onOpenChange={(o) => { if (!o) setChosen(new Set()); onOpenChange(o); }} title="Add permissions" description={<>Choose the policies <span className="font-mono font-semibold text-bright">{name}</span> is given. {a.operation} is made once for each.</>} submit={`Attach ${chosen.size === 1 ? "policy" : "policies"}`} calls={[...chosen].map((PolicyArn) => ({ kind: a.kind, operation: a.operation, input: { [a.member]: name, PolicyArn } }))} testId="attach-dialog">
      <PolicyChooser value={chosen} onChange={setChosen} exclude={exclude} filter entities />
    </ActionDialog>
  );
}

type NameRow = { name: string };
const nameColumn: DataColumn<NameRow> = { id: "name", header: "Name", accessorFn: (r) => r.name, cell: (c) => <span className="font-mono text-[13px] font-medium text-bright">{c.row.original.name}</span> };

/** Add a user to groups (from the user's page). */
export function AddToGroups({ user, member, open, onOpenChange }: { user: string; member: readonly string[]; open: boolean; onOpenChange: (open: boolean) => void }) {
  const groups = useResourceList("iam", "groups");
  const [chosen, setChosen] = useState<ReadonlySet<string>>(new Set());
  const rows = (groups.data?.pages.flatMap((p) => p.rows) ?? []).filter((r) => !member.includes(r.name)).map((r) => ({ name: r.name }));
  return (
    <ActionDialog open={open} onOpenChange={(o) => { if (!o) setChosen(new Set()); onOpenChange(o); }} title="Add user to groups" description={<><span className="font-mono font-semibold text-bright">{user}</span> receives the permissions of each group it joins.</>} submit="Add to groups" calls={[...chosen].map((GroupName) => ({ kind: USERS, operation: "AddUserToGroup", input: { UserName: user, GroupName } }))} testId="groups-dialog">
      {rows.length === 0 ? <p className="text-[13.5px] text-muted-foreground" data-testid="nothing-to-add">{user} is in every group there is.</p> : <SelectableTable aria-label="Groups" data={rows} columns={[{ ...nameColumn, header: "Group name" }]} rowId={(r) => r.name} selected={chosen} onSelectedChange={setChosen} noun="group" selectedLabel={(c) => `${c} selected`} hint="Select one or more groups." maxHeight={280} />}
    </ActionDialog>
  );
}

/** Add users to a group (from the group's page). */
export function AddUsers({ group, member, open, onOpenChange }: { group: string; member: readonly string[]; open: boolean; onOpenChange: (open: boolean) => void }) {
  const users = useResourceList("iam", "users");
  const [chosen, setChosen] = useState<ReadonlySet<string>>(new Set());
  const rows = (users.data?.pages.flatMap((p) => p.rows) ?? []).filter((r) => !member.includes(r.name)).map((r) => ({ name: r.name }));
  return (
    <ActionDialog open={open} onOpenChange={(o) => { if (!o) setChosen(new Set()); onOpenChange(o); }} title="Add users to group" description={<>Each user added gets the permissions of <span className="font-mono font-semibold text-bright">{group}</span>.</>} submit="Add users" calls={[...chosen].map((UserName) => ({ kind: GROUPS, operation: "AddUserToGroup", input: { UserName, GroupName: group } }))} testId="users-dialog">
      {rows.length === 0 ? <p className="text-[13.5px] text-muted-foreground" data-testid="nothing-to-add">Every user there is belongs to {group} already.</p> : <SelectableTable aria-label="Users" data={rows} columns={[{ ...nameColumn, header: "User name" }]} rowId={(r) => r.name} selected={chosen} onSelectedChange={setChosen} noun="user" selectedLabel={(c) => `${c} selected`} hint="Select one or more users." maxHeight={280} />}
    </ActionDialog>
  );
}



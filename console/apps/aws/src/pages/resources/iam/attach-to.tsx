// Attach a policy to a user, group or role (the policy's Actions menu): choose which kind and which one, and the policy is attached with
// AttachUserPolicy, AttachGroupPolicy or AttachRolePolicy.
import { useState } from "react";
import { Field, ResourcePicker } from "../../../kit";
import { GROUPS, ROLES, USERS } from "./api";
import { ActionDialog } from "./dialogs";
import { Segmented } from "./parts";

const TARGETS = { users: { kind: USERS, op: "AttachUserPolicy", member: "UserName", noun: "user" }, groups: { kind: GROUPS, op: "AttachGroupPolicy", member: "GroupName", noun: "group" }, roles: { kind: ROLES, op: "AttachRolePolicy", member: "RoleName", noun: "role" } } as const;

export function AttachTo({ arn, label, open, onOpenChange }: { arn: string; label: string; open: boolean; onOpenChange: (open: boolean) => void }) {
  const [on, setOn] = useState<"users" | "groups" | "roles">("roles");
  const [name, setName] = useState<string | null>(null);
  const t = TARGETS[on];
  return (
    <ActionDialog open={open} onOpenChange={(o) => { if (!o) setName(null); onOpenChange(o); }} title="Attach policy" description={<>Attach <span className="font-mono font-semibold text-bright">{label}</span> to a user, group or role. {t.op} is made once.</>} submit="Attach policy" disabled={!name} calls={name ? [{ kind: t.kind, operation: t.op, input: { [t.member]: name, PolicyArn: arn } }] : []} testId="attach-to-dialog">
      <Segmented value={on} onChange={(v) => { if (v) { setOn(v); setName(null); } }} label="Attach to" options={[{ value: "roles", label: "Role" }, { value: "users", label: "User" }, { value: "groups", label: "Group" }]} testId="attach-kind" />
      <Field label={`Choose a ${t.noun}`}>{(id) => <ResourcePicker service="iam" kind={on} noun={t.noun} by="name" id={id} value={name} onChange={setName} />}</Field>
    </ActionDialog>
  );
}

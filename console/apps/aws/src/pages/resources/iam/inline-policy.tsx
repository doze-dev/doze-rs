// Create inline policy (a role's or a user's Add permissions menu): a name and a policy document, put on the role with PutRolePolicy or on the
// user with PutUserPolicy. An inline policy is embedded in its role or user and deleted with it.
import { useState } from "react";
import { Input } from "@doze/ui/components/ui/input";
import { Field } from "../../../kit";
import { ROLES, USERS } from "./api";
import { ActionDialog } from "./dialogs";
import { nameProblem, starterPolicy } from "./model";
import { PolicyJson, policyErrors } from "./policy-json";

export function InlinePolicy({ role, user, open, onOpenChange }: { role?: string; user?: string; open: boolean; onOpenChange: (open: boolean) => void }) {
  const on = role ?? user ?? "";
  const call = user !== undefined ? { kind: USERS, operation: "PutUserPolicy", member: "UserName" } : { kind: ROLES, operation: "PutRolePolicy", member: "RoleName" };
  const [name, setName] = useState("");
  const [doc, setDoc] = useState(() => starterPolicy());
  const problem = nameProblem(name, "policy");
  const invalid = problem !== null || policyErrors(doc, "identity").length > 0;
  return (
    <ActionDialog open={open} onOpenChange={onOpenChange} title="Create inline policy" description={<>An inline policy is embedded in <span className="font-mono font-semibold text-bright">{on}</span> and deleted with it. {call.operation} is made once.</>} submit="Create policy" disabled={invalid} calls={[{ kind: call.kind, operation: call.operation, input: { [call.member]: on, PolicyName: name, PolicyDocument: doc } }]} testId="inline-dialog">
      <Field label="Policy name" error={name === "" ? undefined : (problem ?? undefined)}>{(id) => <Input id={id} value={name} onChange={(e) => setName(e.target.value)} placeholder="read-reports" autoComplete="off" className="bg-card font-mono" data-testid="inline-name" />}</Field>
      <PolicyJson value={doc} onChange={setDoc} kind="identity" label="Policy document" rows={10} testId="inline-json" />
    </ActionDialog>
  );
}

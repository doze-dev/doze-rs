// Create inline policy (a role's Add permissions menu): a name and a policy document, put on the role with PutRolePolicy. An inline policy is
// embedded in the role and deleted with it.
import { useState } from "react";
import { Input } from "@doze/ui/components/ui/input";
import { Field } from "../../../kit";
import { ROLES } from "./api";
import { ActionDialog } from "./dialogs";
import { nameProblem, starterPolicy } from "./model";
import { PolicyJson, policyErrors } from "./policy-json";

export function InlinePolicy({ role, open, onOpenChange }: { role: string; open: boolean; onOpenChange: (open: boolean) => void }) {
  const [name, setName] = useState("");
  const [doc, setDoc] = useState(() => starterPolicy());
  const problem = nameProblem(name, "policy");
  const invalid = problem !== null || policyErrors(doc, "identity").length > 0;
  return (
    <ActionDialog open={open} onOpenChange={onOpenChange} title="Create inline policy" description={<>An inline policy is embedded in <span className="font-mono font-semibold text-bright">{role}</span> and deleted with it. PutRolePolicy is made once.</>} submit="Create policy" disabled={invalid} calls={[{ kind: ROLES, operation: "PutRolePolicy", input: { RoleName: role, PolicyName: name, PolicyDocument: doc } }]} testId="inline-dialog">
      <Field label="Policy name" error={name === "" ? undefined : (problem ?? undefined)}>{(id) => <Input id={id} value={name} onChange={(e) => setName(e.target.value)} placeholder="read-reports" autoComplete="off" className="bg-card font-mono" data-testid="inline-name" />}</Field>
      <PolicyJson value={doc} onChange={setDoc} kind="identity" label="Policy document" rows={10} testId="inline-json" />
    </ActionDialog>
  );
}

// Create key, steps 3 to 5 (kms__key-create-admins, key-create-users, key-create-policy): who administers the key, who uses it, and the
// key policy those choices write, which can be edited before the key is made.
import { PencilIcon } from "lucide-react";
import { useMemo } from "react";
import { type DataColumn } from "@doze/ui/components/app/data-table";
import { CodeBlock } from "@doze/ui/components/app/code-block";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { Button } from "@doze/ui/components/ui/button";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Label } from "@doze/ui/components/ui/label";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { FormSection, PolicyEditor } from "../../../kit";
import { useAbout } from "../../../api/queries";
import { useAllRows } from "../cfg-api";
import { cell, Note } from "../cfg-parts";
import { policyOf, type KeyForm } from "./create-model";
import { buildPolicy, SIDS, statementWords } from "./model";

export type Principal = { name: string; arn: string };

/** The IAM users and roles of the account, as `user/ci-bot` and `role/larder-admin`, each with the ARN a key policy names. */
export function usePrincipals(): { principals: Principal[]; loading: boolean; served: boolean } {
  const about = useAbout().data;
  const served = about?.services.some((s) => s.name === "iam" && s.served) ?? false;
  const users = useAllRows("iam", "users", { enabled: served });
  const roles = useAllRows("iam", "roles", { enabled: served });
  const account = about?.account ?? "";
  const principals = useMemo(() => {
    const one = (kind: "user" | "role", rows: typeof users) => rows.rows.map((r) => ({ name: `${kind}/${r.name}`, arn: `arn:aws:iam::${account}:${kind}${cell(rows.columns, r, "Path") ?? "/"}${r.name}` }));
    return [...one("user", users), ...one("role", roles)];
  }, [users, roles, account]);
  return { principals, loading: served && (users.loading || roles.loading), served };
}

function PrincipalTable({ principals, loading, served, chosen, onChange, noun }: { principals: Principal[]; loading: boolean; served: boolean; chosen: string[]; onChange: (arns: string[]) => void; noun: string }) {
  const columns: DataColumn<Principal>[] = [{ id: "name", header: "Name", accessorKey: "name", meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px] font-medium text-bright">{c.row.original.name}</span> }];
  if (loading) return <Skeleton className="h-40 rounded-xl" data-testid="principals-loading" />;
  if (!served) return <Note testId="principals-none">IAM is not running in this doze-aws, so there are no users or roles to choose. Write the principals into the key policy on the policy step.</Note>;
  return (
    <SelectableTable aria-label={`IAM users and roles that ${noun}`} noun="principal" data={principals} columns={columns} rowId={(p) => p.arn} selected={new Set(chosen)} onSelectedChange={(s) => onChange(principals.filter((p) => s.has(p.arn)).map((p) => p.arn))} hint="Select the users and roles." maxHeight={360} selectedLabel={(n) => `${n} selected`} />
  );
}

const statementOf = (f: KeyForm, account: string, sid: string): string[] =>
  (buildPolicy({ account, admins: f.admins, canDelete: f.canDelete, users: f.users, usage: f.usage }).Statement ?? []).filter((s) => s.Sid === sid).map((s) => JSON.stringify(s, null, 2));

export function StepAdmins({ f, set, account }: { f: KeyForm; set: (p: Partial<KeyForm>) => void; account: string }) {
  const p = usePrincipals();
  const statement = statementOf(f, account, SIDS.admins);
  return (
    <>
      <FormSection title="Key administrators" description="The AWS account keeps full control through the first statement of the key policy, so IAM policies can still grant access." testId="section-admins">
        <PrincipalTable {...p} chosen={f.admins} onChange={(admins) => set({ admins })} noun="administer the key" />
        <Note>Administrators manage the key: enable and disable it, change its key policy, turn on rotation. <span className="text-muted-foreground">Because they can change the key policy they can give themselves more permission. Roles, which have temporary credentials, are preferred over IAM users with long-term credentials.</span></Note>
      </FormSection>
      <FormSection title="Key deletion" testId="section-deletion">
        <div className="flex items-start gap-3">
          <Checkbox id="can-delete" checked={f.canDelete} onCheckedChange={(v) => set({ canDelete: v === true })} className="mt-0.5" data-testid="can-delete" />
          <Label htmlFor="can-delete" className="grid gap-0.5 font-normal">
            <span className="font-medium text-bright">Allow key administrators to delete this key</span>
            <span className="text-[13px] text-muted-foreground">Clear it to remove ScheduleKeyDeletion and CancelKeyDeletion from the administrators statement.</span>
          </Label>
        </div>
      </FormSection>
      <FormSection title="For reference: the administrators statement" description="The Developer Guide shows the console adding administrators under the Sid below. Editing this Sid by hand can change how the console shows the statement later." testId="section-admins-statement">
        {statement.length > 0 ? <CodeBlock text={statement[0]!} testId="admins-statement" /> : <Note testId="admins-statement-none">Choose an administrator to see the statement that names them.</Note>}
      </FormSection>
    </>
  );
}

export function StepUsers({ f, set, account }: { f: KeyForm; set: (p: Partial<KeyForm>) => void; account: string }) {
  const p = usePrincipals();
  const statements = [...statementOf(f, account, SIDS.use), ...statementOf(f, account, SIDS.grants)];
  return (
    <>
      <FormSection title="This account" description={`Account ${account}. Select the IAM users and roles that can use the key in cryptographic operations, such as encrypting, decrypting, re-encrypting and generating data keys.`} testId="section-users">
        <PrincipalTable {...p} chosen={f.users} onChange={(users) => set({ users })} noun="use the key" />
        {f.usage === "ENCRYPT_DECRYPT" && <Note>The console also adds a second statement that lets AWS services create grants for a key user. doze-aws keeps and lists grants, but does not evaluate them.</Note>}
      </FormSection>
      <FormSection title="Other AWS accounts" description="Letting principals of another account use this key." testId="section-accounts">
        <div><Button type="button" variant="outline" disabled className="bg-card">Add another AWS account</Button></div>
        <Note>Not available in doze-aws: it serves one account, so there is no other account to name. To let a principal outside it use the key, write its ARN into the key policy on the next steps.</Note>
      </FormSection>
      <FormSection title="For reference: the key users statements" description="The Developer Guide shows the console adding the statements below for key users: one for use of the key, one for grants made by AWS services." testId="section-users-statement">
        {statements.length > 0 ? <CodeBlock text={statements.join(",\n")} testId="users-statement" /> : <Note testId="users-statement-none">Choose a key user to see the statements that name them.</Note>}
      </FormSection>
    </>
  );
}

export function StepPolicy({ f, set, account, editing, onEdit }: { f: KeyForm; set: (p: Partial<KeyForm>) => void; account: string; editing: boolean; onEdit: (on: boolean) => void }) {
  const text = f.policy ?? policyOf(f, account);
  const sids = (JSON.parse(text) as { Statement?: { Sid?: string }[] }).Statement?.map((s) => s.Sid).filter(Boolean) as string[] | undefined;
  return (
    <FormSection
      title="Key policy"
      description={f.policy !== null ? "The policy as you edited it. It is sent as it is." : `${statementWords(text).replace(/^\d+/, (n) => ({ 1: "One", 2: "Two", 3: "Three", 4: "Four" })[Number(n)] ?? n)}: ${(sids ?? []).join(", ")}.`}
      testId="section-policy"
      actions={<Button type="button" variant="outline" className="bg-card" onClick={() => onEdit(!editing)} data-testid="policy-edit-toggle"><PencilIcon /> {editing ? "Done" : "Edit"}</Button>}
    >
      {editing ? <PolicyEditor kind="resource" value={text} onChange={(policy) => set({ policy })} label="Key policy" rows={20} help="Up to 32,768 characters. doze-aws checks that it is a JSON object." testId="policy-editor" /> : <CodeBlock text={text} max="max-h-[34rem]" testId="policy-json" />}
      <Note>The first statement is what lets IAM policies in this account grant access to the key. Without it, IAM policies that allow the key have no effect, and the key can become unmanageable if the only named administrator is deleted.</Note>
    </FormSection>
  );
}

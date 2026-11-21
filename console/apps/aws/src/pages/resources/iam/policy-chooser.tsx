// The table of policies a wizard chooses from (the permissions of a role, a group or a user): the customer managed policies IAM lists
// and the AWS managed ones, with their type and, when the board shows it, what each is attached to. One or more are chosen with their
// checkboxes. IAM lists an AWS managed policy once something has it attached; the well-known names beyond those are ones doze-aws makes a
// document for when they are attached by ARN, so they are offered here as the board offers them.
import { useMemo, useState } from "react";
import { type DataColumn } from "@doze/ui/components/app/data-table";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { POLICIES, useRead } from "./api";
import { SearchBox } from "./list-parts";
import { awsArn, isAwsManaged, managedKind, WELL_KNOWN } from "./model";
import { Segmented } from "./parts";

export type PolicyOption = { arn: string; name: string; aws: boolean; attached: number; description: string };
type Listed = { Policies: { PolicyName: string; Arn: string; AttachmentCount?: number; Description?: string }[] };

/** The policies a person may attach: customer managed ones, then AWS managed ones (listed or well known). */
export function usePolicyOptions(): { options: PolicyOption[]; loading: boolean; error: unknown } {
  const local = useRead<Listed>(POLICIES, "ListPolicies", { Scope: "Local", MaxItems: 100 });
  const aws = useRead<Listed>(POLICIES, "ListPolicies", { Scope: "AWS", MaxItems: 100 });
  const options = useMemo(() => {
    const mine = (local.data?.Policies ?? []).map((p) => ({ arn: p.Arn, name: p.PolicyName, aws: false, attached: p.AttachmentCount ?? 0, description: p.Description ?? "" }));
    const listed = (aws.data?.Policies ?? []).map((p) => ({ arn: p.Arn, name: p.PolicyName, aws: true, attached: p.AttachmentCount ?? 0, description: p.Description ?? "" }));
    const known = WELL_KNOWN.filter((n) => !listed.some((l) => l.name === n)).map((n) => ({ arn: awsArn(n), name: n, aws: true, attached: 0, description: "" }));
    return [...listed, ...known, ...mine];
  }, [local.data, aws.data]);
  return { options, loading: local.isPending, error: local.error };
}

/** The table. `value` is the chosen ARNs; `entities` adds the attached-entities column and `filter` the AWS managed / Customer managed switch and the search. */
export function PolicyChooser({ value, onChange, entities = false, filter = false, label = "policy", footnote, exclude }: { value: ReadonlySet<string>; onChange: (next: Set<string>) => void; entities?: boolean; filter?: boolean; label?: string; footnote?: React.ReactNode; /** ARNs not offered (what is attached already). */ exclude?: ReadonlySet<string> }) {
  const { options, loading } = usePolicyOptions();
  const [scope, setScope] = useState<"" | "aws" | "local">("");
  const [typed, setTyped] = useState("");
  if (loading) return <Skeleton className="h-40 w-full rounded-xl" data-testid="policy-chooser-loading" />;
  const rows = options.filter((o) => !exclude?.has(o.arn)).filter((o) => (scope === "" || (scope === "aws") === o.aws) && o.name.toLowerCase().includes(typed.trim().toLowerCase()));
  const columns: DataColumn<PolicyOption>[] = [
    { id: "name", header: "Policy name", size: 360, min: 220, accessorFn: (r) => r.name, cell: (c) => <span className="font-mono text-[13px] font-medium text-bright">{c.row.original.name}</span> },
    { id: "type", header: "Type", size: 200, accessorFn: (r) => managedKind(r.arn), cell: (c) => managedKind(c.row.original.arn) },
    ...(entities ? [{ id: "entities", header: "Attached entities", size: 220, accessorFn: (r: PolicyOption) => r.attached, meta: { numeric: true }, cell: (c: { row: { original: PolicyOption } }) => c.row.original.attached }] : []),
  ];
  return (
    <div className="grid gap-3" data-testid="policy-chooser">
      <SelectableTable
        aria-label="Permissions policies"
        data={rows}
        columns={columns}
        rowId={(r) => r.arn}
        selected={value}
        onSelectedChange={onChange}
        noun={label}
        selectedLabel={(c) => `${c} selected`}
        toolbar={filter ? <><SearchBox value={typed} onChange={setTyped} noun="policies" /><Segmented value={scope} onChange={setScope} label="Policy type" options={[{ value: "aws", label: "AWS managed" }, { value: "local", label: "Customer managed" }]} testId="policy-scope" /></> : undefined}
        hint="Select one or more policies."
        maxHeight={360}
        empty={typed ? `No policy's name contains “${typed}”.` : "There are no policies of this type."}
      />
      {footnote}
    </div>
  );
}

export { isAwsManaged };

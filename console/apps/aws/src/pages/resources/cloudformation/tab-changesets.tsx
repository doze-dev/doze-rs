// A stack's Change sets tab (cloudformation__stack-changesets, changesets-none): each set links to its own page.
import { Link } from "@tanstack/react-router";
import { GitCompareIcon, PlusIcon } from "lucide-react";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { Button } from "@doze/ui/components/ui/button";
import { DetailBadgeView, RuleHelp } from "../../../kit";
import { at, Loaded } from "../deep/parts";
import { useCfnRead, useStalePreviews } from "./api";
import { Section, Status } from "./parts";

export type ChangeSetRow = { ChangeSetId?: string; ChangeSetName: string; Status?: string; ExecutionStatus?: string; CreationTime?: number | string; StatusReason?: string };

export function ChangeSetsTab({ name, makeOne, updatable }: { name: string; makeOne: () => void; updatable: boolean }) {
  const q = useCfnRead<{ Summaries?: ChangeSetRow[] }>("ListChangeSets", { StackName: name });
  const stale = useStalePreviews(name, q.data?.Summaries?.map((s) => s.ChangeSetName));
  const make = <Button variant="outline" size="sm" className="bg-card" onClick={makeOne} disabled={!updatable} title={updatable ? undefined : "The stack cannot be updated in this status"} data-testid="changeset-create"><PlusIcon /> Create change set</Button>;
  const columns: DataColumn<ChangeSetRow>[] = [
    { id: "n", header: "Name", size: 230, accessorKey: "ChangeSetName", cell: (c) => <Link to="/resources/$service/$kind/$name" params={{ service: "cloudformation", kind: "stacks", name }} search={{ do: "changeset", name: c.row.original.ChangeSetName }} className="rounded-sm font-mono text-[13px] font-medium text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" data-testid="changeset-link">{c.row.original.ChangeSetName}</Link> },
    { id: "s", header: "Status", size: 200, accessorFn: (r) => r.Status ?? "", cell: (c) => <Status status={c.row.original.Status ?? ""} /> },
    { id: "e", header: "Execution status", size: 180, accessorFn: (r) => r.ExecutionStatus ?? "", cell: (c) => (c.row.original.ExecutionStatus ? <DetailBadgeView badge={{ text: c.row.original.ExecutionStatus, tone: c.row.original.ExecutionStatus === "AVAILABLE" || c.row.original.ExecutionStatus === "EXECUTE_COMPLETE" ? "good" : c.row.original.ExecutionStatus === "EXECUTE_FAILED" ? "bad" : "neutral" }} /> : "–") },
    { id: "t", header: "Creation time", size: 220, accessorFn: (r) => String(r.CreationTime ?? ""), cell: (c) => at(c.row.original.CreationTime) },
    { id: "r", header: "Status reason", accessorFn: (r) => r.StatusReason ?? "", cell: (c) => <span className="text-[13px] whitespace-normal">{c.row.original.StatusReason || <span className="text-muted-foreground">–</span>}</span> },
  ];
  return (
    <Loaded q={q} what="The change sets">
      {(p) => {
        const sets = (p.Summaries ?? []).filter((s) => !stale.includes(s.ChangeSetName));
        if (sets.length === 0) return <EmptyTab icon={GitCompareIcon} title="No change sets" action={make} testId="changesets-none">No change set has been created for this stack. A change set previews an update before you run it.</EmptyTab>;
        return (
          <Section title="Change sets" lede="A change set previews what an update would do to the running resources. Nothing changes until you execute one." actions={make} testId="stack-changesets">
            <DataTable aria-label="Change sets" data={sets} columns={columns} rowId={(r) => r.ChangeSetId ?? r.ChangeSetName} rowHeight={56} maxHeight={480} />
            {sets.some((s) => s.Status === "FAILED") && <RuleHelp className="max-w-[56rem]">A change set with status FAILED can't be executed, and its execution status stays UNAVAILABLE; fix what its status reason says and create another. An empty change set fails with exactly the status reason shown, which is how a deploy tool learns there is nothing to do.</RuleHelp>}
          </Section>
        );
      }}
    </Loaded>
  );
}

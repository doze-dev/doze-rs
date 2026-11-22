// What a change set shows wherever it is read: the page of one (cloudformation__changeset-detail), the stack waiting for review
// (stack-review-pending), and the preview of an update's review step (stack-update-review).
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { StatusDot, type Tone } from "@doze/ui/components/app/status-dot";
import { RuleHelp } from "../../../kit";
import { at } from "../deep/parts";
import { ResourceTo } from "../value";
import { changeRows } from "./change-rows";
import { resourceTarget } from "./model";
import { Facts, Section, Status } from "./parts";

export type Change = { ResourceChange?: { Action: string; LogicalResourceId: string; PhysicalResourceId?: string; ResourceType: string; Replacement?: string } };

export type ChangeSet = {
  ChangeSetName: string;
  ChangeSetId?: string;
  StackId?: string;
  StackName?: string;
  Status: string;
  ExecutionStatus?: string;
  StatusReason?: string;
  CreationTime?: number | string;
  Parameters?: { ParameterKey: string; ParameterValue?: string }[];
  Changes?: Change[];
};

const ACTION: Record<string, Tone> = { Add: "ok", Modify: "info", Remove: "bad", Import: "info", Dynamic: "warn" };

const mono = "font-mono text-[13px]";

export { changeRows };

export function ChangesTable({ set, withPhysical = true, lede }: { set: ChangeSet; withPhysical?: boolean; lede?: React.ReactNode }) {
  const rows = changeRows(set);
  const columns: DataColumn<(typeof rows)[number]>[] = [
    { id: "a", header: "Action", size: 130, accessorKey: "Action", cell: (c) => <StatusDot tone={ACTION[c.row.original.Action] ?? "idle"}>{c.row.original.Action}</StatusDot> },
    { id: "l", header: "Logical ID", size: 260, accessorKey: "LogicalResourceId", cell: (c) => <span className={`${mono} font-medium text-bright`}>{c.row.original.LogicalResourceId}</span> },
    ...(withPhysical ? [{ id: "p", header: "Physical ID", size: 260, accessorFn: (r: (typeof rows)[number]) => r.PhysicalResourceId ?? "", cell: (c: { row: { original: (typeof rows)[number] } }) => { const r = c.row.original; const to = resourceTarget(r); return !r.PhysicalResourceId ? <span className="text-muted-foreground">–</span> : to ? <ResourceTo to={to} className={`${mono} break-all whitespace-normal`}>{r.PhysicalResourceId}</ResourceTo> : <span className={mono}>{r.PhysicalResourceId}</span>; } } as DataColumn<(typeof rows)[number]>] : []),
    { id: "t", header: "Resource type", size: 240, accessorKey: "ResourceType", cell: (c) => <span className={mono}>{c.row.original.ResourceType}</span> },
    { id: "r", header: "Replacement", accessorFn: (r) => r.Replacement ?? "", cell: (c) => c.row.original.Replacement ?? <span className="text-muted-foreground">–</span> },
  ];
  return (
    <Section title="Changes" lede={lede ?? "Each row is a resource CloudFormation would add, modify or remove."} testId="changes">
      <DataTable aria-label="Changes" data={rows} columns={columns} rowId={(r) => r.LogicalResourceId} rowHeight={52} maxHeight={420} empty="This change set has no changes." />
    </Section>
  );
}

/** The change set's own facts. `stack`: a link to its stack is wanted (the page of the set); the review of a stack leaves it out. */
export function ChangeSetFacts({ set, title = "Overview", stack }: { set: ChangeSet; title?: string; stack?: React.ReactNode }) {
  return (
    <Facts
      title={title}
      testId="changeset-overview"
      wide={set.ChangeSetId && stack ? [{ label: "Change set ID", value: set.ChangeSetId, mono: true }] : []}
      rows={[
        ...(stack ? [] : [{ label: "Name", value: set.ChangeSetName, mono: true }]),
        ...(stack ? [{ label: "Stack", value: stack }] : []),
        { label: "Status", value: <Status status={set.Status} /> },
        { label: "Execution status", value: set.ExecutionStatus ?? "–" },
        { label: "Creation time", value: at(set.CreationTime) },
        { label: "Status reason", value: set.StatusReason ?? "None" },
      ]}
    />
  );
}

export function ParametersCard({ set }: { set: ChangeSet }) {
  const params = set.Parameters ?? [];
  return (
    <section className="min-w-0 rounded-[14px] border bg-card p-6 lg:p-7" aria-label="Parameters" data-testid="changeset-parameters">
      <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">Parameters</h2>
      <p className="mt-1 text-[13.5px] text-muted-foreground">The inputs this change set was made with.</p>
      {params.length === 0 ? <p className="mt-4 text-[13.5px] text-muted-foreground">The template declares no parameters.</p> : (
        <dl className="mt-3 text-[13.5px]">{params.map((p) => <div key={p.ParameterKey} className="flex gap-6 border-b py-3"><dt className="w-40 shrink-0 text-muted-foreground">{p.ParameterKey}</dt><dd className={`min-w-0 flex-1 break-all ${mono}`}>{p.ParameterValue}</dd></div>)}</dl>
      )}
    </section>
  );
}

/** What executing the set does on doze-aws: from the crate (the work runs in the background; engine/run.rs), by the kind of the set. */
export function ExecutesCard({ creating, rows }: { creating: boolean; rows: { Action: string }[] }) {
  const modifies = rows.some((r) => r.Action === "Modify");
  return (
    <section className="min-w-0 rounded-[14px] border bg-card p-6 lg:p-7" aria-label="What executing does" data-testid="executes">
      <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">What executing does</h2>
      <p className="mt-1 text-[13.5px] text-muted-foreground">Read this before you choose Execute change set.</p>
      <RuleHelp className="mt-4">
        {creating ? "Executing builds the stack: it goes CREATE_IN_PROGRESS while its resources are made in the background, then CREATE_COMPLETE, or ROLLBACK_COMPLETE if a resource fails (a name that is already taken, for one)." : "Executing starts the update: the stack goes UPDATE_IN_PROGRESS while each change is applied in the background, then UPDATE_COMPLETE. If a resource fails, what the update made is undone and the stack ends UPDATE_ROLLBACK_COMPLETE."}{" "}
        The change set then shows execution status EXECUTE_COMPLETE.{modifies ? " A Modify row's Replacement says whether the resource is replaced (True), changed in place (False) or may be (Conditional), as far as the template says." : ""}
      </RuleHelp>
    </section>
  );
}

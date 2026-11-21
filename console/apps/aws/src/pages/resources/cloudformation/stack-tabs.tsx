// A stack's deep tabs: the sentence its status says, its events newest first with status colours and reasons
// (DescribeStackEvents), its template as deployed (GetTemplate) in a read-only code view, and its change sets (ListChangeSets).
import { useState } from "react";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { StatusDot, type Tone } from "@doze/ui/components/app/status-dot";
import { Button } from "@doze/ui/components/ui/button";
import { on, useOpRead } from "../deep/api";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { at, Card, CodeBlock, Loaded, Made, pretty } from "../deep/parts";

/** How a CloudFormation status is coloured: failed red, in progress blue, a rollback or a deletion amber, done green. */
export function stackTone(status: string): Tone {
  if (/FAILED$/.test(status)) return "bad";
  if (/IN_PROGRESS$/.test(status)) return "info";
  if (/ROLLBACK|^DELETE_COMPLETE$|SKIPPED/.test(status)) return "warn";
  if (/COMPLETE$/.test(status)) return "ok";
  return "idle";
}

/** One sentence on where the stack stands, from its status, its reason and its times. */
export function stackSentence(name: string, status: string, why: string, created: string | null, updated: string | null): string {
  const since = updated ? `last updated ${updated}` : created ? `created ${created} and not updated since` : "";
  const head = /IN_PROGRESS$/.test(status)
    ? `${name} is being worked on (${status}).`
    : /FAILED$/.test(status)
      ? `${name} is in ${status}: the last operation did not finish.`
      : /ROLLBACK_COMPLETE$/.test(status)
        ? `${name} was rolled back (${status}): it holds what it held before the operation that failed.`
        : `${name} is ${status}.`;
  return [head, since && `It was ${since}.`, why && `Reason: ${why}`].filter(Boolean).join(" ");
}

export function StackSentence({ d }: { d: ResourceDetail }) {
  const value = (label: string) => d.fields.find((f) => f.label === label)?.value ?? null;
  const status = value("Status") ?? "";
  const time = (label: string) => {
    const v = value(label);
    return v ? at(v) : null;
  };
  return (
    <div className="mb-6 flex flex-wrap items-start gap-3 rounded-[14px] border bg-card px-6 py-5" data-testid="stack-sentence">
      <StatusDot tone={stackTone(status)} className="mt-0.5">{status || "Unknown"}</StatusDot>
      <p className="min-w-0 flex-1 text-[14.5px] leading-[22px] text-foreground">{stackSentence(d.name, status, value("Why") ?? "", time("Created"), time("Updated"))}</p>
    </div>
  );
}

type StackEvent = { EventId: string; Timestamp: number | string; LogicalResourceId?: string; ResourceType?: string; ResourceStatus?: string; ResourceStatusReason?: string; PhysicalResourceId?: string };

const EVENT_COLUMNS: DataColumn<StackEvent>[] = [
  { id: "t", header: "Time (UTC)", size: 210, accessorFn: (r) => String(r.Timestamp), cell: (c) => <span className="text-[13px] tabular-nums">{at(c.row.original.Timestamp)}</span> },
  {
    id: "l",
    header: "Logical ID",
    size: 280,
    accessorFn: (r) => r.LogicalResourceId ?? "",
    cell: (c) => (
      <span className="grid leading-5">
        <span className="font-mono text-[13px] font-medium text-bright">{c.row.original.LogicalResourceId}</span>
        <span className="font-mono text-[12px] text-muted-foreground">{c.row.original.ResourceType}</span>
      </span>
    ),
  },
  { id: "s", header: "Status", size: 230, accessorFn: (r) => r.ResourceStatus ?? "", cell: (c) => <StatusDot tone={stackTone(c.row.original.ResourceStatus ?? "")}>{c.row.original.ResourceStatus}</StatusDot> },
  { id: "r", header: "Status reason", accessorFn: (r) => r.ResourceStatusReason ?? "", cell: (c) => <span className="text-[13px]" title={c.row.original.ResourceStatusReason}>{c.row.original.ResourceStatusReason || <span className="text-muted-foreground">–</span>}</span> },
];

export function StackEvents({ name }: { name: string }) {
  const [tokens, setTokens] = useState<string[]>([]);
  const token = tokens[tokens.length - 1];
  const q = useOpRead<{ StackEvents?: StackEvent[]; NextToken?: string }>(on.stack(name), "DescribeStackEvents", { StackName: name, ...(token ? { NextToken: token } : {}) });
  return (
    <Card title="Events" lede="Everything that happened to the stack and its resources, newest first, as CloudFormation wrote it." testId="stack-events">
      <Loaded q={q} what="The stack's events">
        {(p) => (
          <>
            <DataTable aria-label="Stack events" data={p.StackEvents ?? []} columns={EVENT_COLUMNS} rowId={(r) => r.EventId} rowHeight={60} maxHeight={640} empty="No events." />
            <div className="mt-3 flex justify-end gap-2">
              {tokens.length > 0 && <Button variant="outline" size="sm" className="bg-card" onClick={() => setTokens(tokens.slice(0, -1))}>Newer</Button>}
              {p.NextToken && <Button variant="outline" size="sm" className="bg-card" onClick={() => setTokens([...tokens, p.NextToken as string])}>Older</Button>}
            </div>
          </>
        )}
      </Loaded>
      <Made className="mt-3" ops={["DescribeStackEvents"]} />
    </Card>
  );
}

export function StackTemplate({ name }: { name: string }) {
  const [wrap, setWrap] = useState(false);
  const q = useOpRead<{ TemplateBody?: string; StagesAvailable?: string[] }>(on.stack(name), "GetTemplate", { StackName: name, TemplateStage: "Original" });
  const body = q.data?.TemplateBody ?? "";
  const json = body.trim().startsWith("{");
  return (
    <Card
      title="Template"
      lede={`The template the stack was last deployed with, as it was sent${json ? " (JSON, indented here)" : ""}. Read only.`}
      actions={
        body && (
          <>
            <Button variant="outline" size="sm" className="bg-card" onClick={() => setWrap(!wrap)} aria-pressed={wrap}>{wrap ? "No wrap" : "Wrap lines"}</Button>
            <CopyButton text={body} what="Template copied" className="bg-card" />
          </>
        )
      }
      testId="stack-template"
    >
      <Loaded q={q} what="The template">
        {() => <CodeBlock text={json ? pretty(body) : body} wrap={wrap} testId="template-body" max="max-h-[640px]" />}
      </Loaded>
      <Made className="mt-3" ops={["GetTemplate"]} more={q.data?.StagesAvailable && q.data.StagesAvailable.length > 1 ? `Stages: ${q.data.StagesAvailable.join(", ")}; this is Original.` : undefined} />
    </Card>
  );
}

type ChangeSet = { ChangeSetId?: string; ChangeSetName: string; Status?: string; ExecutionStatus?: string; CreationTime?: number | string; StatusReason?: string; Description?: string };

const SET_COLUMNS: DataColumn<ChangeSet>[] = [
  { id: "n", header: "Name", size: 220, accessorKey: "ChangeSetName", meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px] font-medium text-bright">{c.row.original.ChangeSetName}</span> },
  { id: "s", header: "Status", size: 190, accessorFn: (r) => r.Status ?? "", cell: (c) => <StatusDot tone={stackTone(c.row.original.Status ?? "")}>{c.row.original.Status}</StatusDot> },
  { id: "e", header: "Execution status", size: 170, accessorFn: (r) => r.ExecutionStatus ?? "", meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px]">{c.row.original.ExecutionStatus ?? "–"}</span> },
  { id: "t", header: "Created (UTC)", size: 210, accessorFn: (r) => String(r.CreationTime ?? ""), cell: (c) => at(c.row.original.CreationTime) },
  { id: "r", header: "Status reason", accessorFn: (r) => r.StatusReason ?? "", cell: (c) => c.row.original.StatusReason || <span className="text-muted-foreground">–</span> },
];

export function StackChangeSets({ name }: { name: string }) {
  const q = useOpRead<{ Summaries?: ChangeSet[] }>(on.stack(name), "ListChangeSets", { StackName: name });
  return (
    <Card title="Change sets" lede="A change set previews what an update would do to the stack's resources; nothing changes until one is executed (aws cloudformation execute-change-set)." testId="stack-changesets">
      <Loaded q={q} what="The change sets">
        {(p) => ((p.Summaries ?? []).length === 0 ? <p className="rounded-[12px] border border-dashed px-5 py-4 text-[13.5px] text-muted-foreground" data-testid="changesets-none">None. Make one with aws cloudformation create-change-set.</p> : <DataTable aria-label="Change sets" data={p.Summaries ?? []} columns={SET_COLUMNS} rowId={(r) => r.ChangeSetId ?? r.ChangeSetName} maxHeight={420} />)}
      </Loaded>
      <Made className="mt-3" ops={["ListChangeSets"]} />
    </Card>
  );
}

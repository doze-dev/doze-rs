// A state machine's executions, the newest first (ListExecutions): name, status, start and stop dates, the version and alias each was
// started with, and how often it was redriven; each opens on its own page (stepfunctions__machine-detail, executions-none).
import { Link } from "@tanstack/react-router";
import { ChevronLeftIcon, ChevronRightIcon, PlayIcon } from "lucide-react";
import { useState } from "react";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Button } from "@doze/ui/components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { RecentCalls } from "../../../kit";
import { on, useOpRead } from "../deep/api";
import { at, Loaded } from "../deep/parts";
import { StartDialog } from "./dialogs";
import { fieldOf } from "./details-card";
import { STATUS } from "./history";
import { aliasOf, versionOf } from "./model";
import { parseDefinition } from "./graph";

export type ExecutionItem = { executionArn: string; name: string; status: string; startDate: number | string; stopDate?: number | string; redriveCount?: number; stateMachineVersionArn?: string; stateMachineAliasArn?: string };

export const statusOf = (s: string) => STATUS[s] ?? { word: s, tone: "idle" as const };

const dash = <span className="text-muted-foreground">-</span>;

const COLUMNS: DataColumn<ExecutionItem>[] = [
  { id: "n", header: "Name", size: 250, min: 160, accessorKey: "name", meta: { mono: true }, cell: (c) => <Link to="/resources/$service/$kind/$name" params={{ service: "stepfunctions", kind: "executions", name: c.row.original.executionArn }} search={{}} className="rounded-sm font-mono text-[13px] font-medium text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" data-testid="execution-link">{c.row.original.name}</Link> },
  { id: "s", header: "Status", size: 130, accessorKey: "status", cell: (c) => { const s = statusOf(c.row.original.status); return <StatusDot tone={s.tone}>{s.word}</StatusDot>; } },
  { id: "a", header: "Start date", size: 190, accessorFn: (r) => String(r.startDate), cell: (c) => at(c.row.original.startDate) },
  { id: "b", header: "Stop date", size: 190, accessorFn: (r) => String(r.stopDate ?? ""), cell: (c) => (c.row.original.stopDate ? at(c.row.original.stopDate) : dash) },
  { id: "v", header: "Version", size: 100, accessorFn: (r) => versionOf(r.stateMachineVersionArn) ?? "", cell: (c) => versionOf(c.row.original.stateMachineVersionArn) ?? dash },
  { id: "l", header: "Alias", size: 110, accessorFn: (r) => aliasOf(r.stateMachineAliasArn) ?? "", cell: (c) => aliasOf(c.row.original.stateMachineAliasArn) ?? dash },
  { id: "r", header: "Redrive count", size: 150, fit: 104, accessorFn: (r) => r.redriveCount ?? 0, meta: { numeric: true }, cell: (c) => c.row.original.redriveCount ?? 0 },
];

const FILTERS = ["ALL", "RUNNING", "SUCCEEDED", "FAILED", "TIMED_OUT", "ABORTED"];

export function MachineExecutions({ d }: { d: ResourceDetail }) {
  const [starting, setStarting] = useState(false);
  const [opened, setOpened] = useState(0);
  const [filter, setFilter] = useState("ALL");
  const [tokens, setTokens] = useState<string[]>([]);
  const token = tokens[tokens.length - 1];
  const express = fieldOf(d, "Type") === "EXPRESS";
  const q = useOpRead<{ executions?: ExecutionItem[]; nextToken?: string }>(on.machine(d.name), "ListExecutions", { maxResults: 50, ...(filter !== "ALL" ? { statusFilter: filter } : {}), ...(token ? { nextToken: token } : {}) }, !express);
  const startAt = parseDefinition(fieldOf(d, "Definition") ?? "")?.StartAt;
  const start = () => { setOpened((n) => n + 1); setStarting(true); };
  const dialog = <StartDialog key={opened} machine={d.id} name={d.name} startAt={startAt} open={starting} onOpenChange={setStarting} />;
  const calls = <RecentCalls calls={d.calls} noun="state machine" className="mt-10" />;
  if (express) {
    return (
      <>
        <EmptyTab icon={PlayIcon} title="No execution history" testId="executions-express">{d.name} is an Express state machine. Express keeps no execution history: its runs are written to its log group.</EmptyTab>
      </>
    );
  }
  return (
    <>
      <Loaded q={q} what="The executions">
        {(p) => {
          const list = p.executions ?? [];
          if (list.length === 0 && filter === "ALL" && tokens.length === 0) {
            return (
              <EmptyTab icon={PlayIcon} title="No executions" action={<Button type="button" variant="outline" className="bg-card" onClick={start} data-testid="start-empty">Start execution</Button>} testId="executions-none">
                {d.name} has not run. Start an execution to see it here.
              </EmptyTab>
            );
          }
          return (
            <>
              <div className="mb-3 flex justify-end">
                <Select value={filter} onValueChange={(f) => { setFilter(f); setTokens([]); }}>
                  <SelectTrigger className="w-40 bg-card" aria-label="Status" data-testid="executions-filter"><SelectValue /></SelectTrigger>
                  <SelectContent>{FILTERS.map((f) => <SelectItem key={f} value={f}>{f === "ALL" ? "Any status" : statusOf(f).word}</SelectItem>)}</SelectContent>
                </Select>
              </div>
              <DataTable aria-label="Executions" data={list} columns={COLUMNS} rowId={(r) => r.executionArn} maxHeight={560} empty="None with this status." />
              <div className="mt-4 flex items-center justify-end gap-3 text-[13px] text-muted-foreground">
                <span data-testid="page-line">{list.length} {list.length === 1 ? "execution" : "executions"} on this page</span>
                <Button variant="outline" size="icon" className="bg-card" disabled={tokens.length === 0} onClick={() => setTokens(tokens.slice(0, -1))} aria-label="Newer executions" data-testid="page-newer"><ChevronLeftIcon /></Button>
                <Button variant="outline" size="icon" className="bg-card" disabled={!p.nextToken} onClick={() => setTokens([...tokens, p.nextToken as string])} aria-label="Older executions" data-testid="page-older"><ChevronRightIcon /></Button>
              </div>
              <p className="mt-5 max-w-[46rem] border-l-2 border-input pl-3.5 text-[13px] leading-5 text-muted-foreground" data-testid="executions-note">
                <b className="font-medium text-foreground">Version</b> and <b className="font-medium text-foreground">Alias</b> come from the ARN a run was started with. A run started with the plain state machine ARN has neither, even when a version holds the same revision. Running executions are listed by when they started; finished ones by when they stopped.
              </p>
              {calls}
            </>
          );
        }}
      </Loaded>
      {dialog}
    </>
  );
}

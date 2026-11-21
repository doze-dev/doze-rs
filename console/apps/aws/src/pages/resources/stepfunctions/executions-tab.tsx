// A state machine's executions, the newest first (ListExecutions), each opening on its own page; and Start execution.
import { Link } from "@tanstack/react-router";
import { PlayIcon } from "lucide-react";
import { useState } from "react";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Button } from "@doze/ui/components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { on, useOpRead } from "../deep/api";
import { at, Card, Loaded, Made } from "../deep/parts";
import { StartDialog } from "./dialogs";
import { STATUS } from "./history";

export type ExecutionItem = { executionArn: string; name: string; status: string; startDate: number | string; stopDate?: number | string; redriveCount?: number };

export const statusOf = (s: string) => STATUS[s] ?? { word: s, tone: "idle" as const };

const COLUMNS: DataColumn<ExecutionItem>[] = [
  { id: "n", header: "Name", accessorKey: "name", meta: { mono: true }, cell: (c) => <Link to="/resources/$service/$kind/$name" params={{ service: "stepfunctions", kind: "executions", name: c.row.original.executionArn }} search={{}} className="rounded-sm font-mono text-[13px] text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" data-testid="execution-link">{c.row.original.name}</Link> },
  { id: "s", header: "Status", size: 150, accessorKey: "status", cell: (c) => { const s = statusOf(c.row.original.status); return <StatusDot tone={s.tone}>{s.word}</StatusDot>; } },
  { id: "a", header: "Started (UTC)", size: 220, accessorFn: (r) => String(r.startDate), cell: (c) => at(c.row.original.startDate) },
  { id: "b", header: "Stopped (UTC)", size: 220, accessorFn: (r) => String(r.stopDate ?? ""), cell: (c) => at(c.row.original.stopDate) },
];

const FILTERS = ["ALL", "RUNNING", "SUCCEEDED", "FAILED", "TIMED_OUT", "ABORTED"];

export function MachineExecutions({ arn, name }: { arn: string; name: string }) {
  const [starting, setStarting] = useState(false);
  const [opened, setOpened] = useState(0);
  const [filter, setFilter] = useState("ALL");
  const [tokens, setTokens] = useState<string[]>([]);
  const token = tokens[tokens.length - 1];
  const q = useOpRead<{ executions?: ExecutionItem[]; nextToken?: string }>(on.machine(name), "ListExecutions", { stateMachineArn: arn, maxResults: 50, ...(filter !== "ALL" ? { statusFilter: filter } : {}), ...(token ? { nextToken: token } : {}) });
  return (
    <Card
      title="Executions"
      lede="Each run of the machine, the newest first. Open one for its graph, its events, its input and output."
      actions={
        <>
          <Select value={filter} onValueChange={(f) => { setFilter(f); setTokens([]); }}>
            <SelectTrigger className="w-40 bg-card" aria-label="Status" data-testid="executions-filter"><SelectValue /></SelectTrigger>
            <SelectContent>
              {FILTERS.map((f) => <SelectItem key={f} value={f}>{f === "ALL" ? "Any status" : statusOf(f).word}</SelectItem>)}
            </SelectContent>
          </Select>
          <Button onClick={() => { setOpened((n) => n + 1); setStarting(true); }} data-testid="start-open"><PlayIcon /> Start execution</Button>
        </>
      }
      testId="machine-executions"
    >
      <Loaded q={q} what="The executions">
        {(p) => (
          <>
            <DataTable aria-label="Executions" data={p.executions ?? []} columns={COLUMNS} rowId={(r) => r.executionArn} maxHeight={560} empty={filter === "ALL" ? "No execution yet. Start one." : "None with this status."} />
            <div className="mt-3 flex justify-end gap-2">
              {tokens.length > 0 && <Button variant="outline" size="sm" className="bg-card" onClick={() => setTokens(tokens.slice(0, -1))}>Newer</Button>}
              {p.nextToken && <Button variant="outline" size="sm" className="bg-card" onClick={() => setTokens([...tokens, p.nextToken as string])}>Older</Button>}
            </div>
          </>
        )}
      </Loaded>
      <Made className="mt-3" ops={["ListExecutions"]} />
      <StartDialog key={opened} machine={arn} name={name} open={starting} onOpenChange={setStarting} />
    </Card>
  );
}

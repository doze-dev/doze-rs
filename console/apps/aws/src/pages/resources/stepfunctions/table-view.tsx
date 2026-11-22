// An execution's steps as a table (stepfunctions__execution-table): one row per state it entered, with what it is, how it ended, what
// it called, when it started after the execution did and how long it took. Choosing a row chooses the step shown under the table.
import { SearchIcon, SettingsIcon } from "lucide-react";
import { useState } from "react";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { Button } from "@doze/ui/components/ui/button";
import { DropdownMenu, DropdownMenuCheckboxItem, DropdownMenuContent, DropdownMenuLabel, DropdownMenuTrigger } from "@doze/ui/components/ui/dropdown-menu";
import { Input } from "@doze/ui/components/ui/input";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Card } from "../deep/parts";
import { durationWords } from "./execution-model";
import type { AslState } from "./graph";
import { STATUS, type Step } from "./history";
import { taskWords } from "./model";

type Row = { step: Step; state: AslState | undefined; after: number | null };

const dash = <span className="text-muted-foreground">–</span>;

export function TableView({ steps, states, startMs, selected, onSelect }: { steps: Step[]; states: Record<string, AslState>; startMs: number | null; selected?: string; onSelect: (name: string) => void }) {
  const [filter, setFilter] = useState("");
  // Preferences: the columns that are not shown (the name always is).
  const [hidden, setHidden] = useState<string[]>([]);
  const all: Row[] = steps.map((step) => ({ step, state: states[step.name], after: startMs !== null && step.startMs !== undefined ? step.startMs - startMs : null }));
  const rows = all.filter((r) => `${r.step.name} ${r.state?.Type ?? ""} ${r.step.status} ${r.state ? taskWords(r.state) : ""}`.toLowerCase().includes(filter.trim().toLowerCase()));
  const columns: DataColumn<Row>[] = [
    { id: "n", header: "Name", size: 240, min: 150, accessorFn: (r) => r.step.name, meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px] font-medium text-bright">{c.row.original.step.name}</span> },
    { id: "t", header: "Type", size: 120, accessorFn: (r) => r.state?.Type ?? "" },
    { id: "s", header: "Status", size: 140, accessorFn: (r) => r.step.status, cell: (c) => { const s = STATUS[c.row.original.step.status] ?? { word: c.row.original.step.status, tone: "idle" as const }; return <StatusDot tone={s.tone}>{s.word}</StatusDot>; } },
    { id: "r", header: "Resource", min: 200, accessorFn: (r) => (r.state ? taskWords(r.state) : ""), cell: (c) => (c.row.original.state?.Type === "Task" ? taskWords(c.row.original.state) : dash) },
    { id: "a", header: "Started After", size: 150, fit: 104, accessorFn: (r) => r.after ?? 0, meta: { numeric: true }, cell: (c) => durationWords(c.row.original.after) },
    { id: "d", header: "Duration", size: 130, accessorFn: (r) => (r.step.endMs !== undefined && r.step.startMs !== undefined ? r.step.endMs - r.step.startMs : 0), meta: { numeric: true }, cell: (c) => (c.row.original.step.status === "running" ? dash : durationWords(c.row.original.step.endMs !== undefined && c.row.original.step.startMs !== undefined ? c.row.original.step.endMs - c.row.original.step.startMs : null)) },
  ];
  const id = (c: DataColumn<Row>) => c.id ?? "";
  const shown = columns.filter((c) => id(c) === "n" || !hidden.includes(id(c)));
  const optional = columns.filter((c) => id(c) !== "n");
  return (
    <Card title="Table view" lede="Each state the execution entered, in the order it entered them. Choose a row to see its details." testId="execution-table">
      <div className="mb-4 flex flex-wrap items-center gap-3">
        <div className="relative w-full max-w-[22rem]">
          <SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input value={filter} onChange={(e) => setFilter(e.target.value)} placeholder="Filter by properties or search by keyword" aria-label="Filter steps" className="bg-card pl-9" data-testid="steps-filter" />
        </div>
        <DropdownMenu>
          <DropdownMenuTrigger asChild><Button type="button" variant="outline" className="bg-card" data-testid="steps-preferences"><SettingsIcon /> Preferences</Button></DropdownMenuTrigger>
          <DropdownMenuContent align="start" className="min-w-56">
            <DropdownMenuLabel>Columns</DropdownMenuLabel>
            {optional.map((c) => (
              <DropdownMenuCheckboxItem key={c.id} checked={!hidden.includes(id(c))} onSelect={(e) => e.preventDefault()} onCheckedChange={(on) => setHidden(on ? hidden.filter((h) => h !== id(c)) : [...hidden, id(c)])} data-testid={`steps-column-${id(c)}`}>{String(c.header)}</DropdownMenuCheckboxItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      <DataTable aria-label="Steps" data={rows} columns={shown} rowId={(r) => r.step.name} maxHeight={420} rowClassName={(r) => (r.step.name === selected ? "[&>td]:bg-accent" : undefined)} onRowClick={(r) => onSelect(r.step.name)} empty={all.length === 0 ? "This execution has not entered a state yet." : "No step matches."} />
    </Card>
  );
}

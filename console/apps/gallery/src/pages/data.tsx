import { useMemo, useState } from "react";
import { CodeValue } from "@doze/ui/components/app/code-value";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { IdValue } from "@doze/ui/components/app/id-value";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Spans } from "@doze/ui/components/app/spans";
import { Figures, StatStrip } from "@doze/ui/components/app/stat";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Badge } from "@doze/ui/components/ui/badge";
import { Button } from "@doze/ui/components/ui/button";
import { Table, TableBody, TableCaption, TableCell, TableHead, TableHeader, TableRow } from "@doze/ui/components/ui/table";
import { clockUtc, number, plural } from "@doze/ui/lib/format";
import { LONG, type Resource, resources } from "../data";
import { Section, Specimen } from "../specimen";

const COLUMNS: DataColumn<Resource>[] = [
  { id: "name", header: "Name", accessorKey: "name", meta: { mono: true } },
  { id: "kind", header: "Kind", accessorKey: "kind", size: 110, cell: (c) => <Badge variant="secondary">{c.row.original.kind}</Badge> },
  {
    id: "state",
    header: "State",
    accessorKey: "state",
    size: 120,
    cell: (c) => {
      const s = c.row.original.state;
      return <StatusDot tone={s === "Active" ? "ok" : s === "Failing" ? "bad" : "idle"}>{s}</StatusDot>;
    },
  },
  { id: "calls", header: "Calls", accessorKey: "calls", size: 120, meta: { numeric: true }, cell: (c) => number(c.row.original.calls) },
  { id: "updated", header: "Last call, UTC", accessorKey: "updated", size: 150, meta: { numeric: true, mono: true }, cell: (c) => clockUtc(c.row.original.updated) },
];

const THOUSAND = resources(1000);
const TWELVE = resources(12);

function Tail() {
  const [rows, setRows] = useState(() => resources(6));
  const [fresh, setFresh] = useState<string | null>(null);
  const add = () => {
    const id = `new${rows.length}`;
    setRows((r) => [{ ...r[0]!, id, name: `arrived-${String(r.length).padStart(4, "0")}`, state: "Active" }, ...r]);
    setFresh(id);
  };
  return (
    <div className="flex w-full flex-col gap-3">
      <div>
        <Button variant="outline" size="sm" onClick={add} data-testid="tail-add">
          A row arrives
        </Button>
      </div>
      <DataTable data={rows} columns={COLUMNS} rowId={(r) => r.id} aria-label="A tail" maxHeight={260} anchorTop rowClassName={(r) => (r.id === fresh ? "row-new" : undefined)} />
    </div>
  );
}

export function DataPage() {
  const [picked, setPicked] = useState("none");
  const big = useMemo(() => THOUSAND, []);
  return (
    <>
      <PageHeader title="Data" sub="Figures and tables. A table's layout never depends on its rows: fixed columns, one line a cell, numbers in tabular figures, and a virtual body when the rows are many." />

      <Section id="stat" title="StatStrip and Figures" source="components/app/stat.tsx" note="A live value is never in a sentence: it has a slot of reserved width, so a changing number moves nothing.">
        <Specimen label="A strip" stack>
          <StatStrip
            className="w-full"
            stats={[
              { label: "Queues", value: "12", unit: "3 FIFO" },
              { label: "Waiting", value: "1,204", unit: "messages", tip: "1,204 visible messages across 12 queues" },
              { label: "In flight", value: "37", unit: "hidden while being worked" },
              { label: "Oldest", value: "3 d 4 h", unit: "in pick-lists-dlq" },
              { label: "Endpoint", value: "127.0.0.1:4566", mono: true, ch: 14 },
            ]}
          />
        </Specimen>
        <Specimen label="Dense, long text" stack>
          <StatStrip className="w-full" dense stats={[{ label: "A label that is long enough to take two lines in a narrow tile", value: "128,004,551,902", unit: "a unit that is cut with an ellipsis when it does not fit its tile" }, { label: "Empty", value: "0" }, { label: "Not known", value: "–", unit: "never measured" }]} />
        </Specimen>
        <Specimen label="Figures">
          <Figures aria-label="A queue's counts" figures={[{ label: "Waiting", value: "3" }, { label: "In flight", value: "0" }, { label: "Delayed", value: "12" }, { label: "Receives", value: "48,210", ch: 7 }]} />
        </Specimen>
      </Section>

      <Section id="table" title="DataTable" source="components/app/data-table.tsx">
        <Specimen label="Sortable, rows open" stack>
          <DataTable data={TWELVE} columns={COLUMNS} rowId={(r) => r.id} aria-label="Twelve resources" maxHeight={420} onRowClick={(r) => setPicked(r.name)} />
          <span className="text-[12.5px] text-muted-foreground">
            Opened: <code data-testid="table-picked">{picked}</code>
          </span>
        </Specimen>
        <Specimen label={`${plural(big.length, "row")}, virtual`} stack>
          <DataTable data={big} columns={COLUMNS} rowId={(r) => r.id} aria-label="A thousand resources" maxHeight={440} className="w-full" />
        </Specimen>
        <Specimen label="A new row" stack>
          <Tail />
        </Specimen>
        <Specimen label="Empty" stack>
          <DataTable data={[]} columns={COLUMNS} rowId={(r) => r.id} aria-label="No resources" empty="No queue matches “zzz”. Clear the filter to see all 12." className="w-full" />
        </Specimen>
      </Section>

      <Section id="plain-table" title="Table" source="components/ui/table.tsx" note="The plain table, for a short list of keys and values.">
        <Specimen label="Keys and values" stack>
          <div className="w-full max-w-[40rem] rounded-xl border bg-card">
            <Table>
              <TableCaption className="sr-only">A queue's attributes</TableCaption>
              <TableHeader>
                <TableRow>
                  <TableHead>Attribute</TableHead>
                  <TableHead>Value</TableHead>
                  <TableHead className="text-right">Default</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {(
                  [
                    ["VisibilityTimeout", "30 s", "30 s"],
                    ["MessageRetentionPeriod", "4 d", "4 d"],
                    ["MaximumMessageSize", "1 MiB", "1 MiB"],
                  ] as const
                ).map(([k, v, d]) => (
                  <TableRow key={k}>
                    <TableCell className="font-mono text-[13px]">{k}</TableCell>
                    <TableCell className="tabular-nums">{v}</TableCell>
                    <TableCell className="text-right text-muted-foreground tabular-nums">{d}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        </Specimen>
      </Section>

      <Section id="values" title="CodeValue, IdValue and Spans" source="components/app/code-value.tsx · id-value.tsx · spans.tsx">
        <Specimen label="A long value">
          <CodeValue value={LONG} label="ARN" className="w-full max-w-[34rem]" />
        </Specimen>
        <Specimen label="An id in a line">
          <p className="max-w-[34rem] text-[13.5px]">
            Read by <IdValue value="consumer-7f3a9c2e-41d8-4b6e-9a57-0c1d2e3f4a5b" what="Consumer id copied" className="max-w-[16rem]" /> since 14:32 UTC.
          </p>
        </Specimen>
        <Specimen label="A server's sentence">
          <p className="max-w-[34rem] text-[13.5px] text-muted-foreground">
            <Spans
              spans={[
                { kind: "strong", text: "3" },
                { kind: "text", text: " waiting in " },
                { kind: "mono", text: "orders" },
                { kind: "text", text: ", dead letters to " },
                { kind: "mono", text: "pick-lists-dlq" },
                { kind: "text", text: "." },
              ]}
            />
          </p>
        </Specimen>
      </Section>
    </>
  );
}

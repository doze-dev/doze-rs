import { Trash2Icon } from "lucide-react";
import { useMemo, useState } from "react";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { FilterChips } from "@doze/ui/components/app/filter-chips";
import { chipValue } from "@doze/ui/components/app/filter-chips-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Pagination } from "@doze/ui/components/app/pagination";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Badge } from "@doze/ui/components/ui/badge";
import { Button } from "@doze/ui/components/ui/button";
import { useUrlParam } from "@doze/ui/hooks/use-url-param";
import { clockUtc, number } from "@doze/ui/lib/format";
import { type Resource, resources } from "../data";
import { Section, Specimen } from "../specimen";

const COLUMNS: DataColumn<Resource>[] = [
  { id: "name", header: "Name", accessorKey: "name", meta: { mono: true } },
  { id: "kind", header: "Kind", accessorKey: "kind", size: 110, cell: (c) => <Badge variant="secondary">{c.row.original.kind}</Badge> },
  { id: "state", header: "State", accessorKey: "state", size: 120, cell: (c) => <StatusDot tone={c.row.original.state === "Active" ? "ok" : c.row.original.state === "Failing" ? "bad" : "idle"}>{c.row.original.state}</StatusDot> },
  { id: "calls", header: "Calls", accessorKey: "calls", size: 110, meta: { numeric: true }, cell: (c) => number(c.row.original.calls) },
  { id: "updated", header: "Last call, UTC", accessorKey: "updated", size: 150, meta: { numeric: true, mono: true }, cell: (c) => clockUtc(c.row.original.updated) },
];
const TWELVE = resources(12);
const THOUSAND = resources(1000);
const rowId = (r: Resource) => r.id;

function Selectable({ rows, label, max = 360, toolbar }: { rows: Resource[]; label: string; max?: number; toolbar?: React.ReactNode }) {
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [gone, setGone] = useState(0);
  return (
    <div className="w-full">
      <SelectableTable
        toolbar={toolbar}
        data={rows}
        columns={COLUMNS}
        rowId={rowId}
        selected={selected}
        onSelectedChange={setSelected}
        noun="resource"
        aria-label={label}
        maxHeight={max}
        actions={(chosen) => (
          <Button variant="outline" size="sm" className="border-bad/40 bg-card text-bad hover:text-bad" onClick={() => { setGone(chosen.length); setSelected(new Set()); }} data-testid="bulk-delete">
            <Trash2Icon /> Delete {chosen.length}
          </Button>
        )}
      />
      {gone > 0 && <p className="mt-2 text-[13px] text-muted-foreground" data-testid="bulk-done">{gone} deleted (not really: this is a specimen).</p>}
    </div>
  );
}

const STATES = ["Active", "Idle", "Failing"] as const;

/** Chips, the table they filter, and the paging line: the shape of a list page, the chip kept in the address. */
function FilteredList() {
  const [asked, setAsked] = useUrlParam("state");
  const chips = useMemo(() => [{ value: "", label: "All", count: TWELVE.length }, ...STATES.map((s) => ({ value: s, label: s, count: TWELVE.filter((r) => r.state === s).length }))], []);
  const chosen = chipValue(asked, chips);
  const rows = TWELVE.filter((r) => !chosen || r.state === chosen);
  return (
    <div className="grid w-full gap-3" data-testid="filtered-list">
      <FilterChips label="Filter resources by state" chips={chips} value={asked} onChange={(v) => setAsked(v || undefined)} />
      <DataTable data={rows} columns={COLUMNS} rowId={rowId} aria-label="Filtered resources" maxHeight={300} empty="Nothing is in that state." />
      <Pagination shown={rows.length} total={rows.length} noun="resources" />
    </div>
  );
}

function Loadable() {
  const [n, setN] = useState(25);
  return (
    <div className="grid w-full gap-3">
      <DataTable data={THOUSAND.slice(0, n)} columns={COLUMNS} rowId={rowId} aria-label="A paged list" maxHeight={240} />
      <Pagination shown={n} more={n < 100} onMore={() => setN((x) => x + 25)} noun="resources" />
    </div>
  );
}

export function ListsPage() {
  return (
    <>
      <PageHeader title="Lists" sub="What a list page is made of beyond the table: choosing rows, filtering by a chip kept in the address, and the line that says how much there is." />

      <Section id="selectable" title="SelectableTable" source="components/app/selectable-table.tsx" note="A DataTable with a checkbox on each row. The bar above holds its height, so choosing a row moves nothing; the header's checkbox is a bar when some rows are chosen. Clicking a checkbox does not open the row.">
        <Specimen label="Twelve rows" stack>
          <Selectable rows={TWELVE} label="Twelve selectable resources" />
        </Specimen>
        <Specimen label="With the list's own controls" stack>
          <Selectable rows={TWELVE.slice(0, 5)} label="Resources with a toolbar" max={300} toolbar={<FilterChips label="Filter by state" chips={[{ value: "", label: "All", count: 5 }, { value: "Active", label: "Active", count: 3 }, { value: "Idle", label: "Idle", count: 1 }]} value="" onChange={() => undefined} />} />
        </Specimen>
        <Specimen label="1,000 rows (virtual)" stack>
          <Selectable rows={THOUSAND} label="A thousand selectable resources" max={300} />
        </Specimen>
        <Specimen label="Empty" stack>
          <div className="w-full">
            <SelectableTable data={[]} columns={COLUMNS} rowId={rowId} selected={new Set()} onSelectedChange={() => undefined} noun="resource" aria-label="No resources" empty="No resources yet." maxHeight={200} />
          </div>
        </Specimen>
      </Section>

      <Section id="chips" title="FilterChips" source="components/app/filter-chips.tsx" note="One chip is chosen; each says how many rows it holds, and a chip with none stays (a filter that vanishes moves under the cursor). The choice is the address's: ?state=Idle opens the list filtered, and Reload keeps it.">
        <Specimen label="A filtered list" stack>
          <FilteredList />
        </Specimen>
        <Specimen label="Long labels, no counts">
          <FilterChips label="Filter by long names" chips={[{ value: "", label: "Everything the service has ever had" }, { value: "a", label: "Insufficient data, waiting for the first metric" }, { value: "b", label: "OK" }]} value="a" onChange={() => undefined} />
        </Specimen>
        <Specimen label="An unknown value">
          <FilterChips label="Filter with an old link" chips={[{ value: "", label: "All", count: 4 }, { value: "x", label: "X", count: 0 }]} value="gone" onChange={() => undefined} />
        </Specimen>
      </Section>

      <Section id="pagination" title="Pagination" source="components/app/pagination.tsx" note="1 to N of N when the service has said how many; when it has only given a token, what is loaded and more to load, and Load more reads the next page.">
        <Specimen label="Whole list">
          <Pagination shown={25} total={25} noun="queues" className="w-full" />
        </Specimen>
        <Specimen label="Empty">
          <Pagination shown={0} className="w-full" />
        </Specimen>
        <Specimen label="Load more" stack>
          <Loadable />
        </Specimen>
        <Specimen label="Loading the next page">
          <Pagination shown={50} more onMore={() => undefined} loading className="w-full" />
        </Specimen>
      </Section>
    </>
  );
}

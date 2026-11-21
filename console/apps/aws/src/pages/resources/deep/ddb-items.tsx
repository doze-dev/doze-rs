// A table's items: a Scan, or a Query by the partition key (and a sort condition) of the table or one of its indexes, with an
// optional filter; the items as a table with the key columns first, a page at a time; an item's JSON in a drawer; create, edit
// and delete. The table's keys and indexes come from DescribeTable.
import { ChevronLeftIcon, ChevronRightIcon, PlayIcon, PlusIcon } from "lucide-react";
import { useState } from "react";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { ToggleGroup, ToggleGroupItem } from "@doze/ui/components/ui/toggle-group";
import { number, plural } from "@doze/ui/lib/format";
import { useOpQuery } from "./api";
import { ItemDialog, ItemSheet } from "./ddb-item-dialog";
import { type Item, type KeyPart, cell } from "./ddb-json";
import { type Described, type Form, request, SORT_OPS, type SortOp, START, sourcesOf } from "./ddb-request";
import type { DeepProps } from "./pages";
import { Card, JsonBlock, Refused } from "./parts";

const PAGE = 50;
/** The most attribute columns shown beside the keys; the rest are in an item's drawer. */
const MOST_COLUMNS = 12;

type Answer = { Items?: Item[]; Count?: number; ScannedCount?: number; LastEvaluatedKey?: Item };

const TYPE: Record<string, string> = { S: "String", N: "Number", B: "Binary" };

/** The columns of a page of items: the keys first, then the other attributes the page holds, by name. */
export function columnsOf(items: Item[], keys: KeyPart[]): { name: string; sub: string }[] {
  const seen = new Set(keys.map((k) => k.name));
  const rest = [...new Set(items.flatMap((i) => Object.keys(i)))].filter((n) => !seen.has(n)).sort();
  return [...keys.map((k) => ({ name: k.name, sub: `${k.role === "HASH" ? "Partition key" : "Sort key"} · ${TYPE[k.type] ?? k.type}` })), ...rest.slice(0, MOST_COLUMNS).map((name) => ({ name, sub: "" }))];
}

export function TableItems({ d }: DeepProps) {
  const t = { service: d.service, kind: d.kind, name: d.name };
  const described = useOpQuery<Described>(t, "DescribeTable", {}, ["describe"]);
  const [form, setForm] = useState<Form>(START);
  const [ran, setRan] = useState<Form>(START);
  const [starts, setStarts] = useState<Item[]>([]);
  const [open, setOpen] = useState<Item | null>(null);
  const [creating, setCreating] = useState(false);
  const sources = described.data ? sourcesOf(described.data) : [];
  const table = sources[0];
  const source = sources.find((s) => s.index === form.index) ?? table;
  const ranSource = sources.find((s) => s.index === ran.index) ?? table;
  const req = source ? request(form, source) : "Reading the table's keys…";
  const ranReq = ranSource ? request(ran, ranSource) : null;
  const start = starts.at(-1);
  const input = ranReq && typeof ranReq !== "string" ? { ...ranReq.input, Limit: PAGE, ...(start ? { ExclusiveStartKey: start } : {}) } : {};
  const op = ranReq && typeof ranReq !== "string" ? ranReq.op : "Scan";
  const items = useOpQuery<Answer>(t, op, input, ["items", JSON.stringify(input)], !!ranReq && typeof ranReq !== "string");
  const keys = [...(table?.keys ?? []), ...(ranSource?.keys ?? []).filter((k) => !table?.keys.some((x) => x.name === k.name))];
  const rows = items.data?.Items ?? [];
  const cols = columnsOf(rows, keys);
  const columns: DataColumn<Item>[] = cols.map((c, i) => ({
    id: `c${i}`,
    header: () => (
      <span className="grid normal-case">
        <span className="font-mono text-[12.5px] tracking-normal text-foreground">{c.name}</span>
        {c.sub && <span className="text-[11.5px] font-normal tracking-normal">{c.sub}</span>}
      </span>
    ),
    size: i < keys.length ? 200 : undefined,
    accessorFn: (r) => cell(r[c.name]),
    meta: { mono: true },
    cell: (x) => <span className={i === 0 ? "text-link" : undefined}>{cell(x.row.original[c.name]) || <span className="text-muted-foreground">–</span>}</span>,
  }));
  const run = (e: React.FormEvent) => {
    e.preventDefault();
    if (typeof req === "string") return;
    setRan(form);
    setStarts([]);
    if (JSON.stringify(form) === JSON.stringify(ran)) void items.refetch();
  };
  const set = (patch: Partial<Form>) => setForm((f) => ({ ...f, ...patch }));
  const pk = source?.keys.find((k) => k.role === "HASH");
  const sk = source?.keys.find((k) => k.role === "RANGE");
  if (described.isPending) return <Skeleton className="h-80 rounded-[14px]" />;
  return (
    <div className="grid gap-6">
      <Refused operation="DescribeTable" error={described.error} />
      <div className="grid items-start gap-6 xl:grid-cols-[minmax(0,1fr)_400px]">
        <Card title="Scan or query items" testId="items-form">
          <form onSubmit={run} className="grid gap-5">
            <div className="flex flex-wrap items-end gap-4">
              <ToggleGroup type="single" variant="outline" value={form.mode} onValueChange={(v) => v && set({ mode: v as Form["mode"] })} aria-label="Scan or query">
                <ToggleGroupItem value="scan" className="px-4" data-testid="mode-scan">
                  Scan
                </ToggleGroupItem>
                <ToggleGroupItem value="query" className="px-4" data-testid="mode-query">
                  Query
                </ToggleGroupItem>
              </ToggleGroup>
              <div className="grid min-w-56 gap-1.5">
                <Label htmlFor="items-source">Table or index</Label>
                <Select value={form.index || "-"} onValueChange={(v) => set({ index: v === "-" ? "" : v })}>
                  <SelectTrigger id="items-source" className="w-full bg-card font-mono text-[13px]" data-testid="items-source">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {sources.map((s) => (
                      <SelectItem key={s.index || "-"} value={s.index || "-"} className="font-mono text-[13px]">
                        {s.index ? `${s.index} (${s.global ? "global" : "local"} index)` : `${d.name} (table)`}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            </div>
            {form.mode === "query" && pk && (
              <div className="grid gap-4 sm:grid-cols-2">
                <div className="grid gap-1.5">
                  <Label htmlFor="q-pk">
                    <span className="font-mono">{pk.name}</span> = <span className="font-normal text-muted-foreground">partition key, {TYPE[pk.type]}</span>
                  </Label>
                  <Input id="q-pk" value={form.pk} onChange={(e) => set({ pk: e.target.value })} className="bg-card font-mono text-[13px]" placeholder={pk.type === "N" ? "e.g. 42" : "e.g. ord-0372"} data-testid="q-pk" />
                </div>
                {sk && (
                  <div className="grid gap-1.5">
                    <Label htmlFor="q-sk">
                      <span className="font-mono">{sk.name}</span> <span className="font-normal text-muted-foreground">sort key, {TYPE[sk.type]}, optional</span>
                    </Label>
                    <div className="flex gap-2">
                      <Select value={form.sortOp} onValueChange={(v) => set({ sortOp: v as SortOp })}>
                        <SelectTrigger aria-label="Sort key condition" className="w-36 flex-none bg-card font-mono text-[13px]">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {SORT_OPS.map((o) => (
                            <SelectItem key={o} value={o} className="font-mono text-[13px]">
                              {o}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                      <Input id="q-sk" value={form.sk} onChange={(e) => set({ sk: e.target.value })} className="bg-card font-mono text-[13px]" data-testid="q-sk" />
                      {form.sortOp === "between" && <Input aria-label="and" value={form.sk2} onChange={(e) => set({ sk2: e.target.value })} className="bg-card font-mono text-[13px]" />}
                    </div>
                  </div>
                )}
              </div>
            )}
            <details className="group grid gap-3 [&_summary::-webkit-details-marker]:hidden">
              <summary className="cursor-pointer text-[13.5px] font-medium text-link">Filter (optional)</summary>
              <div className="mt-3 grid gap-3">
                <Input aria-label="FilterExpression" value={form.filter} onChange={(e) => set({ filter: e.target.value })} placeholder="e.g. #s = :s" className="bg-card font-mono text-[13px]" />
                <div className="grid gap-3 sm:grid-cols-2">
                  <Input aria-label="ExpressionAttributeNames" value={form.names} onChange={(e) => set({ names: e.target.value })} placeholder='e.g. {"#s": "status"}' className="bg-card font-mono text-[13px]" />
                  <Input aria-label="ExpressionAttributeValues" value={form.values} onChange={(e) => set({ values: e.target.value })} placeholder='e.g. {":s": {"S": "shipped"}}' className="bg-card font-mono text-[13px]" />
                </div>
                <p className="text-[12.5px] text-muted-foreground">Applied after the items are read: a page of {PAGE} read can return fewer.</p>
              </div>
            </details>
            {typeof req === "string" && (form.mode === "query" || form.filter) ? <p className="text-[13px] text-bad">{req}</p> : null}
            <div className="flex justify-end border-t pt-5">
              <Button type="submit" disabled={typeof req === "string"} data-testid="items-run">
                <PlayIcon /> Run
              </Button>
            </div>
          </form>
        </Card>
        <div className="rounded-[14px] border bg-card p-5">
          <p className="mb-3 text-[12px] font-medium tracking-[0.07em] text-muted-foreground uppercase">The call this will make</p>
          <p className="mb-3 font-mono text-[14px] text-bright">{typeof req === "string" ? form.mode === "query" ? "Query" : "Scan" : req.op}</p>
          <JsonBlock value={typeof req === "string" ? req : { TableName: d.name, ...req.input, Limit: PAGE }} className="max-h-72" testId="items-call" />
        </div>
      </div>
      <Card
        title={items.data ? `Items returned (${number(items.data.Count ?? rows.length)})` : "Items"}
        lede={items.data ? <>{op} read {plural(items.data.ScannedCount ?? 0, "item")} for this page{items.data.LastEvaluatedKey ? "; there is more" : ""}. Choose an item to see its JSON, edit or delete it.</> : undefined}
        actions={
          <Button variant="outline" className="bg-card" disabled={!table} onClick={() => setCreating(true)} data-testid="item-create">
            <PlusIcon /> Create item
          </Button>
        }
        testId="items-card"
      >
        <div className="grid gap-4">
          <Refused operation={op} error={items.error} />
          <DataTable aria-label="Items" data={rows} columns={columns} rowId={(r) => JSON.stringify(keys.map((k) => r[k.name]))} onRowClick={setOpen} maxHeight={560} empty={items.isFetching ? "Reading…" : ran.mode === "query" ? "No item matches this query." : "This table holds no item yet."} />
          <div className="flex items-center justify-end gap-2 text-[13px] text-muted-foreground">
            <span>Page {starts.length + 1}</span>
            <Button variant="outline" size="sm" className="bg-card" aria-label="Previous page" disabled={starts.length === 0} onClick={() => setStarts((s) => s.slice(0, -1))} data-testid="items-prev">
              <ChevronLeftIcon />
            </Button>
            <Button variant="outline" size="sm" className="bg-card" aria-label="Next page" disabled={!items.data?.LastEvaluatedKey} onClick={() => items.data?.LastEvaluatedKey && setStarts((s) => [...s, items.data.LastEvaluatedKey as Item])} data-testid="items-next">
              <ChevronRightIcon />
            </Button>
          </div>
        </div>
      </Card>
      {table && <ItemSheet item={open} onClose={() => setOpen(null)} t={t} keys={table.keys} />}
      {table && <ItemDialog key={creating ? "open" : "closed"} open={creating} onOpenChange={setCreating} t={t} keys={table.keys} item={null} />}
    </div>
  );
}

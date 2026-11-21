// Explore table items (dynamodb__items-explore, items-scan, items-none, item-delete): the form, the call it will make, and the items it
// returned with the key attributes first. Create item and Edit item are pages of their own; Delete asks over this page. What was
// typed is kept per table, so coming back from an item's page finds the query as it was.
import { ArrowLeftIcon, ChevronLeftIcon, ChevronRightIcon, PencilIcon, PlusIcon, SearchIcon, Trash2Icon } from "lucide-react";
import { useMemo, useState } from "react";
import type { DataColumn } from "@doze/ui/components/app/data-table";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { number, plural } from "@doze/ui/lib/format";
import { DetailError, CallPanel } from "../../../kit";
import { useOp, useOpQuery } from "../deep/api";
import type { OwnProps } from "../deep/registry";
import { Card, Refused } from "../deep/parts";
import { ExploreForm } from "./explore-form";
import { keyParam } from "./item-rows";
import { type Item, type KeyPart, cell, keyOf } from "./json";
import { TYPE_WORDS } from "./model";
import { Ask, crumbs, Said, tableTarget, useTable, useTableNav } from "./parts";
import { type Form, request, START, sourcesOf } from "./request";
import { isStatusColumn, statusTone } from "./status";

const PAGE = 50;
/** The most attribute columns shown beside the keys; the rest are in an item's JSON. */
const MOST_COLUMNS = 12;

type Answer = { Items?: Item[]; Count?: number; ScannedCount?: number; LastEvaluatedKey?: Item };

/** What was typed and run, by table. */
const MEMORY = new Map<string, { form: Form; ran: Form }>();

/** The columns of a page of items: the keys first, then the other attributes the page holds, by name. */
export function columnsOf(items: Item[], keys: KeyPart[], others: KeyPart[] = []): { name: string; sub: string; numeric: boolean }[] {
  const seen = new Set([...keys, ...others].map((k) => k.name));
  const rest = [...new Set(items.flatMap((i) => Object.keys(i)))].filter((n) => !seen.has(n)).sort();
  const numeric = (n: string) => items.some((i) => i[n]) && items.every((i) => !i[n] || "N" in i[n]!);
  return [
    ...keys.map((k) => ({ name: k.name, sub: k.role === "HASH" ? "Partition key" : "Sort key", numeric: k.type === "N" })),
    ...others.map((k) => ({ name: k.name, sub: TYPE_WORDS[k.type], numeric: k.type === "N" })),
    ...rest.slice(0, MOST_COLUMNS).map((name) => ({ name, sub: items.map((i) => i[name]).find(Boolean) ? (TYPE_WORDS[Object.keys(items.map((i) => i[name]).find(Boolean)!)[0] as keyof typeof TYPE_WORDS] ?? "") : "", numeric: numeric(name) })),
  ];
}

export function Explore({ name, search }: OwnProps) {
  const table = name!;
  const t = tableTarget(table);
  const read = useTable(table);
  const go = useTableNav(table);
  const [form, setForm] = useState<Form>(() => MEMORY.get(table)?.form ?? START);
  const [ran, setRan] = useState<Form>(() => MEMORY.get(table)?.ran ?? START);
  const [starts, setStarts] = useState<Item[]>([]);
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set());
  const [deleting, setDeleting] = useState(false);
  const del = useOp<unknown>(t, "DeleteItem");
  const sources = useMemo(() => (read.table ? sourcesOf(read.table) : []), [read.table]);
  const base = sources[0];
  const source = sources.find((s) => s.index === form.index) ?? base;
  const ranSource = sources.find((s) => s.index === ran.index) ?? base;
  const req = source ? request(form, source) : "Reading the table's keys…";
  const ranReq = ranSource ? request(ran, ranSource) : null;
  const ok = ranReq && typeof ranReq !== "string" ? ranReq : null;
  const start = starts.at(-1);
  const input = ok ? { ...ok.input, Limit: PAGE, ...(start ? { ExclusiveStartKey: start } : {}) } : {};
  const op = ok?.op ?? "Scan";
  const items = useOpQuery<Answer>(t, op, input, ["items", JSON.stringify(input)], ok !== null);
  // The queried source's keys come first (an index's), then the table's own; an item is chosen by the table's.
  const first = ranSource?.keys ?? [];
  const others = base ? base.keys.filter((k) => !first.some((x) => x.name === k.name)) : [];
  const keys = [...first, ...others];
  const rows = items.data?.Items ?? [];
  const id = (r: Item) => JSON.stringify((base?.keys ?? []).map((k) => r[k.name]));
  const chosen = selected.size === 1 ? rows.find((r) => id(r) === [...selected][0]) : undefined;
  const set = (patch: Partial<Form>) => setForm((f) => (MEMORY.set(table, { form: { ...f, ...patch }, ran }), { ...f, ...patch }));
  const run = () => {
    if (typeof req === "string") return;
    MEMORY.set(table, { form, ran: form });
    setRan(form);
    setStarts([]);
    setSelected(new Set());
    if (JSON.stringify(form) === JSON.stringify(ran)) void items.refetch();
  };
  const cols = columnsOf(rows, first, others);
  const columns: DataColumn<Item>[] = cols.map((c, i) => ({
    id: `c${i}`,
    header: () => (
      <span className="grid normal-case">
        <span className="font-mono text-[12.5px] tracking-normal text-foreground">{c.name}</span>
        {c.sub && <span className="text-[11.5px] font-normal tracking-normal">{c.sub}</span>}
      </span>
    ),
    size: i < keys.length ? 200 : undefined,
    min: 110,
    accessorFn: (r) => cell(r[c.name]),
    meta: { mono: true, numeric: c.numeric },
    cell: (x) => {
      const text = cell(x.row.original[c.name]);
      const tone = isStatusColumn(c.name) && "S" in (x.row.original[c.name] ?? {}) ? statusTone(text) : null;
      return tone ? <StatusDot tone={tone} className="font-medium">{text}</StatusDot> : text || <span className="text-muted-foreground">–</span>;
    },
  }));
  if (read.isError && !read.table) return <DetailError service="dynamodb" kind="tables" name={table} error={read.error} onRetry={() => void read.refetch()} />;
  if (!read.table || !base) return <Skeleton className="h-80 rounded-[14px]" />;
  const count = items.data?.Count ?? rows.length;
  const scanned = items.data?.ScannedCount ?? 0;
  const pkWords = ran.mode === "query" && ran.pk ? `${(ranSource?.keys[0]?.name ?? "").replace(/_id$/, "")} ${ran.pk}` : "";
  const lede = !items.data ? undefined : ran.mode === "query" ? (count === 0 ? `Nothing the query found for ${pkWords}.` : `Everything the query found for ${pkWords}${items.data.LastEvaluatedKey ? " so far" : ""}.`) : ran.filter ? `Scanned ${number(scanned)} ${scanned === 1 ? "item" : "items"}; ${count} ${count === 1 ? "matches" : "match"} the filter.` : count === 0 ? "Nothing the scan found." : `Everything the scan read${items.data.LastEvaluatedKey ? " so far" : ""}.`;
  const gone = search.dialog === "item-deleted";
  const banner = search.dialog === "item-saved" ? "Item saved." : search.dialog === "item-created" ? "Item created." : null;
  return (
    <div data-testid="explore-page">
      <PageHeader
        title="Explore table items"
        sub={<><span className="font-mono text-foreground">{table}</span>. Query by key, or scan the whole table.</>}
        crumbs={[...crumbs(table), { label: "Explore table items", to: "" }]}
        actions={
          <Button variant="outline" className="bg-card" onClick={() => go({})} data-testid="back-to-table">
            <ArrowLeftIcon /> <span className="font-mono">{table}</span>
          </Button>
        }
      />
      {(banner || gone) && <ResultBanner className="mb-6" title={banner ?? "Item deleted."} returned={gone ? "DeleteItem removed the item by its primary key." : "PutItem wrote the whole item."} onDismiss={() => go({ do: "explore" })} testId="item-banner" />}
      <div className="grid items-start gap-6 xl:grid-cols-[minmax(0,1fr)_22.5rem]">
        <ExploreForm table={table} form={form} set={set} sources={sources} problem={typeof req === "string" ? req : null} onRun={run} running={items.isFetching} />
        <CallPanel request={typeof req === "string" ? null : { service: "dynamodb", operation: req.op, input: { TableName: table, ...req.input, Limit: PAGE } }} />
      </div>
      <Card className="mt-6" title={items.data ? `Items returned (${number(count)})` : "Items returned"} lede={lede} testId="items-card">
        <div className="grid gap-4">
          <Refused operation={op} error={items.error} />
          {rows.length > 0 && <SelectableTable
            aria-label="Items"
            data={rows}
            columns={columns}
            rowId={id}
            selected={selected}
            onSelectedChange={(s) => setSelected(s.size > 1 ? new Set([[...s].at(-1)!]) : s)}
            noun="item"
            maxHeight={560}
            empty={items.isFetching ? "Reading…" : ""}
            toolbar={
              <>
                <Button variant="outline" className="bg-card" onClick={() => go({ do: "item-create" })} data-testid="item-create">
                  <PlusIcon /> Create item
                </Button>
                {rows.length > 0 && (
                  <>
                    <Button variant="outline" className="bg-card" disabled={!chosen} onClick={() => chosen && go({ do: "item-edit", key: keyParam(keyOf(chosen, base.keys)) })} data-testid="item-edit">
                      <PencilIcon /> Edit item
                    </Button>
                    <Button variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={!chosen} onClick={() => setDeleting(true)} data-testid="item-delete">
                      <Trash2Icon /> Delete
                    </Button>
                  </>
                )}
              </>
            }
          />}
          {rows.length === 0 && (
            <div>
              <Button variant="outline" className="bg-card" onClick={() => go({ do: "item-create" })} data-testid="item-create">
                <PlusIcon /> Create item
              </Button>
            </div>
          )}
          {items.data && rows.length === 0 && (
            <EmptyTab icon={SearchIcon} title="No items returned" testId="items-none">
              {ran.mode === "query" ? "The query found no items for this key. Change the key or the index." : ran.filter ? "No item matches the filter. Change it, or scan without one." : "This table holds no item yet. Create an item to start."}
            </EmptyTab>
          )}
          {rows.length > 0 && (
            <>
              <Said testId="items-note">
                {ran.mode === "query" ? <>A query reads only the partition you name, up to <b>1 MB</b> per call; a result that stops early returns a key to continue from.</> : <>A scan reads at most <b>1 MB</b> per call and returns a key to continue from when there is more.</>}{" "}
                {items.data?.LastEvaluatedKey || starts.length > 0 ? `This is page ${starts.length + 1}, up to ${plural(PAGE, "item")} a page; Next continues from the key the call returned.` : `Here ${ran.mode === "query" ? "the query" : "one call"} found everything, so there is nothing to continue.`} Tick a row to Edit item or Delete.
              </Said>
              {(items.data?.LastEvaluatedKey || starts.length > 0) && (
                <div className="flex items-center justify-end gap-2 text-[13px] text-muted-foreground">
                  <span>Page {starts.length + 1}</span>
                  <Button variant="outline" size="sm" className="bg-card" aria-label="Previous page" disabled={starts.length === 0} onClick={() => setStarts((s) => s.slice(0, -1))} data-testid="items-prev">
                    <ChevronLeftIcon />
                  </Button>
                  <Button variant="outline" size="sm" className="bg-card" aria-label="Next page" disabled={!items.data?.LastEvaluatedKey} onClick={() => items.data?.LastEvaluatedKey && setStarts((s) => [...s, items.data.LastEvaluatedKey as Item])} data-testid="items-next">
                    <ChevronRightIcon />
                  </Button>
                </div>
              )}
            </>
          )}
        </div>
      </Card>
      {deleting && chosen && (
        <Ask
          open
          onOpenChange={(o) => !o && (setDeleting(false), del.reset())}
          title="Delete 1 item?"
          verb="Delete item"
          call="DeleteItem"
          pending={del.isPending}
          error={del.error}
          testId="item-delete-dialog"
          onConfirm={() => del.mutate({ Key: keyOf(chosen, base.keys) }, { onSuccess: () => (setDeleting(false), setSelected(new Set()), go({ do: "explore", dialog: "item-deleted" })) })}
        >
          <p>
            This removes the ticked item from <span className="font-mono">{table}</span> by its primary key:
          </p>
          <dl className="grid grid-cols-[minmax(7rem,10rem)_minmax(0,1fr)] font-mono text-[13px] [&>div:last-child>*]:border-b-0" data-testid="item-key">
            {base.keys.map((k) => (
              <div key={k.name} className="contents">
                <dt className="border-b py-2.5 pr-4 text-muted-foreground">{k.name}</dt>
                <dd className="border-b py-2.5 break-all text-bright">{cell(chosen[k.name])}</dd>
              </div>
            ))}
          </dl>
          <Said>
            {read.table.global.length + read.table.local.length > 0 ? `The item also disappears from ${read.table.global.concat(read.table.local).map((i) => i.name).join(" and ")}. ` : ""}Deleting an item that is already gone is not an error.
          </Said>
        </Ask>
      )}
    </div>
  );
}

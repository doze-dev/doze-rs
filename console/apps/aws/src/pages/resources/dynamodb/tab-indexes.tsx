// A table's Indexes tab (dynamodb__table-indexes, indexes-none, index-created, index-delete): its global secondary indexes with
// Create and Delete, and its local ones, which are only read (a local index is made with the table and never deleted).
import { PlusIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Button } from "@doze/ui/components/ui/button";
import { CallPanel } from "../../../kit";
import { Card } from "../deep/parts";
import { useOp } from "../deep/api";
import { deleteIndexInput } from "./index-model";
import { type IndexInfo, projectionWords, TYPE_WORDS } from "./model";
import { Ask, Said, tableTarget } from "./parts";

const Key = ({ k }: { k: IndexInfo["hash"] | null }) =>
  k ? (
    <span className="grid">
      <span className="font-mono text-[13px]">{k.name}</span>
      <span className="text-[12.5px] text-muted-foreground">{TYPE_WORDS[k.type]}</span>
    </span>
  ) : (
    <span className="text-muted-foreground">–</span>
  );

const status = (s: string) => <StatusDot tone={s === "ACTIVE" ? "ok" : "warn"}>{s.charAt(0) + s.slice(1).toLowerCase()}</StatusDot>;

function columns(onDelete?: (i: IndexInfo) => void): DataColumn<IndexInfo>[] {
  return [
    { id: "name", header: "Index name", accessorFn: (r) => r.name, size: 240, min: 140, meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px] font-medium text-bright">{c.row.original.name}</span> },
    { id: "status", header: "Status", accessorFn: (r) => r.status, size: 160, fit: 90, cell: (c) => status(c.row.original.status) },
    { id: "pk", header: "Partition key", accessorFn: (r) => r.hash.name, size: 220, min: 130, cell: (c) => <Key k={c.row.original.hash} /> },
    { id: "sk", header: "Sort key", accessorFn: (r) => r.range?.name ?? "", size: 220, min: 130, cell: (c) => <Key k={c.row.original.range} /> },
    { id: "projection", header: "Projection", accessorFn: (r) => r.projection, size: 200, min: 100, meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px]">{projectionWords(c.row.original)}</span> },
    ...(onDelete
      ? [{ id: "delete", header: "", size: 120, enableSorting: false, cell: (c: { row: { original: IndexInfo } }) => (
          <Button variant="outline" size="sm" className="border-bad/40 bg-card text-bad hover:text-bad" onClick={() => onDelete(c.row.original)} data-testid={`index-delete-${c.row.original.name}`}>
            <Trash2Icon /> Delete
          </Button>
        ) } satisfies DataColumn<IndexInfo>]
      : []),
  ];
}

export function IndexesTab({ name, global, local, onCreate, onDeleted }: { name: string; global: IndexInfo[]; local: IndexInfo[]; onCreate: () => void; onDeleted: (index: string) => void }) {
  const [asked, setAsked] = useState<IndexInfo | null>(null);
  const update = useOp<unknown>(tableTarget(name), "UpdateTable");
  const create = (
    <Button onClick={onCreate} data-testid="index-create-open">
      <PlusIcon /> Create global secondary index
    </Button>
  );
  if (global.length === 0 && local.length === 0) {
    return (
      <EmptyTab title="No global secondary indexes" action={<Button variant="outline" className="bg-card" onClick={onCreate} data-testid="index-create-open"><PlusIcon /> Create index</Button>} testId="indexes-none">
        {name} is queried by its primary key only. A global secondary index adds another partition key and sort key to query by.
      </EmptyTab>
    );
  }
  return (
    <div className="grid gap-6">
      <Card title="Global secondary indexes" lede="A copy of the table's items under a different key, kept in step by DynamoDB. The columns are fields of GlobalSecondaryIndexes in DescribeTable." testId="global-indexes">
        {global.length === 0 ? (
          <div className="grid justify-items-start gap-3">
            <p className="text-[13.5px] text-muted-foreground">None. {name} has local secondary indexes only.</p>
            {create}
          </div>
        ) : (
          <div className="grid gap-4">
            <div>{create}</div>
            <DataTable aria-label="Global secondary indexes" data={global} columns={columns(setAsked)} rowId={(r) => r.name} />
            <Said>
              <b>20 global secondary indexes</b> per table by default.{global.length > 1 ? ` Every write to ${name} now also updates ${global.length === 2 ? "both indexes" : `the ${global.length} indexes`}.` : ""}
            </Said>
          </div>
        )}
      </Card>
      <Card title="Local secondary indexes" testId="local-indexes">
        {local.length === 0 ? (
          <p className="text-[13.5px] text-muted-foreground">None. A local index shares the table&apos;s partition key and can only be created together with the table; it can never be deleted.</p>
        ) : (
          <DataTable aria-label="Local secondary indexes" data={local} columns={columns()} rowId={(r) => r.name} />
        )}
      </Card>
      {asked && (
        <Ask
          open
          onOpenChange={(o) => {
            if (!o) {
              setAsked(null);
              update.reset();
            }
          }}
          title={`Delete index ${asked.name}?`}
          verb="Delete index"
          call="UpdateTable"
          pending={update.isPending}
          error={update.error}
          testId="index-delete-dialog"
          onConfirm={() => update.mutate(deleteIndexInput(name, asked.name), { onSuccess: () => (setAsked(null), onDeleted(asked.name)) })}
        >
          <p data-testid="index-delete-words">
            The index is removed. The items in <span className="font-mono">{name}</span> are not touched. Queries that name <span className="font-mono">{asked.name}</span> will fail once it is gone.
          </p>
          <CallPanel request={{ service: "dynamodb", operation: "UpdateTable", input: deleteIndexInput(name, asked.name) }} testId="index-delete-call" />
        </Ask>
      )}
    </div>
  );
}

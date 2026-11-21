// The PartiQL editor (dynamodb__partiql, partiql-error): a statement, run with ExecuteStatement, and what it returned as a table or as
// JSON. Choose a table and Query table writes a starting statement. A refused write says nothing was changed, in the service's words.
import { CodeIcon, PlayIcon } from "lucide-react";
import { useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Button } from "@doze/ui/components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Tabs, TabsList, TabsTrigger } from "@doze/ui/components/ui/tabs";
import { Textarea } from "@doze/ui/components/ui/textarea";
import { ApiFailure } from "@doze/ui/lib/api";
import { useResourceList } from "../../../api/resources";
import { CallPanel, Field, useKindOp } from "../../../kit";
import { Card, JsonBlock } from "../deep/parts";
import { type Item, cell, toPlain } from "./json";
import { crumbs, KIND, Said } from "./parts";

type Answer = { Items?: Item[]; NextToken?: string };

/** The statement `Query table` writes for `table`. */
export const startStatement = (table: string): string => `SELECT * FROM "${table}"`;

/** Whether a statement changes an item (its refusal then says nothing was changed). */
export const writes = (statement: string): boolean => /^\s*(INSERT|UPDATE|DELETE)\b/i.test(statement);

export function Partiql() {
  const tables = useResourceList("dynamodb", "tables");
  const names = (tables.data?.pages.flatMap((p) => p.rows) ?? []).map((r) => r.name);
  const [table, setTable] = useState<string | null>(null);
  const [statement, setStatement] = useState("");
  const [ran, setRan] = useState<string | null>(null);
  const [view, setView] = useState<"table" | "json">("table");
  const run = useKindOp<Answer>(KIND, "ExecuteStatement");
  const chosen = table ?? names[0] ?? null;
  const failed = run.error ? errorWords(run.error) : null;
  const code = run.error instanceof ApiFailure ? run.error.code : "";
  const items = run.data?.Items ?? [];
  const names2 = [...new Set(items.flatMap((i) => Object.keys(i)))];
  const columns: DataColumn<Item>[] = names2.map((n) => ({ id: n, header: n, accessorFn: (r) => cell(r[n]), meta: { mono: true }, min: 110, cell: (c) => cell(c.row.original[n]) || <span className="text-muted-foreground">–</span> }));
  const request = statement.trim() ? { service: "dynamodb", operation: "ExecuteStatement", input: { Statement: statement } } : null;
  const submit = () => {
    if (!statement.trim()) return;
    setRan(statement);
    run.mutate({ Statement: statement });
  };
  return (
    <div data-testid="partiql-page">
      <PageHeader title="PartiQL editor" sub="Run SQL-compatible statements. Each INSERT, UPDATE or DELETE touches one item." crumbs={[...crumbs().slice(0, 4), { label: "PartiQL editor", to: "" }]} />
      <div className="grid items-start gap-6 xl:grid-cols-[minmax(0,1fr)_22.5rem]">
        <div className="grid min-w-0 gap-6">
          <Card title="Statement" lede="Choose a table and Query table writes a starting statement; edit it, then Run." testId="statement-card">
            <form className="grid gap-4" onSubmit={(e) => (e.preventDefault(), submit())}>
              <div className="flex flex-wrap items-end gap-3">
                <Field label="Table" className="min-w-64">
                  {(id) => (
                    <Select value={chosen ?? ""} onValueChange={setTable}>
                      <SelectTrigger id={id} className="w-full bg-card font-mono text-[13px]" data-testid="partiql-table">
                        <SelectValue placeholder={tables.isPending ? "Reading tables…" : "No tables"} />
                      </SelectTrigger>
                      <SelectContent>
                        {names.map((n) => (
                          <SelectItem key={n} value={n} className="font-mono text-[13px]">
                            {n}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  )}
                </Field>
                <Button type="button" variant="outline" className="bg-card" disabled={!chosen} onClick={() => chosen && setStatement(startStatement(chosen))} data-testid="partiql-start">
                  <CodeIcon /> Query table
                </Button>
              </div>
              <Textarea value={statement} onChange={(e) => (setStatement(e.target.value), run.reset())} rows={5} spellCheck={false} aria-label="Statement" placeholder={`UPDATE "orders-table" SET status = 'cancelled' WHERE order_id = 'ord-1' AND created_at = '2026-11-08T08:01:00Z'`} className="bg-card font-mono text-[13px] leading-6" data-testid="partiql-statement" />
              <div>
                <Button type="submit" disabled={!statement.trim() || run.isPending} data-testid="partiql-run">
                  <PlayIcon /> {run.isPending ? "Running…" : "Run"}
                </Button>
              </div>
            </form>
          </Card>
          {failed && (
            <Alert tone="bad" title={ran && writes(ran) ? "Nothing was changed." : "The statement was not run."} testId="partiql-error">
              <span className="font-mono text-[13px]">{code || failed.code}</span> {failed.message}
              <span className="mt-2 block text-foreground">Other single-item outcomes: an INSERT of a key that exists returns <span className="font-mono">DuplicateItemException</span>; a DELETE of a missing item succeeds with 0 items deleted. The WHERE clause must name one primary key.</span>
            </Alert>
          )}
          <Card
            title="Results"
            lede="What the statement returns appears here."
            actions={
              <Tabs value={view} onValueChange={(v) => setView(v as "table" | "json")}>
                <TabsList aria-label="View">
                  <TabsTrigger value="table" data-testid="view-table">Table view</TabsTrigger>
                  <TabsTrigger value="json" data-testid="view-json">JSON view</TabsTrigger>
                </TabsList>
              </Tabs>
            }
            testId="results-card"
          >
            {!run.data ? (
              <EmptyTab title="Nothing run yet" testId="partiql-none">Run a statement to see its items in table or JSON view.</EmptyTab>
            ) : items.length === 0 ? (
              <EmptyTab title="The statement ran" testId="partiql-empty">It returned no items{ran && writes(ran) ? ", which is how a write answers" : ""}.</EmptyTab>
            ) : view === "table" ? (
              <DataTable aria-label="Results" data={items} columns={columns} rowId={(r) => JSON.stringify(r)} maxHeight={480} />
            ) : (
              <JsonBlock value={items.map((i) => Object.fromEntries(Object.entries(i).map(([k, v]) => [k, toPlain(v)])))} className="max-h-[480px]" testId="results-json" />
            )}
            {run.data && items.length > 0 && <Said testId="results-count">{items.length} {items.length === 1 ? "item" : "items"} returned.{run.data.NextToken ? " There are more; the console shows the first page." : ""}</Said>}
          </Card>
        </div>
        <CallPanel request={request} refusal={failed ? { code: code || failed.code || "Error", message: failed.message } : null} />
      </div>
    </div>
  );
}

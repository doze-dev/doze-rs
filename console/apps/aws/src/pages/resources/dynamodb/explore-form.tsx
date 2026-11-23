// The form of Explore table items (dynamodb__items-explore, items-scan): Query or Scan, the table or one of its indexes, the partition
// key and sort key condition of a query, or the filter of a scan, and Run. The call it makes is docked at the right by the page.
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Card } from "../deep/parts";
import { Field } from "../../../kit";
import { type Form, SORT_OPS, type SortOp, type Source } from "./request";
import { TYPE_WORDS } from "./model";

const Mono = ({ children }: { children: React.ReactNode }) => <span className="font-mono text-[12.5px] text-foreground">{children}</span>;

/** `orders-table`, `orders-table - by-customer (index)`. */
export const sourceLabel = (table: string, s: Source): string => (s.index ? `${table} - ${s.index} (${s.global ? "global " : "local "}index)` : table);

export function ExploreForm({ table, form, set, sources, problem, onRun, running }: { table: string; form: Form; set: (patch: Partial<Form>) => void; sources: Source[]; problem: string | null; onRun: () => void; running: boolean }) {
  const source = sources.find((s) => s.index === form.index) ?? sources[0];
  const pk = source?.keys.find((k) => k.role === "HASH");
  const sk = source?.keys.find((k) => k.role === "RANGE");
  const inactive = source && source.status !== "ACTIVE";
  return (
    <Card title="Scan or query items" testId="items-form">
      <form onSubmit={(e) => (e.preventDefault(), onRun())} className="grid gap-5">
        <RadioGroup value={form.mode} onValueChange={(v) => set({ mode: v as Form["mode"] })} className="flex gap-8" aria-label="Scan or query">
          {(["query", "scan"] as const).map((m) => (
            <Label key={m} htmlFor={`mode-${m}`} className="flex cursor-pointer items-center gap-3 font-normal text-bright">
              <RadioGroupItem id={`mode-${m}`} value={m} data-testid={`mode-${m}`} />
              {m === "query" ? "Query" : "Scan"}
            </Label>
          ))}
        </RadioGroup>
        <Field label="Table or index" hint={form.mode === "query" ? "A global secondary index can be queried once it is Active." : "A scan reads every item in the table or index, then applies the filter."}>
          {(id) => (
            <Select value={form.index || "-"} onValueChange={(v) => set({ index: v === "-" ? "" : v })}>
              <SelectTrigger id={id} className="w-full bg-card" data-testid="items-source">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {sources.map((s) => (
                  <SelectItem key={s.index || "-"} value={s.index || "-"}>
                    {sourceLabel(table, s)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          )}
        </Field>
        {form.mode === "query" && pk && (
          <div className="grid gap-5 sm:grid-cols-[minmax(0,1fr)_minmax(0,1.1fr)]">
            <Field label="Partition key" hint={<><Mono>{pk.name}</Mono> ({TYPE_WORDS[pk.type]}). Equals only.</>}>
              {(id) => <Input id={id} value={form.pk} onChange={(e) => set({ pk: e.target.value })} className="bg-card font-mono" placeholder={pk.type === "N" ? "42" : "cus-1042"} autoComplete="off" data-testid="q-pk" />}
            </Field>
            {sk && (
              <Field label={<>Sort key condition <span className="font-normal text-muted-foreground">optional</span></>} hint={<><Mono>{sk.name}</Mono> ({TYPE_WORDS[sk.type]}). Also =, &lt;, &lt;=, &gt;, &gt;= and BETWEEN.</>}>
                {(id) => (
                  <div className="flex gap-2">
                    <Select value={form.sortOp} onValueChange={(v) => set({ sortOp: v as SortOp })}>
                      <SelectTrigger aria-label="Sort key condition" className="w-36 flex-none bg-card font-mono text-[13px]" data-testid="q-sortop">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {SORT_OPS.map((o) => (
                          <SelectItem key={o} value={o} className="font-mono text-[13px]">
                            {o === "between" ? "BETWEEN" : o}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                    <Input id={id} value={form.sk} onChange={(e) => set({ sk: e.target.value })} className="bg-card font-mono" autoComplete="off" data-testid="q-sk" />
                    {form.sortOp === "between" && <Input aria-label="and" value={form.sk2} onChange={(e) => set({ sk2: e.target.value })} className="bg-card font-mono" autoComplete="off" data-testid="q-sk2" />}
                  </div>
                )}
              </Field>
            )}
          </div>
        )}
        {form.mode === "scan" && (
          <>
            <Field label={<>FilterExpression <span className="font-normal text-muted-foreground">optional</span></>} hint="Applied after the items are read; the 1 MB limit applies before the filter.">
              {(id) => <Input id={id} value={form.filter} onChange={(e) => set({ filter: e.target.value })} placeholder="#s = :s" className="bg-card font-mono" autoComplete="off" data-testid="s-filter" />}
            </Field>
            <div className="grid gap-5 sm:grid-cols-2">
              <Field label={<>ExpressionAttributeNames <span className="font-normal text-muted-foreground">optional</span></>}>
                {(id) => <Input id={id} value={form.names} onChange={(e) => set({ names: e.target.value })} placeholder='{"#s": "status"}' className="bg-card font-mono" autoComplete="off" data-testid="s-names" />}
              </Field>
              <Field label={<>ExpressionAttributeValues <span className="font-normal text-muted-foreground">optional</span></>}>
                {(id) => <Input id={id} value={form.values} onChange={(e) => set({ values: e.target.value })} placeholder='{":s": {"S": "shipped"}}' className="bg-card font-mono" autoComplete="off" data-testid="s-values" />}
              </Field>
            </div>
          </>
        )}
        {inactive && <p className="text-[13px] text-warn">{form.index} is {source.status.toLowerCase()}: a query on it may not answer yet.</p>}
        {problem && (form.mode === "query" || form.filter) && <p className="text-[13px] text-bad" data-testid="items-problem">{problem}</p>}
        <div>
          <Button type="submit" disabled={Boolean(problem) || running} data-testid="items-run">
            Run
          </Button>
        </div>
      </form>
    </Card>
  );
}

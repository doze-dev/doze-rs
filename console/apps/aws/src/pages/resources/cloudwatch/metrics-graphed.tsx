// The Graphed metrics table (cloudwatch__metrics-graph, widget-line): a row for each metric or expression drawn, its statistic and
// period, and what to do with it. Add math adds an expression over the other rows; the alarm button starts an alarm from a row.
import { BellIcon, CopyIcon, PlusIcon, Trash2Icon } from "lucide-react";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { cn } from "@doze/ui/lib/utils";
import { type GM, nextId } from "./graph-model";
import { PERIODS, periodWords } from "./model";
import { STATS } from "./wizard-model";

export const SWATCH = ["bg-cool", "bg-warn", "bg-good", "bg-refused", "bg-bad"];

const details = (r: GM) => (r.ns ? `${r.ns}${(r.d ?? []).map((x) => ` · ${x.Name}=${x.Value}`).join("")}` : "");

export function GraphedTable({ rows, onChange, onAlarm, math = true }: { rows: GM[]; onChange: (rows: GM[]) => void; onAlarm?: (r: GM) => void; math?: boolean }) {
  const patch = (id: string, p: Partial<GM>) => onChange(rows.map((r) => (r.id === id ? { ...r, ...p } : r)));
  const head = "px-4 py-3 text-left text-[12px] font-medium tracking-[0.08em] text-muted-foreground uppercase";
  return (
    <div className="grid gap-4" data-testid="graphed-metrics">
      <div className="flex flex-wrap items-center gap-3">
        <Button type="button" variant="outline" className="bg-card" disabled={!math || rows.every((r) => r.expr !== undefined)} onClick={() => onChange([...rows, { id: nextId(rows, true), stat: "", period: 0, label: "Expression", shown: true, expr: rows.find((r) => r.expr === undefined)?.id ?? "" }])} data-testid="add-math"><PlusIcon /> Add math</Button>
        <span className="text-[13.5px] text-muted-foreground">An expression works on the rows above by their ids (<span className="font-mono">m1</span>, <span className="font-mono">m2</span>): <span className="font-mono">m1 / m2 * 100</span>, <span className="font-mono">RATE(m1)</span>. doze-aws evaluates metric math.</span>
      </div>
      <div className="overflow-x-auto rounded-[14px] border bg-card">
        <table className="w-full min-w-[56rem] text-[14px]" aria-label="Graphed metrics">
          <thead className="bg-muted/60"><tr><th className="w-12 px-4" /><th className={head}>Label</th><th className={head}>Details</th><th className={head}>Statistic</th><th className={head}>Period</th><th className={head}>Y-axis</th>{onAlarm && <th className={head}>Actions</th>}</tr></thead>
          <tbody>
            {rows.map((r, i) => (
              <tr key={r.id} className="border-t" data-testid={`graphed-${r.id}`}>
                <td className="px-4 py-3"><Checkbox checked={r.shown} onCheckedChange={(v) => patch(r.id, { shown: v === true })} aria-label={`Graph ${r.label}`} data-testid={`graph-${r.id}`} /></td>
                <td className="px-4 py-3"><span className="flex items-center gap-2"><span aria-hidden className={cn("size-2 shrink-0 rounded-full", SWATCH[i % SWATCH.length])} /><span className="font-mono text-[13px] text-bright">{r.expr !== undefined ? <Input value={r.label} onChange={(e) => patch(r.id, { label: e.target.value })} className="h-8 w-40 font-mono" aria-label="Label" /> : r.label}</span></span></td>
                <td className="px-4 py-3 font-mono text-[13px] text-muted-foreground">{r.expr !== undefined ? <span className="flex items-center gap-2"><span className="text-foreground">{r.id}</span><Input value={r.expr} onChange={(e) => patch(r.id, { expr: e.target.value })} className="h-8 w-64 font-mono" aria-label={`Expression ${r.id}`} data-testid={`expr-${r.id}`} /></span> : <span className="break-all">{r.id} · {details(r)}</span>}</td>
                <td className="px-4 py-3">{r.expr !== undefined ? <span className="text-muted-foreground">–</span> : <Select value={r.stat} onValueChange={(stat) => patch(r.id, { stat })}><SelectTrigger className="h-9 w-36 bg-card" aria-label={`Statistic of ${r.label}`} data-testid={`stat-${r.id}`}><SelectValue /></SelectTrigger><SelectContent>{(STATS.includes(r.stat) ? STATS : [r.stat, ...STATS]).map((s) => <SelectItem key={s} value={s}>{s}</SelectItem>)}</SelectContent></Select>}</td>
                <td className="px-4 py-3">{r.expr !== undefined ? <span className="text-muted-foreground">–</span> : <Select value={String(r.period)} onValueChange={(p) => patch(r.id, { period: Number(p) })}><SelectTrigger className="h-9 w-40 bg-card" aria-label={`Period of ${r.label}`} data-testid={`period-${r.id}`}><SelectValue /></SelectTrigger><SelectContent>{(PERIODS.includes(r.period) ? PERIODS : [r.period, ...PERIODS]).map((p) => <SelectItem key={p} value={String(p)}>{periodWords(p)}</SelectItem>)}</SelectContent></Select>}</td>
                <td className="px-4 py-3 text-bright">Left</td>
                {onAlarm && (
                  <td className="px-4 py-3">
                    <span className="flex items-center gap-2">
                      {r.expr === undefined && <Button type="button" variant="outline" size="icon" className="bg-card" aria-label={`Create an alarm from ${r.label}`} title="Create an alarm from this metric, statistic and period" onClick={() => onAlarm(r)} data-testid={`alarm-${r.id}`}><BellIcon /></Button>}
                      {r.expr === undefined && <Button type="button" variant="outline" size="icon" className="bg-card" aria-label={`Duplicate ${r.label}`} title="Duplicate the metric" onClick={() => onChange([...rows, { ...r, id: nextId(rows) }])} data-testid={`copy-${r.id}`}><CopyIcon /></Button>}
                      <Button type="button" variant="outline" size="icon" className="bg-card" aria-label={`Remove ${r.label}`} title="Remove from the graph" onClick={() => onChange(rows.filter((x) => x.id !== r.id))} data-testid={`remove-${r.id}`}><Trash2Icon /></Button>
                    </span>
                  </td>
                )}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="max-w-[46rem] border-l-2 pl-3.5 text-[13px] leading-5 text-muted-foreground" data-testid="graphed-note">
        <b className="font-semibold text-foreground">Statistic:</b> Average, Minimum, Maximum, Sum, SampleCount, or a percentile such as <span className="font-mono">p95.45</span>.{onAlarm && " In Actions, the alarm icon starts an alarm from the metric, statistic and period on this row; the copy icon duplicates the metric."}
      </p>
    </div>
  );
}

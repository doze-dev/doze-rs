import { PlusIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";
import { cn } from "../../lib/utils";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { type KeyValueRow, type KeyValueRules, newRow, validateRows } from "./key-values";

/** Rows of key and value that a person adds to and removes from (tags, message attributes, environment variables). A row's
 * problem (an empty key, a key another row has) is said under it once the person has left the field or tried to save
 * (`showErrors`); until then the row is only a row. The editor owns nothing: the rows are the caller's. */
export function KeyValueEditor({ rows, onChange, rules, addLabel = "Add new tag", keyLabel = "Key", valueLabel = "Value", showErrors = false, disabled = false, className, testId = "kv" }: { rows: KeyValueRow[]; onChange: (rows: KeyValueRow[]) => void; rules?: KeyValueRules; addLabel?: string; keyLabel?: string; valueLabel?: string; showErrors?: boolean; disabled?: boolean; className?: string; testId?: string }) {
  const [touched, setTouched] = useState<ReadonlySet<string>>(new Set());
  const errors = validateRows(rows, rules);
  const full = rules?.maxRows !== undefined && rows.length >= rules.maxRows;
  const set = (id: string, patch: Partial<KeyValueRow>) => onChange(rows.map((r) => (r.id === id ? { ...r, ...patch } : r)));
  const touch = (id: string) => setTouched((t) => new Set(t).add(id));
  return (
    <div className={cn("grid gap-3", className)} data-slot="key-value-editor" data-testid={testId}>
      {rows.length > 0 && (
        <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_2rem] items-start gap-x-3 gap-y-2">
          <span className="text-[12px] font-medium tracking-[0.04em] text-muted-foreground uppercase">{keyLabel}</span>
          <span className="text-[12px] font-medium tracking-[0.04em] text-muted-foreground uppercase">{valueLabel}</span>
          <span />
          {rows.map((r, i) => {
            const found = errors.get(r.id);
            const e = found && (showErrors || touched.has(r.id) || found.live) ? found : undefined;
            return (
              <div key={r.id} className="contents" data-row={i}>
                <div className="grid gap-1">
                  <Input value={r.key} onChange={(ev) => set(r.id, { key: ev.target.value })} onBlur={() => touch(r.id)} aria-label={`${keyLabel} ${i + 1}`} aria-invalid={e?.key ? true : undefined} aria-describedby={e?.key ? `${testId}-${r.id}-key` : undefined} disabled={disabled} autoComplete="off" spellCheck={false} className="font-mono" data-testid={`${testId}-key-${i}`} />
                  {e?.key && (
                    <p id={`${testId}-${r.id}-key`} className="text-[12.5px] text-bad">
                      {e.key}
                    </p>
                  )}
                </div>
                <div className="grid gap-1">
                  <Input value={r.value} onChange={(ev) => set(r.id, { value: ev.target.value })} onBlur={() => touch(r.id)} aria-label={`${valueLabel} ${i + 1}`} aria-invalid={e?.value ? true : undefined} aria-describedby={e?.value ? `${testId}-${r.id}-value` : undefined} disabled={disabled} autoComplete="off" spellCheck={false} className="font-mono" data-testid={`${testId}-value-${i}`} />
                  {e?.value && (
                    <p id={`${testId}-${r.id}-value`} className="text-[12.5px] text-bad">
                      {e.value}
                    </p>
                  )}
                </div>
                <Button type="button" variant="ghost" size="icon-sm" onClick={() => onChange(rows.filter((x) => x.id !== r.id))} disabled={disabled} aria-label={`Remove ${keyLabel.toLowerCase()} ${r.key || i + 1}`} data-testid={`${testId}-remove-${i}`}>
                  <Trash2Icon />
                </Button>
              </div>
            );
          })}
        </div>
      )}
      <div className="flex flex-wrap items-center gap-3">
        <Button type="button" variant="outline" className="bg-card" onClick={() => onChange([...rows, newRow()])} disabled={disabled || full} data-testid={`${testId}-add`}>
          <PlusIcon /> {addLabel}
        </Button>
        {full && <span className="text-[12.5px] text-muted-foreground">The most is {rules?.maxRows}.</span>}
      </div>
    </div>
  );
}

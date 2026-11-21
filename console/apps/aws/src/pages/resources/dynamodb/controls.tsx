// The controls the DynamoDB forms share: a key attribute (a name and its type), choices drawn as two cards or as rows, and the
// attribute-type select.
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { cn } from "@doze/ui/lib/utils";
import { type KeyType, TYPE_WORDS } from "./model";

type Option<T extends string> = { value: T; title: React.ReactNode; description?: React.ReactNode; disabled?: boolean };

/** Choices as cards side by side, the radio inside each (the boards' Default settings and Customize settings). */
export function CardChoice<T extends string>({ value, onChange, options, name }: { value: T; onChange: (v: T) => void; options: Option<T>[]; name: string }) {
  return (
    <RadioGroup value={value} onValueChange={(v) => onChange(v as T)} className="grid gap-4 sm:grid-cols-2" aria-label={name}>
      {options.map((o) => (
        <Label key={o.value} htmlFor={`${name}-${o.value}`} className={cn("flex cursor-pointer items-start gap-3 rounded-xl border bg-card p-4 font-normal leading-5 has-[[data-state=checked]]:border-primary has-[[data-state=checked]]:bg-pick/40 has-[[data-state=checked]]:ring-[3px] has-[[data-state=checked]]:ring-primary/10", o.disabled && "cursor-not-allowed opacity-60")}>
          <RadioGroupItem id={`${name}-${o.value}`} value={o.value} disabled={o.disabled} className="mt-0.5" data-testid={`${name}-${o.value}`} />
          <span className="grid gap-0.5">
            <span className="font-medium text-bright">{o.title}</span>
            {o.description && <span className="text-[13px] text-muted-foreground">{o.description}</span>}
          </span>
        </Label>
      ))}
    </RadioGroup>
  );
}

/** Choices as rows: a radio, a title and a sentence, with no box (the boards' Encryption at rest and Projection). */
export function RowChoice<T extends string>({ value, onChange, options, name, children }: { value: T; onChange: (v: T) => void; options: Option<T>[]; name: string; /** Drawn under the row of this value when it is chosen. */ children?: Partial<Record<T, React.ReactNode>> }) {
  return (
    <RadioGroup value={value} onValueChange={(v) => onChange(v as T)} className="grid gap-4" aria-label={name}>
      {options.map((o) => (
        <div key={o.value} className="grid gap-2.5">
          <Label htmlFor={`${name}-${o.value}`} className={cn("flex cursor-pointer items-start gap-3 font-normal leading-5", o.disabled && "cursor-not-allowed opacity-60")}>
            <RadioGroupItem id={`${name}-${o.value}`} value={o.value} disabled={o.disabled} className="mt-0.5" data-testid={`${name}-${o.value}`} />
            <span className="grid gap-0.5">
              <span className="font-medium text-bright">{o.title}</span>
              {o.description && <span className="text-[13px] text-muted-foreground">{o.description}</span>}
            </span>
          </Label>
          {value === o.value && children?.[o.value] && <div className="pl-7">{children[o.value]}</div>}
        </div>
      ))}
    </RadioGroup>
  );
}

/** String, Number or Binary. */
export function TypeSelect({ value, onChange, label, id, disabled, testId }: { value: KeyType; onChange: (v: KeyType) => void; label: string; id?: string; disabled?: boolean; testId?: string }) {
  return (
    <Select value={value} onValueChange={(v) => onChange(v as KeyType)} disabled={disabled}>
      <SelectTrigger id={id} aria-label={label} className="w-full bg-card" data-testid={testId}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {(Object.keys(TYPE_WORDS) as KeyType[]).map((t) => (
          <SelectItem key={t} value={t}>
            {TYPE_WORDS[t]}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

/** An attribute name beside its type, on one row (a table's keys, an index's). */
export function KeyRow({ name, type, onName, onType, placeholder, label, testId, nameId, disabled }: { name: string; type: KeyType; onName: (v: string) => void; onType: (v: KeyType) => void; placeholder: string; label: string; testId: string; nameId?: string; disabled?: boolean }) {
  return (
    <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_14rem]">
      <Input id={nameId} value={name} onChange={(e) => onName(e.target.value)} placeholder={placeholder} autoComplete="off" spellCheck={false} disabled={disabled} className="bg-card font-mono" aria-label={`${label} name`} data-testid={testId} />
      <TypeSelect value={type} onChange={onType} label={`${label} type`} disabled={disabled} testId={`${testId}-type`} />
    </div>
  );
}

// What the pages of KMS, Secrets Manager and Parameter Store share: a titled card, a note set apart by a rule, label and value rows,
// a sentence with its names set in type, and the reading of a list's columns by title.
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { Badge } from "@doze/ui/components/ui/badge";
import { Label } from "@doze/ui/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { cn } from "@doze/ui/lib/utils";
import type { ColumnSpec } from "../../api/types/ColumnSpec";
import type { ResourceRow } from "../../api/types/ResourceRow";

/** A card of a page: its title and what it is (`lede`), the card's own action at the right of the title, and the content. */
export function Card({ title, lede, actions, children, className, testId }: { title: string; lede?: React.ReactNode; actions?: React.ReactNode; children?: React.ReactNode; className?: string; testId?: string }) {
  return (
    <section className={cn("rounded-[14px] border bg-card p-6 lg:p-7", className)} aria-label={title} data-testid={testId}>
      <div className="flex flex-wrap items-start justify-between gap-x-6 gap-y-3">
        <div className="max-w-[48rem] min-w-0">
          <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">{title}</h2>
          {lede && <p className="mt-1.5 text-[13.5px] leading-[21px] text-muted-foreground">{lede}</p>}
        </div>
        {actions && <div className="flex shrink-0 flex-wrap items-center gap-2">{actions}</div>}
      </div>
      {children && <div className="mt-5 grid gap-4">{children}</div>}
    </section>
  );
}

/** A note set apart by a rule at its left (what a setting does here). Bold what is a limit with `<b>`. */
export function Note({ children, className, testId }: { children: React.ReactNode; className?: string; testId?: string }) {
  return (
    <div className={cn("max-w-[46rem] border-l-2 pl-3.5 text-[13.5px] leading-[21px] text-muted-foreground [&_b]:font-semibold [&_b]:text-foreground", className)} data-testid={testId}>
      {children}
    </div>
  );
}

export type Fact = { label: string; value: React.ReactNode; testId?: string };

/** Label and value rows, in the card's one or two columns. */
export function Facts({ facts, columns = 1 }: { facts: Fact[]; columns?: 1 | 2 }) {
  const half = columns === 2 ? Math.ceil(facts.length / 2) : facts.length;
  const col = (rows: Fact[]) => (
    <dl className="grid grid-cols-[minmax(8rem,12rem)_minmax(0,1fr)] content-start [&>div:last-child>*]:border-b-0">
      {rows.map((f) => (
        <div key={f.label} className="contents">
          <dt className="border-b py-3 pr-4 text-[13.5px] text-muted-foreground">{f.label}</dt>
          <dd className="min-w-0 border-b py-3 text-[13.5px] break-words text-bright" data-testid={f.testId ?? `fact-${f.label}`}>{f.value}</dd>
        </div>
      ))}
    </dl>
  );
  return <div className={cn("grid", columns === 2 && "lg:grid-cols-2 lg:gap-x-12")}>{col(facts.slice(0, half))}{columns === 2 && col(facts.slice(half))}</div>;
}

/** A sentence whose resource names are in monospace and whose figures (a pattern or a phrase) are in bold. */
export function Say({ text, names, bold, className, testId }: { text: string; names?: readonly string[]; bold?: readonly (string | RegExp)[]; className?: string; testId?: string }) {
  return (
    <p className={cn("max-w-[62rem] text-[15px] leading-6 text-muted-foreground", className)} data-testid={testId}>
      <Emphasis text={text} names={names} bold={bold} />
    </p>
  );
}

/** The cell of `row` under the column titled `title`; `null` when the list has no such column or the cell has no value. */
export function cell(columns: readonly ColumnSpec[], row: ResourceRow, title: string): string | null {
  const i = columns.findIndex((c) => c.title === title);
  return i < 0 ? null : (row.cells[i] ?? null);
}

/** `a`, `a and b`, `a, b and c`. */
export function andList(items: readonly string[]): string {
  if (items.length <= 1) return items.join("");
  return `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`;
}

export type Choice<T extends string> = { value: T; title: React.ReactNode; body?: React.ReactNode; /** Drawn, not chosen: the words say why. */ off?: string };

/** Choices as rows, one chosen: a radio, its title and what it means under it. One that cannot be chosen (`off`) says so in a badge. */
export function RowChoice<T extends string>({ value, onChange, options, label, name, className }: { value: T; onChange: (v: T) => void; options: readonly Choice<T>[]; label: string; name: string; className?: string }) {
  return (
    <RadioGroup value={value} onValueChange={(v) => onChange(v as T)} aria-label={label} className={cn("gap-5", className)}>
      {options.map((o) => (
        <div key={o.value} className={cn("flex items-start gap-3", o.off && "opacity-70")}>
          <RadioGroupItem id={`${name}-${o.value}`} value={o.value} disabled={Boolean(o.off)} className="mt-1" data-testid={`${name}-${o.value}`} />
          <Label htmlFor={`${name}-${o.value}`} className={cn("grid gap-0.5 font-normal leading-5", o.off ? "cursor-not-allowed" : "cursor-pointer")}>
            <span className="flex flex-wrap items-center gap-2 font-medium text-bright">
              {o.title}
              {o.off && <Badge variant="secondary" className="font-normal text-muted-foreground">Not available</Badge>}
            </span>
            {(o.off ?? o.body) && <span className="text-[13px] text-muted-foreground">{o.off ?? o.body}</span>}
          </Label>
        </div>
      ))}
    </RadioGroup>
  );
}

/** The value of the field of a resource's detail labelled `label`; `null` when it has none. */
export function fieldOf(d: { fields: readonly { label: string; value: string | null }[] }, label: string): string | null {
  return d.fields.find((f) => f.label === label)?.value ?? null;
}

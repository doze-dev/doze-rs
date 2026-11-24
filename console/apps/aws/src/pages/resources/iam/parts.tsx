// What the IAM pages share: the sentence with its names in bold, a titled card, the key-value summary, a segmented control, the
// radio rows and cards of the wizards, the crumbs, and the table of policies.
import { Link } from "@tanstack/react-router";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { Button } from "@doze/ui/components/ui/button";
import { Label } from "@doze/ui/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { ToggleGroup, ToggleGroupItem } from "@doze/ui/components/ui/toggle-group";
import { cn } from "@doze/ui/lib/utils";
import type { ValueFormat } from "../../../api/types/ValueFormat";
import { shown } from "../model";
import { type AttachedPolicy, breakPoints, managedKind } from "./model";

/** An ARN (or any resource text) that wraps at its colons and slashes, and breaks inside a part only when the part alone is wider than the cell. */
export function Breakable({ text, className }: { text: string; className?: string }) {
  return (
    <span className={cn("font-mono text-[13px] [overflow-wrap:anywhere]", className)} title={text}>
      {breakPoints(text).map((s, i) => <span key={i}>{s}<wbr /></span>)}
    </span>
  );
}

/** A sentence whose resource names are in monospace bold and whose figures (a pattern or a phrase) are in bold. */
export function Say({ text, names, bold, className, testId }: { text: string; names?: readonly string[]; bold?: readonly (string | RegExp)[]; className?: string; testId?: string }) {
  return (
    <p className={cn("max-w-[62rem] text-[15px] leading-6 text-muted-foreground", className)} data-testid={testId}>
      <Emphasis text={text} names={names} bold={bold} />
    </p>
  );
}

/** A card of a page: its title and what it is (`lede`), the card's own action at the right of the title, and the content. */
export function Card({ title, lede, actions, children, className, testId }: { title: string; lede?: React.ReactNode; actions?: React.ReactNode; children?: React.ReactNode; className?: string; testId?: string }) {
  return (
    <section className={cn("rounded-[var(--radius-card)] border bg-card p-6 lg:p-7", className)} aria-label={title} data-testid={testId}>
      <div className="flex flex-wrap items-start justify-between gap-x-6 gap-y-3">
        <div className="min-w-0 max-w-[48rem]">
          <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">{title}</h2>
          {lede && <p className="mt-1.5 text-[13.5px] leading-[21px] text-muted-foreground">{lede}</p>}
        </div>
        {actions && <div className="flex shrink-0 flex-wrap items-center gap-2">{actions}</div>}
      </div>
      {children && <div className="mt-5 grid gap-4">{children}</div>}
    </section>
  );
}

/** A note set apart by a rule at its left (what a setting does here, what is stored and not applied). Bold what is a limit with `<b>`. */
export function Note({ children, className, testId }: { children: React.ReactNode; className?: string; testId?: string }) {
  return (
    <div className={cn("max-w-[46rem] border-l-2 pl-3.5 text-[13.5px] leading-[21px] text-foreground [&_.dim]:text-muted-foreground [&_b]:font-semibold", className)} data-testid={testId}>
      {children}
    </div>
  );
}

export type Fact = { label: string; value: React.ReactNode; testId?: string };

/** The Summary card: label and value rows, in the two columns of the boards that draw them so. */
export function Summary({ facts, title = "Summary", columns = 1 }: { facts: Fact[]; title?: string; columns?: 1 | 2 }) {
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
  return (
    <section className="mb-10 rounded-[var(--radius-card)] border bg-card p-6 lg:p-7" aria-label={title} data-testid="summary">
      <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">{title}</h2>
      <div className={cn("mt-4 grid", columns === 2 && "lg:grid-cols-2 lg:gap-x-12")}>
        {col(facts.slice(0, half))}
        {columns === 2 && col(facts.slice(half))}
      </div>
    </section>
  );
}

/** Two or three choices side by side in one box, one on at a time (the board's AWS managed / Customer managed). */
export function Segmented<T extends string>({ value, onChange, options, label, testId }: { value: T | ""; onChange: (v: T | "") => void; options: { value: T; label: string; /** A colour dot before the label (`bg-good`). */ dot?: string }[]; label: string; testId?: string }) {
  return (
    <ToggleGroup type="single" value={value} onValueChange={(v) => onChange(v as T | "")} aria-label={label} className="rounded-[var(--radius-field)] border bg-muted p-1" data-testid={testId}>
      {options.map((o) => (
        <ToggleGroupItem key={o.value} value={o.value} aria-label={o.label} className="h-8 rounded-[7px] px-4 text-[13.5px] font-normal data-[state=on]:bg-card data-[state=on]:font-medium data-[state=on]:text-bright data-[state=on]:shadow-sm" data-testid={`${testId ?? "segment"}-${o.value}`}>
          {o.dot && <span aria-hidden className={cn("mr-2 size-2 rounded-full", o.dot)} />}
          {o.label}
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  );
}

type Option<T extends string> = { value: T; title: React.ReactNode; description?: React.ReactNode; disabled?: boolean; /** A grid class for the card (`sm:col-span-2`). */ span?: string };

/** Choices as cards side by side, the radio inside each. */
export function CardChoice<T extends string>({ value, onChange, options, name, className }: { value: T; onChange: (v: T) => void; options: Option<T>[]; name: string; className?: string }) {
  return (
    <RadioGroup value={value} onValueChange={(v) => onChange(v as T)} className={cn("grid gap-4", className ?? "sm:grid-cols-3")} aria-label={name}>
      {options.map((o) => (
        <Label key={o.value} htmlFor={`${name}-${o.value}`} className={cn("flex cursor-pointer items-start gap-3 rounded-xl border bg-card p-4 font-normal leading-5 has-[[data-state=checked]]:border-primary has-[[data-state=checked]]:bg-pick/40 has-[[data-state=checked]]:ring-[3px] has-[[data-state=checked]]:ring-primary/10", o.disabled && "cursor-not-allowed bg-muted/40 opacity-60", o.span)}>
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

/** Choices as rows: a radio, a title and a sentence, with no box. */
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

/** Resources, Identity, IAM, then the kind and the page on screen. */
export function crumbs(kind?: { id: string; title: string }, name?: string, last?: string): { label: string; to: string }[] {
  const base = [
    { label: "Resources", to: "/resources" },
    { label: "Identity", to: "/resources" },
    { label: "IAM", to: "/resources/iam" },
  ];
  if (!kind) return base;
  const k = [...base, { label: kind.title, to: `/resources/iam/${kind.id}` }];
  const one = name ? [...k, { label: name, to: `/resources/iam/${kind.id}/${encodeURIComponent(name)}` }] : k;
  return last ? [...one, { label: last, to: "" }] : one;
}

export const KINDS = {
  users: { id: "users", title: "Users" },
  groups: { id: "groups", title: "User groups" },
  roles: { id: "roles", title: "Roles" },
  policies: { id: "policies", title: "Policies" },
} as const;

/** A link to an IAM resource, in monospace. */
export function To({ kind, name, label, className }: { kind: keyof typeof KINDS; name: string; label?: React.ReactNode; className?: string }) {
  return (
    <Link to="/resources/$service/$kind/$name" params={{ service: "iam", kind, name }} className={cn("rounded-sm font-mono text-[13px] font-medium text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none", className)} data-testid="resource-link">
      {label ?? name}
    </Link>
  );
}

export type PolicyRow = AttachedPolicy & { via?: React.ReactNode; entities?: number | null };

/** The policies attached to something: the name (a link for a customer managed one), the type, and what else the board gives it
 * (`ATTACHED VIA`, `ATTACHED ENTITIES`). */
export function PolicyTable({ rows, via = false, entities = false, label = "Permissions policies" }: { rows: PolicyRow[]; via?: boolean; entities?: boolean; label?: string }) {
  const columns: DataColumn<PolicyRow>[] = [
    { id: "name", header: "Policy name", accessorFn: (r) => r.PolicyName, size: 340, min: 200, cell: (c) => <PolicyLink policy={c.row.original} /> },
    { id: "type", header: "Type", size: 200, accessorFn: (r) => managedKind(r.PolicyArn), cell: (c) => managedKind(c.row.original.PolicyArn) },
    ...(via ? [{ id: "via", header: "Attached via", accessorFn: (r: PolicyRow) => String(r.via ?? ""), cell: (c: { row: { original: PolicyRow } }) => c.row.original.via }] : []),
    ...(entities ? [{ id: "entities", header: "Attached entities", size: 220, accessorFn: (r: PolicyRow) => r.entities ?? 0, cell: (c: { row: { original: PolicyRow } }) => c.row.original.entities ?? "–" }] : []),
  ];
  return <DataTable aria-label={label} data={rows} columns={columns} rowId={(r) => `${r.PolicyArn}|${String(r.via ?? "")}`} maxHeight={420} />;
}

/** A policy's name: a link for a customer managed policy (its page), plain for an AWS managed one (IAM lists it only once something holds it). */
export function PolicyLink({ policy }: { policy: AttachedPolicy }) {
  return <To kind="policies" name={policy.PolicyArn} label={policy.PolicyName} className="text-[13px]" />;
}

/** What a resource's detail read says at `label` (`ARN`, `Created`), `null` when it said nothing. */
export const fieldOf = (d: { fields: readonly { label: string; value: string | null; format: ValueFormat }[] }, label: string): string | null => {
  const f = d.fields.find((x) => x.label === label);
  return f?.value ? shown(f.value, f.format) : null;
};

/** The words of a resource that is not there: not a red box but what is true (no such user) and the way to the ones that are. */
export function NotThere({ kind, name }: { kind: keyof typeof KINDS; name: string }) {
  const noun = { users: "user", groups: "user group", roles: "role", policies: "policy" }[kind];
  return (
    <div className="grid max-w-[40rem] gap-3 rounded-[var(--radius-card)] border border-dashed bg-card/60 px-8 py-10" data-testid="not-there">
      <h2 className="text-[17px] font-semibold text-bright">No {noun} named <span className="font-mono">{name}</span></h2>
      <p className="text-[13.5px] leading-[21px] text-muted-foreground">IAM answered NoSuchEntity: there is no {noun} of that name in this account. It may have been deleted, or the address is mistyped.</p>
      <p><Link to="/resources/$service/$second" params={{ service: "iam", second: kind }} search={{}} className="text-[13.5px] font-medium text-link hover:underline">See the {KINDS[kind].title.toLowerCase()}</Link></p>
    </div>
  );
}

/** A block of the review step: its title, the Edit that goes back to the step, and what that step chose. */
export function ReviewBlock({ title, onEdit, children, testId }: { title: string; onEdit?: () => void; children: React.ReactNode; testId?: string }) {
  return (
    <section className="min-w-0 rounded-[var(--radius-card)] border bg-card p-6 lg:p-7" aria-label={title} data-testid={testId}>
      <div className="flex items-start justify-between gap-3">
        <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">{title}</h2>
        {onEdit && <Button type="button" variant="outline" size="sm" className="bg-card" onClick={onEdit} data-testid={`review-edit-${testId ?? title}`}>Edit</Button>}
      </div>
      <div className="mt-3 grid gap-3">{children}</div>
    </section>
  );
}

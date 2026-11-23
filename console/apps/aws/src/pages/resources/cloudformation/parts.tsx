// What the CloudFormation pages share: a titled section, a sentence with its names set apart, a card of facts, a status, the events
// table, and the links between a stack and the resources it owns.
import { Link } from "@tanstack/react-router";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { StatusDot, type Tone } from "@doze/ui/components/app/status-dot";
import { cn } from "@doze/ui/lib/utils";
import type { BadgeTone, DetailBadge } from "../../../kit";
import { at } from "../deep/parts";
import { ResourceTo } from "../value";
import { type StackEvent, stackTone } from "./model";
import type { Said } from "./sentences";

/** A titled part of a page: the title and what it is for at the left, the part's own action at the right on the title's row. */
export function Section({ title, lede, actions, children, testId, className }: { title: string; lede?: React.ReactNode; actions?: React.ReactNode; children?: React.ReactNode; testId?: string; className?: string }) {
  return (
    <section className={cn("grid min-w-0 gap-3", className)} aria-label={title} data-testid={testId}>
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div className="min-w-0 max-w-[46rem]">
          <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">{title}</h2>
          {lede && <p className="mt-1 text-[13.5px] text-muted-foreground">{lede}</p>}
        </div>
        {actions && <div className="flex shrink-0 flex-wrap items-center gap-2">{actions}</div>}
      </div>
      {children}
    </section>
  );
}

/** A sentence whose resource names and figures are in bold type. */
export function Lede({ said, className, testId = "stack-sentence", bold }: { said: Said; className?: string; testId?: string; bold?: (string | RegExp)[] }) {
  return (
    <p className={cn("mb-6 max-w-[62rem] text-[15px] leading-6 text-muted-foreground", className)} data-testid={testId}>
      <Emphasis text={said.text} names={said.names} bold={bold ?? [/\b\d[\d,]*(?= resources?| services?| changes?| exports?)/, /\b[A-Z][A-Z_]{6,}\b/]} />
    </p>
  );
}

export type Fact = { label: string; value: React.ReactNode; mono?: boolean };

function FactRow({ label, value, mono }: Fact) {
  return (
    <div className="flex gap-6 border-b py-3 text-[13.5px]">
      <dt className="w-36 shrink-0 text-muted-foreground">{label}</dt>
      <dd className={cn("min-w-0 flex-1 break-words text-bright", mono && "font-mono text-[13px]")} data-testid={`field-${label}`}>{value}</dd>
    </div>
  );
}

/** A card of facts, label then value: the `wide` ones across the card, the others in two columns under them. */
export function Facts({ title, lede, wide = [], rows, testId, children, className }: { title: string; lede?: React.ReactNode; wide?: Fact[]; rows: Fact[]; testId?: string; children?: React.ReactNode; className?: string }) {
  return (
    <section className={cn("min-w-0 rounded-[14px] border bg-card p-6 lg:p-7", className)} aria-label={title} data-testid={testId}>
      <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">{title}</h2>
      {lede && <p className="mt-1 text-[13.5px] text-muted-foreground">{lede}</p>}
      {wide.length > 0 && <dl className="mt-3">{wide.map((r) => <FactRow key={r.label} {...r} />)}</dl>}
      <dl className={cn("grid gap-x-12 lg:grid-cols-2", wide.length === 0 && "mt-3")}>{rows.map((r) => <FactRow key={r.label} {...r} />)}</dl>
      {children}
    </section>
  );
}

export const TONES: Record<Tone, BadgeTone> = { ok: "good", warn: "warn", bad: "bad", idle: "neutral", info: "info" };

/** The badge of a status word, as the stack's header draws it. */
export const statusBadge = (status: string): DetailBadge => ({ text: status, tone: TONES[stackTone(status)] });

/** A status as a coloured dot and its word. */
export const Status = ({ status, wrap = false }: { status: string; /** A long word (`UPDATE_COMPLETE_CLEANUP_IN_PROGRESS`) may break in a narrow cell instead of being cut. */ wrap?: boolean }) => <StatusDot tone={stackTone(status)} className={wrap ? "items-start whitespace-normal [&>span:first-child]:mt-1.5 [overflow-wrap:anywhere]" : undefined}>{wrap ? status.split("_").flatMap((w, i, all) => (i < all.length - 1 ? [w, "_", <wbr key={i} />] : [w])) : status}</StatusDot>;

const eventColumns: DataColumn<StackEvent>[] = [
  { id: "t", header: "Timestamp", size: 240, accessorFn: (r) => String(r.Timestamp), cell: (c) => <span className="font-mono text-[13px] tabular-nums">{at(c.row.original.Timestamp)}</span> },
  {
    id: "l",
    header: "Logical ID",
    size: 300,
    accessorFn: (r) => r.LogicalResourceId ?? "",
    cell: (c) => (
      <span className="grid leading-5">
        <span className="font-mono text-[13px] font-medium text-bright">{c.row.original.LogicalResourceId}</span>
        <span className="font-mono text-[12px] text-muted-foreground">{c.row.original.ResourceType}</span>
      </span>
    ),
  },
  { id: "s", header: "Status", size: 300, accessorFn: (r) => r.ResourceStatus ?? "", cell: (c) => <Status status={c.row.original.ResourceStatus ?? ""} wrap /> },
  { id: "r", header: "Status reason", accessorFn: (r) => r.ResourceStatusReason ?? "", cell: (c) => <span className="text-[13px] break-words whitespace-normal">{c.row.original.ResourceStatusReason || <span className="text-muted-foreground">–</span>}</span> },
];

/** The events as a table, newest first. */
export function EventsTable({ events, maxHeight = 100_000 }: { events: StackEvent[]; maxHeight?: number }) {
  return <DataTable aria-label="Stack events" data={events} columns={eventColumns} rowId={(r) => r.EventId} rowHeight={60} maxHeight={maxHeight} empty="No events." />;
}

/** A link to a stack's page. */
export function StackTo({ name, children, className }: { name: string; children?: React.ReactNode; className?: string }) {
  return <ResourceTo to={{ service: "cloudformation", kind: "stacks", name }} className={cn("font-mono text-[13px] font-medium", className)}>{children ?? name}</ResourceTo>;
}

/** Back to the list of stacks, as a button's link. */
export function ToStacks({ children = "Back to stacks" }: { children?: React.ReactNode }) {
  return <Link to="/resources/$service/$second" params={{ service: "cloudformation", second: "stacks" }} search={{}} className="inline-flex h-9 items-center rounded-md border bg-card px-4 text-[14px] font-medium text-bright hover:bg-accent focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none" data-testid="back-to-stacks">{children}</Link>;
}

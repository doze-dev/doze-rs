// What the API Gateway pages share: a sentence with its names in bold, the chip navigation of the boards (`ACCOUNT  APIs  Usage plans`,
// `LARDER-PUBLIC  Resources  Stages  Authorizers`), a section's heading, the details card, and where the console is.
import { Link } from "@tanstack/react-router";
import { ChartNoAxesColumnIcon, Share2Icon } from "lucide-react";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { Badge } from "@doze/ui/components/ui/badge";
import { cn } from "@doze/ui/lib/utils";
import { useAbout } from "../../../api/queries";

/** The Region and account the console is for, and where SDKs reach doze-aws (what an ARN and a local URL are made of). */
export function useWhere(): { region: string; account: string; endpoint: string } {
  const about = useAbout().data;
  return { region: about?.region ?? "us-east-1", account: about?.account ?? "000000000000", endpoint: about?.endpoint ?? "http://127.0.0.1:4566" };
}

/** The line under a title: names (monospace, bold) and figures (bold) set apart. */
export function Said({ text, names = [], bold, className, testId = "list-sentence" }: { text: string; names?: readonly string[]; bold?: readonly (string | RegExp)[]; className?: string; testId?: string }) {
  if (text === "") return null;
  return (
    <p className={cn("mb-8 max-w-[62rem] text-[15px] leading-6 text-muted-foreground", className)} data-testid={testId}>
      <Emphasis text={text} names={names} bold={bold} />
    </p>
  );
}

/** A section's heading and its one line. */
export function Heading({ title, children, id, className }: { title: string; children?: React.ReactNode; id?: string; className?: string }) {
  return (
    <div className={cn("mb-4", className)}>
      <h2 id={id} className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">{title}</h2>
      {children && <p className="mt-1 text-[13.5px] text-muted-foreground">{children}</p>}
    </div>
  );
}

/** The small capitals over a table (`ROUTES`), with the section's button at the right. */
export function Caption({ title, action, className }: { title: string; action?: React.ReactNode; className?: string }) {
  return (
    <div className={cn("mb-3 flex min-h-9 flex-wrap items-center justify-between gap-3", className)}>
      <h2 className="text-[12px] font-medium tracking-[0.08em] text-muted-foreground uppercase">{title}</h2>
      {action}
    </div>
  );
}

export type Chip = { icon?: React.ReactNode; label: string; to: { kind: string; name?: string; search?: Record<string, string | undefined> }; id: string };

/** The chip navigation of a page's sections: a small-capitals label, then one chip per section, the one on screen marked. */
export function ChipNav({ label, chips, current, testId = "section-chips" }: { label: string; chips: readonly Chip[]; current: string; testId?: string }) {
  return (
    <nav aria-label={`${label} sections`} className="mb-6 flex flex-wrap items-center gap-2.5" data-testid={testId}>
      <span className="mr-1 text-[12px] font-medium tracking-[0.08em] text-muted-foreground uppercase">{label}</span>
      {chips.map((c) => {
        const on = c.id === current;
        const cls = cn("inline-flex h-9 items-center gap-2 rounded-full border px-4 text-[14px] outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50", on ? "border-primary/30 bg-pick font-medium text-bright" : "bg-card text-foreground hover:bg-accent");
        return c.to.name ? (
          <Link key={c.id} to="/resources/$service/$kind/$name" params={{ service: "apigateway", kind: c.to.kind, name: c.to.name }} search={(c.to.search ?? {}) as never} aria-current={on ? "page" : undefined} className={cls} data-testid={`chip-${c.id}`}>{c.icon}{c.label}</Link>
        ) : (
          <Link key={c.id} to="/resources/$service/$second" params={{ service: "apigateway", second: c.to.kind }} search={(c.to.search ?? {}) as never} aria-current={on ? "page" : undefined} className={cls} data-testid={`chip-${c.id}`}>{c.icon}{c.label}</Link>
        );
      })}
    </nav>
  );
}

/** `ACCOUNT  APIs  Usage plans`. */
export function AccountNav({ current }: { current: "apis" | "usageplans" }) {
  return <ChipNav label="Account" current={current} testId="account-nav" chips={[{ id: "apis", label: "APIs", icon: <Share2Icon aria-hidden className="size-4" />, to: { kind: "restapis" } }, { id: "usageplans", label: "Usage plans", icon: <ChartNoAxesColumnIcon aria-hidden className="size-4" />, to: { kind: "usageplans" } }]} />;
}

/** What an API is, beside its name in the header: its name, its kind and where it is reachable from. */
export function NameChips({ name, kind, extra }: { name: string; kind: string; extra?: string }) {
  return (
    <span className="flex flex-wrap items-center gap-2" data-testid="api-chips">
      <Badge variant="secondary" className="font-mono text-[12px] font-normal" data-testid="chip-name">{name}</Badge>
      <Badge variant="secondary" className="text-[12px] font-normal" data-testid="chip-kind">{kind}</Badge>
      {extra && <Badge variant="secondary" className="text-[12px] font-normal" data-testid="chip-extra">{extra}</Badge>}
    </span>
  );
}

/** A card of rows: a label, and a value with what is said under it. */
export function Rows({ rows, testId = "details" }: { rows: readonly { label: string; value: React.ReactNode; note?: React.ReactNode }[]; testId?: string }) {
  return (
    <dl className="grid grid-cols-[minmax(8rem,12rem)_minmax(0,1fr)] [&>div:last-child>*]:border-b-0" data-testid={testId}>
      {rows.map((r) => (
        <div key={r.label} className="contents">
          <dt className="border-b py-3.5 pr-4 text-[14px] text-muted-foreground">{r.label}</dt>
          <dd className="min-w-0 border-b py-3.5 text-[14px] break-words text-bright" data-testid={`field-${r.label}`}>
            {r.value}
            {r.note && <div className="mt-0.5 text-[12.5px] text-muted-foreground">{r.note}</div>}
          </dd>
        </div>
      ))}
    </dl>
  );
}

/** A card with a heading, an optional Edit at its right, and what is in it. */
export function Card({ title, sub, action, children, className, testId }: { title: string; sub?: React.ReactNode; action?: React.ReactNode; children: React.ReactNode; className?: string; testId?: string }) {
  return (
    <section className={cn("min-w-0 rounded-[var(--radius-card)] border bg-card p-6 lg:p-7", className)} aria-label={title} data-testid={testId}>
      <div className="mb-4 flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h2 className="text-[19px] leading-7 font-semibold tracking-[-0.01em] text-bright">{title}</h2>
          {sub && <p className="mt-1 text-[13.5px] text-muted-foreground">{sub}</p>}
        </div>
        {action}
      </div>
      {children}
    </section>
  );
}

/** A value that is an address, a name or an ARN: monospace, wrapping where it must. */
export const Mono = ({ children }: { children: React.ReactNode }) => <span className="font-mono text-[13px] break-all">{children}</span>;
export const Dim = ({ children = "–" }: { children?: React.ReactNode }) => <span className="text-muted-foreground">{children}</span>;

/** The chips between the sections of one API (`LARDER-PUBLIC  Resources  Stages  Authorizers`): each a `?tab=` of the API's page. */
export function ApiChips({ kind, id, name, sections, current }: { kind: "restapis" | "apis"; id: string; name: string; sections: readonly { id: string; label: string }[]; current: string }) {
  return <ChipNav label={name} current={current} chips={sections.map((s) => ({ id: s.id, label: s.label, to: { kind, name: id, search: { tab: s.id } } }))} testId="api-sections" />;
}

/** A link to a page of one API: its section (`?tab=`) or a form of it (`?do=`). */
export function ApiLink({ kind, id, search = {}, children, className, testId, title }: { kind: "restapis" | "apis"; id: string; search?: Record<string, string | undefined>; children: React.ReactNode; className?: string; testId?: string; title?: string }) {
  return (
    <Link to="/resources/$service/$kind/$name" params={{ service: "apigateway", kind, name: id }} search={search as never} className={cn("rounded-sm text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none", className)} data-testid={testId} title={title}>
      {children}
    </Link>
  );
}

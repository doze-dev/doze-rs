import { Link } from "@tanstack/react-router";
import { ArrowRightIcon } from "lucide-react";
import { Fragment } from "react";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { emphasise } from "@doze/ui/lib/emphasis";
import { Badge } from "@doze/ui/components/ui/badge";
import { cn } from "@doze/ui/lib/utils";
import { type BadgeTone, type DetailBadge, type ResourceDetail, resourceHref, type Wired } from "./contract";

const BADGES: Record<BadgeTone, string> = {
  good: "border-good/40 bg-good/10 text-good",
  warn: "border-warn/40 bg-warn/10 text-warn",
  bad: "border-bad/40 bg-bad/10 text-bad",
  info: "border-cool/40 bg-cool/10 text-cool",
  neutral: "border-border bg-secondary text-secondary-foreground",
};

/** A badge of a resource's header (its type, its state): the word carries the meaning, the tone repeats it. */
export function DetailBadgeView({ badge }: { badge: DetailBadge }) {
  return (
    <Badge variant="outline" className={cn("px-2.5 text-[12.5px] font-normal", BADGES[badge.tone])} data-tone={badge.tone} data-testid="detail-badge">
      {badge.text}
    </Badge>
  );
}

/** `text` cut into the parts that are `name` and the parts that are not, so a sentence can set the resource's name in type. The name is
 * found whole: `pick-lists` is not found inside `pick-lists-dlq`. */
export function splitName(text: string, name: string): { text: string; name: boolean }[] {
  return emphasise(text, { names: [name] }).map((p) => ({ text: p.text, name: p.kind === "name" }));
}

/** The live sentence under the header: the resource's own name and every name `also` gives (what it is wired to) whole, in
 * monospace and bold, and the figures `bold` gives (a pattern or a phrase) in bold. */
export function Sentence({ text, name, also, bold }: { text: string; name: string; also?: readonly string[]; bold?: readonly (string | RegExp)[] }) {
  return (
    <p className="mb-6 max-w-[62rem] text-[15px] leading-6 text-muted-foreground" data-testid="detail-sentence">
      <Emphasis text={text} names={[name, ...(also ?? [])]} bold={bold} />
    </p>
  );
}

/** What the resource is connected to, as a row of links: each says how (`Publishes to`) and what. With `self` the resource's own name is a
 * chip of the row too, at position `at` (what comes before it flows in, what comes after flows out). `chipText` gives a chip its own
 * words (`payment-settled rule, bus larder`) in place of the `label` and name; the label is then on hover. */
export function WiredStrip({ wired, className, self, chipText, lead = "Wired to" }: { wired: Wired[]; className?: string; /** The strip's first words (`Triggered by`). */ lead?: string; self?: { name: string; at: number }; chipText?: (w: Wired) => string }) {
  if (wired.length === 0) return null;
  const chip = (w: Wired) => (
    <Link to={resourceHref(w.to) as never} title={w.note ?? (chipText ? w.label : undefined)} className="inline-flex min-w-0 max-w-full items-center gap-2 rounded-lg border bg-well px-3 py-1.5 text-[13px] outline-none hover:bg-accent focus-visible:ring-[3px] focus-visible:ring-ring/50" data-testid="wired-link">
      {chipText ? <span className="truncate font-mono text-bright">{chipText(w)}</span> : (<><span className="text-muted-foreground">{w.label}</span><span className="truncate font-mono text-bright">{w.to.name}</span></>)}
    </Link>
  );
  const items: React.ReactNode[] = wired.map((w) => <Fragment key={`${w.label}|${w.to.service}|${w.to.kind}|${w.to.name}`}>{chip(w)}</Fragment>);
  if (self) items.splice(Math.min(self.at, items.length), 0, <span key="self" aria-current="page" className="inline-flex items-center rounded-lg border border-primary/40 bg-pick px-3 py-1.5 font-mono text-[13px] text-bright" data-testid="wired-self">{self.name}</span>);
  return (
    <nav aria-label="Wired to" className={cn("mb-8 flex flex-wrap items-center gap-x-3 gap-y-2 rounded-[14px] border bg-card px-5 py-4", className)} data-testid="wired-strip">
      <span className="text-[13.5px] text-muted-foreground">{lead}</span>
      {items.map((it, i) => (
        <Fragment key={i}>
          {i > 0 && <ArrowRightIcon aria-hidden className="size-3.5 text-input" />}
          {it}
        </Fragment>
      ))}
    </nav>
  );
}

/** The fallback Details tab: every field the descriptor read, label and value, in two columns. The slice that wants a hidden
 * value revealed, or a link drawn, passes `renderValue`. */
export function DetailFields({ detail, renderValue }: { detail: ResourceDetail; renderValue?: (f: ResourceDetail["fields"][number]) => React.ReactNode }) {
  const fields = detail.fields;
  const half = Math.ceil(fields.length / 2);
  const col = "grid grid-cols-[minmax(9rem,13rem)_minmax(0,1fr)] content-start [&>div:last-child>*]:border-b-0";
  const row = (f: ResourceDetail["fields"][number]) => (
    <div key={f.label} className="contents">
      <dt className="border-b py-3 pr-4 text-[13.5px] text-muted-foreground">{f.label}</dt>
      <dd className="min-w-0 border-b py-3 text-[13.5px] break-words text-bright" data-testid={`field-${f.label}`}>
        {renderValue ? renderValue(f) : f.hidden ? "Hidden" : (f.value ?? "–")}
      </dd>
    </div>
  );
  return (
    <section className="rounded-[14px] border bg-card p-6 lg:p-7" aria-label="Details" data-testid="details">
      <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">Details</h2>
      <div className="mt-4 grid lg:grid-cols-2 lg:gap-x-12">
        <dl className={col}>{fields.slice(0, half).map(row)}</dl>
        <dl className={col}>{fields.slice(half).map(row)}</dl>
      </div>
    </section>
  );
}

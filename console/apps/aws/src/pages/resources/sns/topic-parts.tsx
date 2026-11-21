// What a topic's page is made of beside its tabs: the strip from publisher to topic to endpoints, and the Details card.
import { Link } from "@tanstack/react-router";
import { ArrowRightIcon, WaypointsIcon } from "lucide-react";
import { Fragment } from "react";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import type { CallRecord } from "../../../api/types/CallRecord";
import { cn } from "@doze/ui/lib/utils";
import { resourceHref } from "../../../kit";
import { endpointName, type SubRow } from "./sub-model";
import type { Topic } from "./topic-model";
import { archiveOf, daysWords } from "./topic-model";

type Chip = { label: string; to?: { service: string; kind: string; name: string }; self?: boolean };

const CHIP = "inline-flex min-w-0 max-w-full items-center gap-2 rounded-lg border bg-well px-3 py-1.5 font-mono text-[13px] text-bright outline-none";

function ChipView({ c }: { c: Chip }) {
  const body = <><span className="truncate">{c.label}</span></>;
  if (c.to) return <Link to={resourceHref(c.to) as never} className={cn(CHIP, "hover:bg-accent focus-visible:ring-[3px] focus-visible:ring-ring/50")} data-testid="wired-link">{body}</Link>;
  return <span className={cn(CHIP, c.self && "border-primary/30 bg-pick")} data-testid={c.self ? "wired-topic" : "wired-chip"}>{c.self && <WaypointsIcon aria-hidden className="size-3.5 text-muted-foreground" />}{body}</span>;
}

/** The distinct callers that published to this topic, newest first (the console's own among them, when it is the only one). */
export function publishers(calls: readonly CallRecord[]): string[] {
  const seen: string[] = [];
  for (const c of calls) if (c.action === "Publish" && c.caller && !seen.includes(c.caller)) seen.push(c.caller);
  return seen.slice(0, 4);
}

/** Publisher → topic → where it delivers: what feeds the topic (from the record of calls) and what it feeds (from its subscriptions). */
export function Pipeline({ name, calls, subs }: { name: string; calls: readonly CallRecord[]; subs: readonly SubRow[] }) {
  const from = publishers(calls);
  const confirmed = subs.filter((s) => !s.pending);
  const groups: { label: string; chips: Chip[] }[] = [];
  const of = (protocol: string, label: string, to: (s: SubRow) => Chip["to"]) => {
    const rows = confirmed.filter((s) => s.protocol === protocol);
    if (rows.length > 0) groups.push({ label, chips: rows.map((s) => ({ label: endpointName(s.protocol, s.endpoint), to: to(s) })) });
  };
  of("sqs", "SQS queues", (s) => ({ service: "sqs", kind: "queues", name: endpointName("sqs", s.endpoint) }));
  of("lambda", "Lambda functions", (s) => ({ service: "lambda", kind: "functions", name: endpointName("lambda", s.endpoint) }));
  of("http", "HTTP endpoints", () => undefined);
  of("https", "HTTPS endpoints", () => undefined);
  const row: { label?: string; chips: Chip[] }[] = [...(from.length > 0 ? [{ label: "Publisher", chips: from.map((l) => ({ label: l })) }] : []), { chips: [{ label: name, self: true }] }, ...groups];
  return (
    <nav aria-label="Wired to" className="mb-8 flex flex-wrap items-center gap-x-3 gap-y-2 rounded-[14px] border bg-card px-5 py-4" data-testid="wired-strip">
      {row.map((g, i) => (
        <Fragment key={i}>
          {i > 0 && <ArrowRightIcon aria-hidden className="size-3.5 text-input" />}
          {g.label && <span className="text-[13.5px] text-muted-foreground">{g.label}</span>}
          <span className="flex flex-wrap items-center gap-2">{g.chips.map((c) => <ChipView key={c.label} c={c} />)}</span>
        </Fragment>
      ))}
    </nav>
  );
}

/** The topic's own identity: name, ARN, display name, owner (and, for a FIFO topic with an archive, how long it keeps). */
export function DetailsCard({ t }: { t: Topic }) {
  const a = archiveOf(t.archive);
  const rows: [string, React.ReactNode][] = [
    ["Name", <span key="n" className="font-mono">{t.name}</span>],
    ["ARN", <span key="a" className="flex items-center gap-2 font-mono" >{t.arn}<CopyButton text={t.arn} what="ARN copied" iconOnly variant="ghost" label="Copy ARN" /></span>],
    ["Display name", t.display ?? <span className="text-muted-foreground">None</span>],
    ["Topic owner", <span key="o" className="font-mono">{t.owner ?? "–"}</span>],
    ...(t.fifo && a.active ? ([["Retention policy", `Archive policy active, ${daysWords(a.days ?? 0)}`]] as [string, React.ReactNode][]) : []),
  ];
  return (
    <section className="mb-10 rounded-[14px] border bg-card px-6 py-2 lg:px-7" aria-label="Details" data-testid="details">
      <dl className="grid grid-cols-[minmax(9rem,13rem)_minmax(0,1fr)] [&>div:last-child>*]:border-b-0">
        {rows.map(([l, v]) => (
          <div key={l} className="contents">
            <dt className="border-b py-3.5 pr-4 text-[13.5px] text-muted-foreground">{l}</dt>
            <dd className="min-w-0 border-b py-3.5 text-[13.5px] break-words text-bright" data-testid={`field-${l}`}>{v}</dd>
          </div>
        ))}
      </dl>
    </section>
  );
}

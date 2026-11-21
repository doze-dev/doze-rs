// What the SNS pages share: the Topics | Subscriptions tabs of the service, a sentence with its names in bold, the calls that touched
// the service or one topic, and a table whose rows are chosen one at a time (the board's radio column).
import { Link } from "@tanstack/react-router";
import { LinkIcon, RadioIcon, WaypointsIcon } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { Button } from "@doze/ui/components/ui/button";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { clockUtc } from "@doze/ui/lib/format";
import { cn } from "@doze/ui/lib/utils";
import { client, PATHS } from "../../../api/client";
import type { CallRecord } from "../../../api/types/CallRecord";
import type { CallsPage } from "../../../api/types/CallsPage";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { Result } from "../../live/outcome";
import { isConsole } from "../../live/search";
import { Block } from "../sqs/parts";
import type { Part } from "./sentences";

/** The two kinds of the service as the pills under the title. */
export function ServiceTabs({ active }: { active: "topics" | "subscriptions" }) {
  const pill = "inline-flex h-9 items-center gap-2 rounded-full border px-4 text-[14px] outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50";
  return (
    <nav aria-label="SNS" className="mb-6 flex gap-2" data-testid="sns-tabs">
      {([["topics", "Topics", WaypointsIcon], ["subscriptions", "Subscriptions", LinkIcon]] as const).map(([id, label, Icon]) => (
        <Link key={id} to="/resources/$service/$second" params={{ service: "sns", second: id }} search={{}} aria-current={id === active ? "page" : undefined} className={cn(pill, id === active ? "border-primary/40 bg-pick text-bright" : "bg-card text-muted-foreground hover:text-foreground")} data-testid={`sns-tab-${id}`}>
          <Icon aria-hidden className="size-4" /> {label}
        </Link>
      ))}
    </nav>
  );
}

/** A sentence whose names and numbers are in bold type. */
export function Rich({ parts, className, testId }: { parts: readonly Part[]; className?: string; testId?: string }) {
  return (
    <p className={cn("max-w-[62rem] text-[15px] leading-6 text-muted-foreground", className)} data-testid={testId}>
      {parts.map((p, i) => (typeof p === "string" ? p : <b key={i} className="font-mono text-[14px] font-semibold text-bright">{p.b}</b>))}
    </p>
  );
}

const CALLS: DataColumn<CallRecord>[] = [
  { id: "time", header: "Time", size: 120, accessorKey: "at_ms", meta: { mono: true }, cell: (c) => clockUtc(c.row.original.at_ms) },
  { id: "call", header: "Call", size: 260, accessorFn: (r) => r.action ?? "", meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px] font-medium text-bright">{c.row.original.action ?? "–"}</span> },
  { id: "caller", header: "Caller", size: 240, min: 140, accessorFn: (r) => r.caller ?? "", cell: (c) => <span className="text-[13.5px]">{c.row.original.caller ?? "–"}{c.row.original.via?.by ? <span className="text-muted-foreground"> via {c.row.original.via.by}</span> : null}</span> },
  { id: "result", header: "Result", accessorFn: (r) => r.status, cell: (c) => <Result call={c.row.original} /> },
];

/** The newest calls the record holds that SNS answered, newest first. */
export function useSnsCalls() {
  return useQuery({ queryKey: ["sns", "calls"], queryFn: ({ signal }) => client.api<CallsPage>(`${PATHS.calls}?limit=200`, {}, signal), retry: false });
}

export const snsCalls = (page: CallsPage | undefined): CallRecord[] => (page?.calls ?? []).filter((c) => c.service === "sns").reverse();

/** `Last calls in SNS` under the lists. */
export function LastCalls({ calls }: { calls: CallRecord[] }) {
  const shown = calls.slice(0, 6);
  return (
    <Block title="Last calls in SNS" lede="The most recent calls this service handled, newest first. Times are UTC." testId="last-calls" actions={<Button asChild variant="outline" size="sm" className="bg-card"><Link to="/live"><RadioIcon /> Open Live</Link></Button>}>
      <DataTable aria-label="Last calls in SNS" data={shown} columns={CALLS} rowId={(r) => String(r.seq)} maxHeight={420} empty="No call in the Live record reached SNS yet." />
    </Block>
  );
}

/** `Last calls that touched orders-created`: the topic's own, newest first; this console's reads are hidden unless asked for. */
export function TopicCalls({ d, mine, onMine, what }: { d: ResourceDetail; mine: boolean; onMine: (v: boolean) => void; /** What the calls touched, when it is not the resource by its name (`this subscription`). */ what?: string }) {
  const label = what ?? d.name;
  const theirs = d.calls.filter((c) => !isConsole(c));
  const hidden = d.calls.length - theirs.length;
  return (
    <Block title={`Last calls that touched ${label}`} testId="recent-calls" lede={<>Newest first, as doze recorded them. Times are UTC.{(hidden > 0 || mine) && <span data-testid="calls-console-hidden"> {mine ? "This console's own are shown." : `${hidden} made by this console ${hidden === 1 ? "is" : "are"} hidden.`} <button type="button" className="font-medium text-link hover:underline" onClick={() => onMine(!mine)} data-testid="calls-console-toggle">{mine ? "Hide them" : "Show"}</button></span>}</>}>
      <DataTable aria-label="Recent calls" data={(mine ? d.calls : theirs).slice(0, 8)} columns={CALLS} rowId={(r) => String(r.seq)} maxHeight={420} empty={hidden > 0 && !mine ? `Only this console has acted on ${label} so far.` : `No call in the Live record acted on ${label} yet.`} />
    </Block>
  );
}

/** The radio of a row: one row is chosen at a time. */
export function RadioRows({ value, onChange, children }: { value: string | null; onChange: (v: string) => void; children: React.ReactNode }) {
  return <RadioGroup value={value ?? ""} onValueChange={onChange} aria-label="Choose a row" className="block">{children}</RadioGroup>;
}

/** The radio column of a table of rows chosen one at a time. */
export function radioColumn<T>(id: (r: T) => string, noun: string): DataColumn<T> {
  return { id: "choose", size: 44, enableSorting: false, header: () => <span className="sr-only">Choose</span>, cell: (c) => <RadioGroupItem value={id(c.row.original)} aria-label={`Choose ${noun} ${id(c.row.original)}`} data-testid={`choose-${id(c.row.original)}`} /> };
}

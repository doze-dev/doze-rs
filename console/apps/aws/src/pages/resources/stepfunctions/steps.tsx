// What a chosen state did (stepfunctions__execution-running, execution-failed): its input, output, details, definition, Retry rules
// and events, in the card under the graph or the table; and the execution's event history as a table (execution-events).
import { ArrowDownIcon, ArrowUpIcon, ChevronRightIcon, SearchIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Segmented } from "../../../kit";
import { cn } from "@doze/ui/lib/utils";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Card, CodeBlock, pretty } from "../deep/parts";
import { causeHint, durationWords, clockMs } from "./execution-model";
import type { AslState } from "./graph";
import { failureOf, firstFailure, type HistoryEvent, statesOfEvents, STATUS, type Step, tsMs } from "./history";
import { taskResource, taskTarget } from "./model";

/** An event's details: the members of its `…EventDetails`, as JSON (inputs and outputs that are JSON read as such). */
export function detailsOf(e: HistoryEvent): string {
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(e)) {
    if (!k.endsWith("EventDetails") || typeof v !== "object" || v === null) continue;
    for (const [dk, dv] of Object.entries(v as Record<string, unknown>)) {
      if (typeof dv === "string" && (dk === "input" || dk === "output" || dk === "cause")) {
        try {
          out[dk] = JSON.parse(dv);
          continue;
        } catch {
          // As it is.
        }
      }
      out[dk] = dv;
    }
  }
  return Object.keys(out).length === 0 ? "" : JSON.stringify(out, null, 2);
}

type View = "input" | "output" | "details" | "definition" | "retry" | "events";
const VIEWS: { value: View; label: string }[] = [
  { value: "input", label: "Input" },
  { value: "output", label: "Output" },
  { value: "details", label: "Details" },
  { value: "definition", label: "Definition" },
  { value: "retry", label: "Retry" },
  { value: "events", label: "Events" },
];

const KIND_WORD: Record<string, string> = { lambda: "Function", sqs: "Queue", sns: "Topic", dynamodb: "Table", s3: "Bucket", states: "State machine" };

/** The message a Lambda error's cause carries (`{"errorMessage": "'email'", …}`), or the cause as it is. */
export function causeWords(cause: string | undefined): { main: string; more?: string } | null {
  if (!cause) return null;
  try {
    const c = JSON.parse(cause) as { errorMessage?: unknown };
    if (typeof c.errorMessage === "string") return { main: c.errorMessage };
  } catch {
    // As it is.
  }
  return { main: cause };
}

function Pair({ label, children, testId }: { label: string; children: React.ReactNode; testId?: string }) {
  return (
    <div className="contents">
      <dt className="border-b py-3 pr-4 text-[13.5px] text-muted-foreground">{label}</dt>
      <dd className="min-w-0 border-b py-3 text-[13.5px] break-words text-bright" data-testid={testId}>{children}</dd>
    </div>
  );
}

export function StepDetails({ step, name, state, events, first }: { step?: Step; name?: string; state?: AslState; events: HistoryEvent[]; first?: View }) {
  const [asked, setView] = useState<View | null>(null);
  const view = asked ?? first ?? "input";
  if (!name) return <Card title="Step details" lede="Choose a state in the graph to see what it took, what it gave and why it failed." testId="step-details" />;
  if (!step) return <Card title={<span>Step details: <span className="font-mono">{name}</span></span>} lede="This execution has not entered this state." testId="step-details" />;
  const s = STATUS[step.status] ?? { word: step.status, tone: "idle" as const };
  const own = events.filter((e) => step.events.includes(e.id));
  const t = taskResource(state?.Resource);
  const target = state ? taskTarget(state) : null;
  const took = step.startMs !== undefined && step.endMs !== undefined && step.status !== "running" ? step.endMs - step.startMs : null;
  const cause = causeWords(step.cause);
  const hint = causeHint({ state, error: step.error, cause: step.cause, input: step.input });
  return (
    <Card title={<span>Step details: <span className="font-mono" data-testid="step-name">{name}</span></span>} actions={<Segmented value={view} onChange={setView} options={VIEWS} label="Step details" testId="step-views" />} testId="step-details">
      {view === "input" && <CodeBlock text={pretty(step.input) || "None."} max="max-h-80" testId="step-input" />}
      {view === "output" && <CodeBlock text={step.output !== undefined ? pretty(step.output) : step.status === "running" ? "Not yet: the state is running." : "None."} max="max-h-80" testId="step-output" />}
      {view === "details" && (
        <div className="grid lg:grid-cols-2 lg:gap-x-12" data-testid="step-info">
          <dl className="grid grid-cols-[minmax(7rem,10rem)_minmax(0,1fr)] content-start">
            <Pair label="Type">{state?.Type ?? "–"}</Pair>
            <Pair label="Status"><StatusDot tone={s.tone}>{s.word}</StatusDot></Pair>
            {state?.Resource && <Pair label="Resource"><span className="font-mono text-[13px]">{state.Resource}</span>{target && t && !t.activity ? <span className="block text-[12.5px] text-muted-foreground">{KIND_WORD[t.service] ?? "Resource"} {target}</span> : null}</Pair>}
            <Pair label="Duration">{durationWords(took)}</Pair>
          </dl>
          <dl className="grid grid-cols-[minmax(7rem,10rem)_minmax(0,1fr)] content-start">
            {step.error ? <Pair label="Error"><span className="font-mono text-[13px]">{step.error}</span></Pair> : null}
            {cause ? <Pair label="Cause"><span className="font-mono text-[13px]">{cause.main}</span>{hint ? <span className="block text-[12.5px] text-muted-foreground" data-testid="cause-hint">{hint}</span> : null}</Pair> : null}
          </dl>
        </div>
      )}
      {view === "definition" && <CodeBlock text={state ? JSON.stringify(state, null, 2) : "This state is not in the definition the execution ran."} max="max-h-80" testId="step-definition" />}
      {view === "retry" && (
        state?.Retry && state.Retry.length > 0 ? (
          <div className="overflow-auto rounded-[var(--radius-panel)] border" data-testid="step-retry">
            <table className="w-full border-collapse text-left text-[13px]">
              <thead className="bg-well"><tr className="border-b text-[11.5px] font-medium tracking-[0.07em] text-muted-foreground uppercase"><th className="px-4 py-2.5 font-medium">Errors</th><th className="px-4 py-2.5 font-medium">Interval</th><th className="px-4 py-2.5 font-medium">Max attempts</th><th className="px-4 py-2.5 font-medium">Backoff rate</th></tr></thead>
              <tbody>{state.Retry.map((r, i) => <tr key={i} className="border-b last:border-b-0"><td className="px-4 py-2.5 font-mono">{(r.ErrorEquals ?? []).join(", ")}</td><td className="px-4 py-2.5">{r.IntervalSeconds ?? 1} s</td><td className="px-4 py-2.5">{r.MaxAttempts ?? 3}</td><td className="px-4 py-2.5">{r.BackoffRate ?? 2}</td></tr>)}</tbody>
            </table>
          </div>
        ) : <p className="text-[13.5px] text-muted-foreground" data-testid="step-retry">{name} has no Retry rule.</p>
      )}
      {view === "events" && (
        <ul className="grid gap-1 text-[13px]" data-testid="step-events">
          {own.map((e) => (
            <li key={e.id} className="flex gap-3">
              <span className="w-8 text-right font-mono text-muted-foreground tabular-nums">{e.id}</span>
              <span className={cn("font-mono", failureOf(e) ? "text-bad" : "text-bright")}>{e.type}</span>
              <span className="font-mono text-muted-foreground">{clockMs(tsMs(e.timestamp))}</span>
            </li>
          ))}
        </ul>
      )}
    </Card>
  );
}

const PAGE = 25;

function EventRow({ e, open, toggle }: { e: HistoryEvent; open: boolean; toggle: () => void }) {
  const details = detailsOf(e);
  const failed = /Failed|TimedOut|Aborted/.test(e.type);
  return (
    <>
      <tr className={cn("border-b align-top", failed && e.type.startsWith("Execution") && "bg-bad/5")} data-testid="history-event" data-event={e.id}>
        <td className="w-10 py-2.5 pl-3">
          {details && (
            <button type="button" onClick={toggle} aria-expanded={open} aria-label={open ? `Close event ${e.id}` : `Open event ${e.id}`} className="rounded-sm text-muted-foreground hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" data-testid="history-open">
              <ChevronRightIcon className={cn("size-4 transition-transform duration-150", open && "rotate-90")} />
            </button>
          )}
        </td>
        <td className="w-16 py-2.5 pr-4 font-mono text-[12.5px] text-muted-foreground tabular-nums">{e.id}</td>
        <td className={cn("py-2.5 pr-4 font-mono text-[13px] font-medium", failed ? "text-bad" : "text-bright")}>{e.type}</td>
        <td className="py-2.5 pr-4 font-mono text-[13px] tabular-nums">{clockMs(tsMs(e.timestamp))}</td>
      </tr>
      {open && (
        <tr className="border-b">
          <td />
          <td colSpan={3} className="pr-4 pb-3"><CodeBlock text={details} max="max-h-72" /></td>
        </tr>
      )}
    </>
  );
}

/** The event history, 25 events a page, oldest first until the Timestamp header is chosen; an event opens for its details. */
export function EventsView({ events, startMs }: { events: HistoryEvent[]; startMs: number | null }) {
  const [newest, setNewest] = useState(false);
  // It opens on the event that failed (a person came to see why), on the page that holds it, until they open another.
  const first = useMemo(() => firstFailure(events), [events]);
  const [page, setPage] = useState(() => Math.max(0, Math.floor(events.findIndex((e) => e.id === first?.id) / PAGE)));
  const [chosen, setChosen] = useState<number | null | undefined>(undefined);
  const open = chosen === undefined ? (first?.id ?? null) : chosen;
  const states = useMemo(() => statesOfEvents(events), [events]);
  const openEvent = open === null ? undefined : events.find((e) => e.id === open);
  const [filter, setFilter] = useState("");
  const found = useMemo(() => events.filter((e) => `${e.id} ${e.type}`.toLowerCase().includes(filter.trim().toLowerCase())), [events, filter]);
  const sorted = useMemo(() => (newest ? [...found].reverse() : found), [found, newest]);
  const shown = sorted.slice(page * PAGE, page * PAGE + PAGE);
  const last = Math.min(found.length, page * PAGE + PAGE);
  return (
    <Card title="Event view" lede={startMs !== null ? <>Timestamps are {new Date(startMs).toISOString().slice(0, 10)}, in UTC.</> : undefined} testId="execution-events">
      <div className="relative mb-4 w-full max-w-[22rem]">
        <SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
        <Input value={filter} onChange={(e) => { setFilter(e.target.value); setPage(0); }} placeholder="Filter by properties or search by keyword" aria-label="Filter events" className="bg-card pl-9" data-testid="events-filter" />
      </div>
      <div className="overflow-auto rounded-[var(--radius-panel)] border" data-testid="history">
        <table className="w-full border-collapse text-left">
          <thead className="bg-well">
            <tr className="border-b text-[11.5px] font-medium tracking-[0.07em] text-muted-foreground uppercase">
              <th className="w-10 py-2.5 pl-3" aria-label="Open" />
              <th className="w-16 py-2.5 pr-4 font-medium">ID</th>
              <th className="py-2.5 pr-4 font-medium">Type</th>
              <th className="py-2.5 pr-4 font-medium">
                <button type="button" onClick={() => { setNewest(!newest); setPage(0); }} className="inline-flex items-center gap-1 rounded-sm font-medium uppercase hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" aria-label={`Timestamp, sorted ${newest ? "newest" : "oldest"} first`} data-testid="history-sort">
                  Timestamp {newest ? <ArrowDownIcon className="size-3.5" /> : <ArrowUpIcon className="size-3.5" />}
                </button>
              </th>
            </tr>
          </thead>
          <tbody>
            {shown.map((e) => <EventRow key={e.id} e={e} open={open === e.id} toggle={() => setChosen(open === e.id ? null : e.id)} />)}
          </tbody>
        </table>
      </div>
      <div className="mt-4 flex items-center justify-between gap-3 text-[13px] text-muted-foreground">
        <span data-testid="history-line">{found.length} {found.length === 1 ? "event" : "events"}, {found.length === 0 ? 0 : page * PAGE + 1} to {last}</span>
        <span className="flex gap-2">
          <Button variant="outline" size="sm" className="bg-card" disabled={page === 0} onClick={() => setPage(page - 1)} data-testid="history-prev">Previous</Button>
          <Button variant="outline" size="sm" className="bg-card" disabled={last >= found.length} onClick={() => setPage(page + 1)} data-testid="history-next">Next</Button>
        </span>
      </div>
      <p className="mt-5 max-w-[46rem] border-l-2 border-input pl-3.5 text-[13px] leading-5 text-muted-foreground" data-testid="history-note">
        A page shows up to 25 events, and an execution's history is limited to 25,000 events. Events are sorted by Timestamp, oldest first; choose the Timestamp header to reverse the order. Choose the arrow beside an ID to see an event's input, output and resource invocation.{openEvent && <>{" "}<span data-testid="history-expanded">The expanded event is event {openEvent.id}, {openEvent.type}{states.get(openEvent.id) ? ` on ${states.get(openEvent.id)}` : ""}.</span></>}
      </p>
    </Card>
  );
}

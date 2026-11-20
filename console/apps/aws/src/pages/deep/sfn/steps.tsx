// A chosen state's details (its status, input, output, error, and its events), and the execution's event history as a list
// that opens each event's details.
import { ChevronRightIcon } from "lucide-react";
import { useState } from "react";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { cn } from "@doze/ui/lib/utils";
import { CodeBlock, Section, pretty, when } from "../parts";
import { failureOf, type HistoryEvent, STATUS, type Step } from "./history";

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

export function StepDetails({ step, name, events }: { step?: Step; name?: string; events: HistoryEvent[] }) {
  const [view, setView] = useState<"input" | "output" | "events">("input");
  if (!name) {
    return (
      <Section title="State details" lede="Choose a state in the graph to see what it took, what it gave and why it failed." testId="step-details" />
    );
  }
  if (!step) {
    return (
      <Section title={<span>State details: <span className="font-mono">{name}</span></span>} lede="This execution has not entered this state." testId="step-details" />
    );
  }
  const s = STATUS[step.status] ?? { word: step.status, tone: "idle" as const };
  const own = events.filter((e) => step.events.includes(e.id));
  return (
    <Section title={<span>State details: <span className="font-mono">{name}</span></span>} actions={<StatusDot tone={s.tone}>{s.word}</StatusDot>} testId="step-details">
      {step.error && (
        <div className="mb-4 rounded-[10px] border border-bad/40 bg-bad/5 px-4 py-3 text-[13px]" data-testid="step-error">
          <b className="font-semibold text-bright">{step.error}</b>
          {step.cause && <p className="mt-1 break-words text-foreground">{step.cause}</p>}
        </div>
      )}
      <div className="mb-3 inline-flex gap-0.5 rounded-[10px] bg-track p-[3px]" role="tablist" aria-label="State details">
        {(["input", "output", "events"] as const).map((v) => (
          <button key={v} type="button" role="tab" aria-selected={view === v} onClick={() => setView(v)} className={cn("h-7 rounded-[8px] px-3 text-[13px] font-medium capitalize", view === v ? "bg-card text-bright shadow-sm" : "text-foreground")} data-testid={`step-${v}`}>
            {v}
          </button>
        ))}
      </div>
      {view === "input" && <CodeBlock text={pretty(step.input) || "None."} max="max-h-80" testId="step-input" />}
      {view === "output" && <CodeBlock text={step.output !== undefined ? pretty(step.output) : step.status === "running" ? "Not yet: the state is running." : "None."} max="max-h-80" testId="step-output" />}
      {view === "events" && (
        <ul className="grid gap-1 text-[13px]">
          {own.map((e) => (
            <li key={e.id} className="flex gap-3">
              <span className="w-8 text-right font-mono text-muted-foreground tabular-nums">{e.id}</span>
              <span className={cn("font-mono", failureOf(e) ? "text-bad" : "text-bright")}>{e.type}</span>
            </li>
          ))}
        </ul>
      )}
    </Section>
  );
}

function EventRow({ e, start }: { e: HistoryEvent; start: number }) {
  const [open, setOpen] = useState(false);
  const details = detailsOf(e);
  const failed = /Failed|TimedOut|Aborted/.test(e.type);
  const ms = typeof e.timestamp === "number" ? (e.timestamp > 1e12 ? e.timestamp : e.timestamp * 1000) : Date.parse(e.timestamp);
  const name = e.stateEnteredEventDetails?.name ?? e.stateExitedEventDetails?.name;
  return (
    <>
      <tr className="border-b align-top" data-testid="history-event">
        <td className="w-8 py-2.5 pl-3">
          {details && (
            <button type="button" onClick={() => setOpen(!open)} aria-expanded={open} aria-label={open ? `Close event ${e.id}` : `Open event ${e.id}`} className="rounded-sm text-muted-foreground hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" data-testid="history-open">
              <ChevronRightIcon className={cn("size-4 transition-transform duration-150", open && "rotate-90")} />
            </button>
          )}
        </td>
        <td className="py-2.5 pr-4 text-right font-mono text-[12.5px] text-muted-foreground tabular-nums">{e.id}</td>
        <td className={cn("py-2.5 pr-4 font-mono text-[13px] font-medium", failed ? "text-bad" : e.type.endsWith("Succeeded") ? "text-good" : "text-bright")}>{e.type}</td>
        <td className="py-2.5 pr-4 font-mono text-[13px]">{name ?? <span className="text-muted-foreground">–</span>}</td>
        <td className="py-2.5 pr-4 text-[13px] tabular-nums">{when(e.timestamp)}</td>
        <td className="py-2.5 pr-4 text-right font-mono text-[12.5px] text-muted-foreground tabular-nums">{Number.isNaN(ms) ? "" : `+${Math.max(0, ms - start)} ms`}</td>
      </tr>
      {open && (
        <tr className="border-b">
          <td />
          <td colSpan={5} className="pr-4 pb-3">
            <CodeBlock text={details} max="max-h-72" />
          </td>
        </tr>
      )}
    </>
  );
}

export function EventsTable({ events }: { events: HistoryEvent[] }) {
  const first = events[0];
  const start = first ? (typeof first.timestamp === "number" ? (first.timestamp > 1e12 ? first.timestamp : first.timestamp * 1000) : Date.parse(first.timestamp)) : 0;
  const newest = [...events].reverse();
  return (
    <div className="max-h-[640px] overflow-auto rounded-[12px] border" data-testid="history">
      <table className="w-full border-collapse text-left">
        <thead className="sticky top-0 z-[1] bg-well">
          <tr className="border-b text-[11.5px] font-medium tracking-[0.07em] text-muted-foreground uppercase">
            <th className="w-8 py-2.5 pl-3" aria-label="Open" />
            <th className="py-2.5 pr-4 text-right font-medium">ID</th>
            <th className="py-2.5 pr-4 font-medium">Type</th>
            <th className="py-2.5 pr-4 font-medium">State</th>
            <th className="py-2.5 pr-4 font-medium">Time (UTC)</th>
            <th className="py-2.5 pr-4 text-right font-medium">Since start</th>
          </tr>
        </thead>
        <tbody>
          {newest.map((e) => (
            <EventRow key={e.id} e={e} start={start} />
          ))}
        </tbody>
      </table>
    </div>
  );
}

// Log events as a list: the time (UTC, to the millisecond), the stream when the list spans several, and the message, wrapped
// or on one line as the person picks; a message that is JSON opens indented. Shared by the stream viewer, the search and Live
// Tail.
import { ChevronRightIcon } from "lucide-react";
import { useState } from "react";
import { Link } from "@tanstack/react-router";
import { clockMsUtc, dateTimeUtc } from "@doze/ui/lib/format";
import { cn } from "@doze/ui/lib/utils";

export type LogEvent = { timestamp?: number; message?: string; logStreamName?: string; eventId?: string; ingestionTime?: number };

/** Whether a message is a JSON object or array (so it can open indented). */
export function jsonMessage(message: string): string | null {
  const t = message.trim();
  if (!(t.startsWith("{") || t.startsWith("["))) return null;
  try {
    return JSON.stringify(JSON.parse(t), null, 2);
  } catch {
    return null;
  }
}

/** The level a message says it is at (`ERROR`, `"level":"warn"`), for its colour; null when it says none. */
export function levelOf(message: string): "bad" | "warn" | null {
  if (/\b(ERROR|FATAL|CRITICAL)\b|"level"\s*:\s*"(error|fatal)"/i.test(message)) return "bad";
  if (/\bWARN(ING)?\b|"level"\s*:\s*"warn(ing)?"/i.test(message)) return "warn";
  return null;
}

function Row({ e, group, wrap, withStream }: { e: LogEvent; group: string; wrap: boolean; withStream: boolean }) {
  const [open, setOpen] = useState(false);
  const message = e.message ?? "";
  const doc = jsonMessage(message);
  const level = levelOf(message);
  const day = e.timestamp ? dateTimeUtc(e.timestamp).slice(0, 10) : "";
  return (
    <tr className="border-b align-top last:border-b-0" data-testid="log-event" data-level={level ?? undefined}>
      <td className="w-8 py-2 pl-3">
        {doc && (
          <button type="button" onClick={() => setOpen(!open)} aria-expanded={open} aria-label={open ? "Close the event" : "Open the event"} className="rounded-sm text-muted-foreground hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none">
            <ChevronRightIcon className={cn("size-4 transition-transform duration-150", open && "rotate-90")} />
          </button>
        )}
      </td>
      <td className="py-2 pr-4 font-mono text-[12.5px] whitespace-nowrap text-muted-foreground tabular-nums" title={day}>
        <span className={cn("mr-2 inline-block size-1.5 rounded-full align-middle", level === "bad" ? "bg-bad" : level === "warn" ? "bg-warn" : "bg-transparent")} aria-hidden />
        {e.timestamp ? `${day.slice(5)} ${clockMsUtc(e.timestamp)}` : "–"}
      </td>
      {withStream && (
        <td className="max-w-[16rem] truncate py-2 pr-4 font-mono text-[12.5px]">
          {e.logStreamName ? (
            <Link to="/resources/$service/$kind/$name" params={{ service: "logs", kind: "streams", name: e.logStreamName }} search={{ group }} className="text-link hover:underline">
              {e.logStreamName}
            </Link>
          ) : (
            "–"
          )}
        </td>
      )}
      <td className={cn("py-2 pr-4 font-mono text-[12.5px] leading-5 text-bright", wrap ? "break-words whitespace-pre-wrap" : "max-w-0 truncate whitespace-pre")} data-testid="log-message">
        {open && doc ? <pre className="my-1 overflow-auto rounded-[10px] border bg-well px-4 py-3 whitespace-pre text-foreground">{doc}</pre> : message}
      </td>
    </tr>
  );
}

export function EventList({ events, group, wrap, withStream = false, empty, testId = "log-events" }: { events: LogEvent[]; group: string; wrap: boolean; withStream?: boolean; empty: React.ReactNode; testId?: string }) {
  if (events.length === 0) return <p className="rounded-[12px] border border-dashed px-5 py-6 text-center text-[13.5px] text-muted-foreground" data-testid={`${testId}-none`}>{empty}</p>;
  return (
    <div className="max-h-[640px] overflow-auto rounded-[12px] border bg-card" data-testid={testId}>
      <table className="w-full table-fixed border-collapse text-left">
        <colgroup>
          <col className="w-8" />
          <col className="w-[12.5rem]" />
          {withStream && <col className="w-[14rem]" />}
          <col />
        </colgroup>
        <thead className="sticky top-0 z-[1] bg-well">
          <tr className="border-b text-[11.5px] font-medium tracking-[0.07em] text-muted-foreground uppercase">
            <th className="py-2.5 pl-3" aria-label="Open" />
            <th className="py-2.5 pr-4 font-medium">Time (UTC)</th>
            {withStream && <th className="py-2.5 pr-4 font-medium">Log stream</th>}
            <th className="py-2.5 pr-4 font-medium">Message</th>
          </tr>
        </thead>
        <tbody>
          {events.map((e, i) => (
            <Row key={e.eventId ?? `${e.timestamp}-${i}`} e={e} group={group} wrap={wrap} withStream={withStream} />
          ))}
        </tbody>
      </table>
    </div>
  );
}

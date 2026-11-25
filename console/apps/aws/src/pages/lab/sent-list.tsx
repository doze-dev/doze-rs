import { RotateCcwIcon, SendIcon, Trash2Icon } from "lucide-react";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Button } from "@doze/ui/components/ui/button";
import { clockUtc, dateTimeUtc, micros, plural } from "@doze/ui/lib/format";
import { Section } from "../card";
import { MOST_SENT, parseSent, type Sent } from "./history";
import { SENT_KEY, useKept, writeKept } from "./kept";

/** The calls sent from here, newest first, kept in this browser for the region chosen. Replay puts a call's input in the editor; Send again
 * sends it as it was. */
export function SentList({ onReplay, onSend, busy }: { onReplay: (s: Sent) => void; onSend: (s: Sent) => void; busy: boolean }) {
  const list = useKept(SENT_KEY, parseSent);
  return (
    <Section
      title="Sent from here"
      testId="lab-history"
      lede={`The last ${MOST_SENT} calls sent from this page, kept in this browser (not in doze-aws), for the region you are looking at.`}
      actions={
        list.length > 0 ? (
          <Button type="button" variant="outline" size="xs" className="bg-card" onClick={() => writeKept(SENT_KEY, [])} data-testid="lab-history-clear">
            <Trash2Icon /> Clear
          </Button>
        ) : undefined
      }
    >
      {list.length === 0 ? (
        <p className="rounded-[var(--radius-card)] border border-dashed bg-card px-6 py-8 text-center text-[13.5px] text-muted-foreground" data-testid="lab-history-empty">
          Nothing sent yet. A call you send is listed here, to replay or send again.
        </p>
      ) : (
        <ul className="divide-y rounded-[var(--radius-card)] border bg-card" aria-label={`${plural(list.length, "call")} sent`}>
          {list.map((s) => (
            <li key={s.id} className="flex min-w-0 flex-wrap items-center gap-x-4 gap-y-2 px-5 py-3.5" data-testid="lab-history-row" data-op={s.op} data-outcome={s.ok ? "ok" : "refused"}>
              <div className="min-w-0 flex-1 basis-64">
                <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                  <span className="text-[14px] font-medium text-bright">
                    {s.title} {s.op}
                  </span>
                  {s.ok ? <StatusDot tone="ok">Answered</StatusDot> : <StatusDot tone="bad">{s.code ?? "Refused"}</StatusDot>}
                  <span className="text-[12.5px] text-muted-foreground" title={dateTimeUtc(s.at)}>
                    {clockUtc(s.at)} · took {micros(s.took_us)}
                  </span>
                </div>
                <p className="mt-1 truncate font-mono text-[12.5px] text-muted-foreground" title={s.summary} data-testid="lab-history-summary">
                  {s.summary}
                </p>
              </div>
              <div className="flex flex-none items-center gap-2">
                <Button type="button" variant="outline" size="sm" className="bg-card" disabled={s.input === null} title={s.input === null ? "Its input was too long to keep." : "Put its input in the editor"} onClick={() => onReplay(s)} data-testid="lab-replay">
                  <RotateCcwIcon /> Replay
                </Button>
                <Button type="button" variant="outline" size="sm" className="bg-card" disabled={s.input === null || busy} title={s.input === null ? "Its input was too long to keep." : "Send it again as it was"} onClick={() => onSend(s)} data-testid="lab-send-again">
                  <SendIcon /> Send again
                </Button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </Section>
  );
}

// Live Tail of a log group: one StartLiveTail session made as the console, carried to the page as it happens (server-sent
// events). The updates are CloudWatch Logs' own, one a second while the session is open; the console keeps no timer. Stop, or
// leaving the tab, ends the session.
import { CircleStopIcon, PlayIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Switch } from "@doze/ui/components/ui/switch";
import { plural } from "@doze/ui/lib/format";
import { TAIL_KEPT, useLiveTail } from "./live-tail";
import { Card, Made } from "../deep/parts";
import { EventList } from "./events";

export function GroupTail({ group }: { group: string }) {
  const [pattern, setPattern] = useState("");
  const [streams, setStreams] = useState("");
  const [asked, setAsked] = useState({ pattern: "", streams: [] as string[] });
  const [wrap, setWrap] = useState(true);
  const tail = useLiveTail(group, asked.streams, asked.pattern);
  const start = (e: React.FormEvent) => {
    e.preventDefault();
    setAsked({ pattern, streams: streams.split(",").map((s) => s.trim()).filter(Boolean) });
    tail.start();
  };
  const status = tail.state.status;
  return (
    <Card
      title="Live Tail"
      lede="Events as the group takes them, while this tab is open. CloudWatch Logs sends an update a second; the newest is at the top."
      actions={
        <StatusDot tone={status === "open" ? "ok" : status === "opening" ? "info" : "idle"} data-testid="tail-status">
          {status === "open" ? "Tailing" : status === "opening" ? "Starting…" : status === "ended" ? "Ended" : "Not started"}
        </StatusDot>
      }
      testId="group-tail"
    >
      <form onSubmit={start} className="flex flex-wrap items-end gap-3">
        <div className="grid min-w-[16rem] flex-1 gap-1.5">
          <Label htmlFor="tail-pattern">Filter pattern <span className="ml-1 font-normal text-muted-foreground">optional</span></Label>
          <Input id="tail-pattern" value={pattern} onChange={(e) => setPattern(e.target.value)} placeholder="e.g. ERROR" spellCheck={false} className="bg-card font-mono" disabled={tail.on} data-testid="tail-pattern" />
        </div>
        <div className="grid min-w-[14rem] flex-1 gap-1.5">
          <Label htmlFor="tail-streams">Log streams <span className="ml-1 font-normal text-muted-foreground">optional</span></Label>
          <Input id="tail-streams" value={streams} onChange={(e) => setStreams(e.target.value)} placeholder="e.g. web-1, web-2" spellCheck={false} className="bg-card font-mono" disabled={tail.on} />
        </div>
        {tail.on ? (
          <Button key="stop" type="button" variant="outline" className="bg-card" onClick={tail.stop} data-testid="tail-stop">
            <CircleStopIcon /> Stop
          </Button>
        ) : (
          <Button key="start" type="submit" data-testid="tail-start">
            <PlayIcon /> Start
          </Button>
        )}
      </form>
      <p className="mt-2 text-[12.5px] text-muted-foreground">Empty fields mean every event of every stream. Streams are names, separated by commas.</p>
      {tail.state.error && (
        <Alert tone={tail.state.error.code === "BUSY" ? "warn" : "bad"} title={`The session ended: ${tail.state.error.code}`} className="mt-4" testId="tail-error">
          {tail.state.error.message}
        </Alert>
      )}
      <div className="mt-5 mb-3 flex flex-wrap items-center justify-between gap-3">
        <p className="text-[13.5px] text-foreground" data-testid="tail-count">
          {tail.events.length === 0 ? (tail.on ? "Waiting for events…" : "No events yet.") : `${plural(tail.events.length, "event")}${tail.events.length >= TAIL_KEPT ? `, the newest ${TAIL_KEPT} kept` : ""}.`}
          {tail.sampled && <span className="text-warn"> CloudWatch Logs sampled some updates: more than 500 events came in a second.</span>}
        </p>
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2">
            <Switch id="tail-wrap" checked={wrap} onCheckedChange={setWrap} />
            <Label htmlFor="tail-wrap" className="font-normal">Wrap lines</Label>
          </div>
          <Button variant="ghost" size="sm" onClick={tail.clear} disabled={tail.events.length === 0}>
            <Trash2Icon /> Clear
          </Button>
        </div>
      </div>
      <EventList events={tail.events} group={group} wrap={wrap} withStream empty={tail.on ? "The session is open; events show here as they come." : "Start a session to see events as they come."} testId="tail-events" />
      <Made className="mt-3" ops={["StartLiveTail"]} more="One session per open tab; doze-aws allows 15 at once in a region, as AWS does." />
    </Card>
  );
}

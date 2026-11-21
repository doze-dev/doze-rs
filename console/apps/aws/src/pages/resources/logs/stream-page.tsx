// One log stream's events (GetLogEvents), a page at a time: the newest page first, or from the start, or from a moment
// chosen (the time jump); Older and Newer follow the tokens the answer gives, both ways. Lines wrap or not, as the person picks.
import { ArrowDownToLineIcon, ArrowUpToLineIcon, ChevronLeftIcon, ChevronRightIcon } from "lucide-react";
import { useCallback, useState } from "react";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { UtcDateTimePicker } from "@doze/ui/components/app/utc-datetime-picker";
import { Button } from "@doze/ui/components/ui/button";
import { Label } from "@doze/ui/components/ui/label";
import { Switch } from "@doze/ui/components/ui/switch";
import { on, useOpRead } from "../deep/api";
import { Loaded, Made } from "../deep/parts";
import { EventList, type LogEvent } from "./events";

type Page = { events?: LogEvent[]; nextForwardToken?: string; nextBackwardToken?: string };
/** Where the reading is: the newest page, the first, from a moment, or a page a token names (and which way it went). */
type At = { from: "end" } | { from: "start" } | { from: "time"; ms: number } | { from: "token"; token: string; way: "older" | "newer" };

/** The most events a page asks for. */
export const PAGE = 100;

/** GetLogEvents' request for `at`. */
export function requestOf(group: string, stream: string, at: At): Record<string, unknown> {
  const base = { logGroupName: group, logStreamName: stream, limit: PAGE };
  switch (at.from) {
    case "end":
      return { ...base, startFromHead: false };
    case "start":
      return { ...base, startFromHead: true };
    case "time":
      return { ...base, startFromHead: true, startTime: at.ms };
    case "token":
      return { ...base, nextToken: at.token };
  }
}

export function StreamPage({ group, stream }: { group: string; stream: string }) {
  const [at, setAt] = useState<At>({ from: "end" });
  const [wrap, setWrap] = useState(true);
  const [jump, setJump] = useState<number | null>(null);
  const q = useOpRead<Page>(on.group(group), "GetLogEvents", requestOf(group, stream, at), group !== "");
  const { refetch } = q;
  const refresh = useCallback(() => void refetch(), [refetch]);
  const page = q.data;
  // A page asked for one way that came back empty: there is nothing further that way (AWS answers the token it was given).
  const empty = !!page && (page.events ?? []).length === 0;
  const noOlder = empty && at.from === "token" && at.way === "older";
  const noNewer = empty && at.from === "token" && at.way === "newer";
  const events = page?.events ?? [];
  const span = events.length > 0 ? `${events.length} events` : "No events";
  return (
    <>
      <PageHeader
        title={stream}
        mono
        crumbs={[
          { label: "Resources", to: "/resources" },
          { label: "CloudWatch Logs", to: "/resources/logs" },
          { label: "Log groups", to: "/resources/logs/groups" },
          { label: group, to: `/resources/logs/groups/${encodeURIComponent(group)}` },
        ]}
        sub={
          <>
            A log stream in <span className="font-mono text-foreground">{group}</span>. {PAGE} events a page, oldest at the top; times are UTC.
          </>
        }
        refresh={<RefreshControl updatedAt={q.dataUpdatedAt} fetching={q.isFetching} onRefresh={refresh} />}
      />
      {!group ? (
        <p className="rounded-[12px] border border-dashed px-5 py-6 text-[13.5px] text-muted-foreground">This address names no log group (?group=). Open the stream from its group's page.</p>
      ) : (
        <div data-testid="stream-viewer">
          <div className="mb-4 flex flex-wrap items-center gap-2">
            <Button variant="outline" size="sm" className="bg-card" onClick={() => setAt({ from: "start" })} data-testid="stream-start"><ArrowUpToLineIcon /> From the start</Button>
            <Button variant="outline" size="sm" className="bg-card" onClick={() => setAt({ from: "end" })} data-testid="stream-end"><ArrowDownToLineIcon /> Newest</Button>
            <span className="mx-1 h-6 w-px bg-border" aria-hidden />
            <UtcDateTimePicker value={jump} onChange={setJump} aria-label="Jump to a time, UTC" className="w-64" />
            <Button variant="outline" size="sm" className="bg-card" disabled={jump === null} onClick={() => jump !== null && setAt({ from: "time", ms: jump })} data-testid="stream-jump">Go</Button>
            <span className="flex-1" />
            <div className="flex items-center gap-2">
              <Switch id="stream-wrap" checked={wrap} onCheckedChange={setWrap} data-testid="stream-wrap" />
              <Label htmlFor="stream-wrap" className="font-normal">Wrap lines</Label>
            </div>
          </div>
          <Loaded q={q} what="The stream's events" height="h-80">
            {(p) => (
              <EventList
                events={p.events ?? []}
                group={group}
                wrap={wrap}
                empty={at.from === "time" ? "No events at or after that time." : at.from === "token" ? `No ${at.way} events.` : "This stream holds no events."}
              />
            )}
          </Loaded>
          <div className="mt-3 flex flex-wrap items-center justify-between gap-3">
            <span className="text-[13px] text-muted-foreground" data-testid="stream-span">
              {span}
              {at.from === "time" && <> from the jump</>}
              {noNewer && <> · nothing newer</>}
              {noOlder && <> · nothing older</>}
            </span>
            <div className="flex items-center gap-2">
              <Button variant="outline" size="sm" className="bg-card" disabled={!page?.nextBackwardToken || !!noOlder} onClick={() => page?.nextBackwardToken && setAt({ from: "token", token: page.nextBackwardToken, way: "older" })} data-testid="stream-older"><ChevronLeftIcon /> Older</Button>
              <Button variant="outline" size="sm" className="bg-card" disabled={!page?.nextForwardToken || !!noNewer} onClick={() => page?.nextForwardToken && setAt({ from: "token", token: page.nextForwardToken, way: "newer" })} data-testid="stream-newer">Newer <ChevronRightIcon /></Button>
            </div>
          </div>
          <Made className="mt-3" ops={["GetLogEvents"]} more="A message that is JSON opens with the arrow beside it." />
        </div>
      )}
    </>
  );
}

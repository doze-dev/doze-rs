// One log stream's events (logs__log-stream-events, log-stream-events-none): FilterLogEvents on this stream alone, over a relative or an
// absolute range, narrowed by a filter pattern, said in UTC or local time. An arrow opens an event whole.
import { ScrollTextIcon, SearchIcon } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { Badge } from "@doze/ui/components/ui/badge";
import { Button } from "@doze/ui/components/ui/button";
import { DropdownMenu, DropdownMenuCheckboxItem, DropdownMenuContent, DropdownMenuTrigger } from "@doze/ui/components/ui/dropdown-menu";
import { Input } from "@doze/ui/components/ui/input";
import { useResourceDetail } from "../../../api/resources";
import { RuleHelp, Segmented, useServiceCrumbs } from "../../../kit";
import { callOp, on } from "../deep/api";
import { Loaded, Made } from "../deep/parts";
import { useQuery } from "@tanstack/react-query";
import { EventList, type LogEvent } from "./events";
import { PATTERN_TEACHING } from "./examples";
import { type Range, DEFAULT_RANGE, type Zone, bounds, clock, spanLine } from "./range";
import { RangeControls } from "./range-controls";
import { streamsOf } from "./model";

type Page = { events?: LogEvent[]; nextToken?: string };
/** The most events a read asks for. */
export const PAGE = 100;

export function StreamPage({ group, stream }: { group: string; stream: string }) {
  const base = useServiceCrumbs("logs");
  const [typed, setTyped] = useState("");
  const [applied, setApplied] = useState("");
  const [range, setRange] = useState<Range>(DEFAULT_RANGE);
  const [zone, setZone] = useState<Zone>("utc");
  const [wrap, setWrap] = useState(true);
  // The moment the range counts back from: fixed until the person searches or refreshes, so the same read is not made twice.
  const [now, setNow] = useState(() => Date.now());
  const [more, setMore] = useState<LogEvent[]>([]);
  const detail = useResourceDetail("logs", "groups", group, group !== "");
  const last = streamsOf(detail.data?.subs.find((s) => s.title === "Log streams")).find((s) => s.name === stream)?.last ?? null;
  const span = useMemo(() => bounds(range, now), [range, now]);
  const req = useMemo(() => ({ logGroupName: group, logStreamNames: [stream], limit: PAGE, ...(applied.trim() ? { filterPattern: applied } : {}), ...span }), [group, stream, applied, span]);
  const q = useQuery({ queryKey: ["resources", "deep", "logs", "groups", group, "stream-events", req], queryFn: ({ signal }) => callOp<Page>(on.group(group), "FilterLogEvents", req, signal), enabled: group !== "", retry: false });
  const { refetch } = q;
  const refresh = useCallback(() => { setNow(Date.now()); setMore([]); void refetch(); }, [refetch]);
  const search = (e: React.FormEvent) => { e.preventDefault(); setApplied(typed); setNow(Date.now()); setMore([]); };
  const loadMore = (token: string) => void callOp<Page>(on.group(group), "FilterLogEvents", { ...req, nextToken: token }).then((p) => setMore((m) => [...m, ...(p.events ?? [])]));
  return (
    <>
      <PageHeader
        title={stream}
        mono
        titleExtra={<Badge variant="outline" className="px-2.5 text-[12.5px] font-normal text-muted-foreground">Log stream</Badge>}
        crumbs={[...base, { label: group, to: `/resources/logs/groups/${encodeURIComponent(group)}` }]}
        sub={<>In <span className="font-mono text-foreground">{group}</span>.{last !== null ? ` Last event at ${clock(last, zone).slice(0, 8)}.` : ""} Choose an arrow to expand one event.</>}
        refresh={<RefreshControl updatedAt={q.dataUpdatedAt} fetching={q.isFetching} onRefresh={refresh} />}
        actions={
          <DropdownMenu>
            <DropdownMenuTrigger asChild><Button type="button" variant="outline" className="bg-card" data-testid="view-menu">Text</Button></DropdownMenuTrigger>
            <DropdownMenuContent align="end"><DropdownMenuCheckboxItem checked={wrap} onCheckedChange={(v) => setWrap(v === true)} data-testid="stream-wrap">Wrap lines</DropdownMenuCheckboxItem></DropdownMenuContent>
          </DropdownMenu>
        }
      />
      {!group ? (
        <p className="rounded-[12px] border border-dashed px-5 py-6 text-[13.5px] text-muted-foreground">This address names no log group (?group=). Open the stream from its group's page.</p>
      ) : (
        <div data-testid="stream-viewer">
          <form className="mb-4 flex flex-wrap items-center gap-3" role="search" onSubmit={search}>
            <div className="relative w-full max-w-[17rem]">
              <SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
              <Input value={typed} onChange={(e) => setTyped(e.target.value)} placeholder="Search filter" aria-label="Search filter" spellCheck={false} className="bg-card pl-9 font-mono text-[13px]" data-testid="stream-filter" />
            </div>
            <RangeControls range={range} onChange={(r) => { setRange(r); setNow(Date.now()); setMore([]); }} />
            <Segmented label="Time zone" testId="zone" value={zone} onChange={setZone} options={[{ value: "utc", label: "UTC" }, { value: "local", label: "Local time zone" }]} />
            <Button type="submit" variant="outline" className="bg-card" data-testid="stream-search">Search</Button>
          </form>
          <Loaded q={q} what="The stream's events" height="h-80">
            {(p) => {
              const events = [...(p.events ?? []), ...more];
              const next = p.nextToken;
              if (events.length === 0) {
                return (
                  <EmptyTab icon={ScrollTextIcon} title="No older events found" testId="stream-empty">
                    No events match this time range or filter pattern. Try a wider range.
                  </EmptyTab>
                );
              }
              const from = span.startTime ?? Math.min(...events.map((e) => e.timestamp ?? Number.POSITIVE_INFINITY));
              const to = span.endTime ?? Math.max(...events.map((e) => e.timestamp ?? 0));
              return (
                <>
                  <EventList events={events} group={group} wrap={wrap} zone={zone} empty="" />
                  <div className="mt-3 flex flex-wrap items-center justify-end gap-3 text-[13px] text-muted-foreground">
                    <span data-testid="stream-span">{spanLine(events.length, from, to, zone)}</span>
                    {next && <Button type="button" variant="outline" size="sm" className="bg-card" onClick={() => loadMore(next)} data-testid="stream-more">Load more</Button>}
                  </div>
                </>
              );
            }}
          </Loaded>
          <RuleHelp className="mt-8 max-w-[44rem]">{PATTERN_TEACHING("The search filter on a stream")}</RuleHelp>
          <Made className="mt-4" ops={["FilterLogEvents"]} />
        </div>
      )}
    </>
  );
}

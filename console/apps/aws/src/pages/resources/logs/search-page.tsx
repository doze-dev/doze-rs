// Search log group (logs__log-group-search): every stream of the group for events a filter pattern matches (FilterLogEvents), over a
// relative or an absolute range; the first page comes back, and the next while the answer says there is more.
import { SearchIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { errorWords } from "@doze/ui/components/app/alert";
import { Alert } from "@doze/ui/components/app/alert";
import { useResourceDetail } from "../../../api/resources";
import { CallPanel, RuleHelp, useServiceCrumbs, withCurrent } from "../../../kit";
import { callOp, on } from "../deep/api";
import { Made } from "../deep/parts";
import { EventList, type LogEvent } from "./events";
import { EXAMPLES, EXAMPLES_NOTE } from "./examples";
import { DEFAULT_RANGE, type Range, bounds, spanWords } from "./range";
import { RangeControls } from "./range-controls";
import { newestFirst, streamsOf } from "./model";

type Found = { events: LogEvent[]; next?: string };

export function GroupSearch({ group }: { group: string }) {
  const base = useServiceCrumbs("logs");
  const [pattern, setPattern] = useState("");
  const [range, setRange] = useState<Range>(DEFAULT_RANGE);
  const [found, setFound] = useState<(Found & { asked: Record<string, unknown>; range: Range }) | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const detail = useResourceDetail("logs", "groups", group);
  const streams = streamsOf(detail.data?.subs.find((s) => s.title === "Log streams")).length;
  // What the call will be, live: the group, the pattern when there is one, the range's start.
  const [now, setNow] = useState(() => Date.now());
  const input = useMemo(() => ({ logGroupName: group, ...(pattern.trim() ? { filterPattern: pattern } : {}), ...bounds(range, now) }), [group, pattern, range, now]);
  const run = async (req: Record<string, unknown>, add: boolean, asked: Range) => {
    setBusy(true);
    setError(null);
    try {
      const p = await callOp<{ events?: LogEvent[]; nextToken?: string }>(on.group(group), "FilterLogEvents", req);
      setFound((old) => ({ events: [...(add && old ? old.events : []), ...(p.events ?? [])], next: p.nextToken, asked: req, range: asked }));
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  };
  const search = (e: React.FormEvent) => {
    e.preventDefault();
    const at = Date.now();
    setNow(at);
    void run({ ...input, limit: 100, ...bounds(range, at) }, false, range);
  };
  const hit = found ? new Set(found.events.map((e) => e.logStreamName)).size : 0;
  const failed = error ? errorWords(error) : null;
  return (
    <>
      <PageHeader
        title="Search log group"
        crumbs={withCurrent([...base, { label: group, to: `/resources/logs/groups/${encodeURIComponent(group)}` }], "Search log group")}
        sub={<>Search every log stream in <span className="font-mono text-foreground">{group}</span> for events that match a filter pattern. <span className="text-muted-foreground">Each search returns up to the first page of data it finds and a token to retrieve the next page.</span></>}
      />
      <div className="grid items-start gap-6 xl:grid-cols-[minmax(0,1fr)_22.5rem]" data-testid="group-search">
        <div className="grid min-w-0 gap-4">
          <form className="flex flex-wrap items-center gap-3" role="search" onSubmit={search}>
            <div className="relative w-full max-w-[18rem]">
              <SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
              <Input value={pattern} onChange={(e) => setPattern(e.target.value)} placeholder="Filter pattern" aria-label="Filter pattern" spellCheck={false} className="bg-card pl-9 font-mono text-[13px]" data-testid="search-pattern" />
            </div>
            <RangeControls range={range} onChange={(r) => { setRange(r); setNow(Date.now()); }} />
            <Button type="submit" variant="outline" className="bg-card" disabled={busy} data-testid="search-run">{busy && !found ? "Searching…" : "Search"}</Button>
          </form>
          {failed && <Alert tone="bad" title={`FilterLogEvents was refused${failed.code ? `: ${failed.code}` : ""}`} testId="write-error">{failed.message}</Alert>}
          {found && (
            <div className="grid gap-3" data-testid="search-results">
              <p className="text-[14px] text-muted-foreground" data-testid="search-count">
                <Emphasis text={found.events.length === 0 ? `No event in ${spanWords(found.range)} matches the pattern.` : `${found.events.length} ${found.events.length === 1 ? "event" : "events"} in ${spanWords(found.range)} ${found.events.length === 1 ? "matches" : "match"} the pattern, in ${hit}${streams ? ` of ${streams}` : ""} ${(streams || hit) === 1 ? "stream" : "streams"}.`} bold={[/^\d[\d,]* events?/, /\d+ of \d+ streams?/]} />
              </p>
              <EventList events={newestFirst(found.events)} group={group} wrap withStream empty="Nothing matched." testId="search-events" />
              {found.next ? (
                <div className="flex justify-end">
                  <Button variant="outline" size="sm" className="bg-card" disabled={busy} onClick={() => void run({ ...found.asked, nextToken: found.next }, true, found.range)} data-testid="search-more">{busy ? "Searching…" : "Search further"}</Button>
                </div>
              ) : (
                <p className="text-right text-[13px] text-muted-foreground" data-testid="search-end">The response has no nextToken: nothing is left to search in this range</p>
              )}
            </div>
          )}
          <Made className="mt-2" ops={["FilterLogEvents"]} />
        </div>
        <div className="grid min-w-0 gap-6 xl:sticky xl:top-6">
          <section className="rounded-[14px] border bg-card p-6" aria-label="Filter pattern examples" data-testid="search-examples">
            <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">Filter pattern examples</h2>
            <dl className="mt-3 grid grid-cols-[minmax(0,1fr)_minmax(0,0.8fr)] [&>div:last-child>*]:border-b-0">
              {EXAMPLES.map(([p, words]) => (
                <div key={p} className="contents">
                  <dt className="border-b py-2.5 pr-3"><button type="button" className="rounded-sm text-left font-mono text-[12.5px] break-all text-muted-foreground hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" onClick={() => setPattern(p)}>{p}</button></dt>
                  <dd className="border-b py-2.5 text-[13.5px] text-foreground">{words}</dd>
                </div>
              ))}
            </dl>
            <RuleHelp className="mt-4">{EXAMPLES_NOTE}</RuleHelp>
          </section>
          <CallPanel request={{ service: "logs", operation: "FilterLogEvents", input }} />
        </div>
      </div>
    </>
  );
}

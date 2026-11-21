// A log group searched (FilterLogEvents): a filter pattern over a time range, every stream of the group, the first page and
// then the next as asked (the answer's nextToken). Times are UTC.
import { SearchIcon } from "lucide-react";
import { useState } from "react";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Switch } from "@doze/ui/components/ui/switch";
import { plural } from "@doze/ui/lib/format";
import { callOp, on } from "../deep/api";
import { Card, Made, Refused } from "../deep/parts";
import { EventList, type LogEvent } from "./events";

/** The ranges a search covers, back from now. */
export const RANGES: { id: string; label: string; ms: number | null }[] = [
  { id: "15m", label: "Last 15 minutes", ms: 15 * 60_000 },
  { id: "1h", label: "Last hour", ms: 3_600_000 },
  { id: "6h", label: "Last 6 hours", ms: 6 * 3_600_000 },
  { id: "24h", label: "Last 24 hours", ms: 24 * 3_600_000 },
  { id: "7d", label: "Last 7 days", ms: 7 * 24 * 3_600_000 },
  { id: "all", label: "All time", ms: null },
];

const EXAMPLES: [string, string][] = [
  ["ERROR", "events with the term ERROR"],
  ["ERROR timeout", "events with both terms"],
  ["?ERROR ?WARN", "events with either term"],
  ["ERROR -retry", "events with ERROR and without retry"],
  ['"connection reset"', "the exact phrase"],
  ['{ $.level = "error" }', "JSON events whose level field is error"],
];

type Found = { events: LogEvent[]; next?: string; searched: number };

export function GroupSearch({ group }: { group: string }) {
  const [pattern, setPattern] = useState("");
  const [range, setRange] = useState("1h");
  const [wrap, setWrap] = useState(true);
  const [found, setFound] = useState<Found | null>(null);
  const [asked, setAsked] = useState<Record<string, unknown> | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const run = async (req: Record<string, unknown>, more: boolean) => {
    setBusy(true);
    setError(null);
    try {
      const p = await callOp<{ events?: LogEvent[]; nextToken?: string; searchedLogStreams?: unknown[] }>(on.group(group), "FilterLogEvents", req);
      setFound((old) => ({ events: [...(more && old ? old.events : []), ...(p.events ?? [])], next: p.nextToken, searched: p.searchedLogStreams?.length ?? 0 }));
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  };
  const search = (e: React.FormEvent) => {
    e.preventDefault();
    const span = RANGES.find((r) => r.id === range)?.ms ?? null;
    const now = Date.now();
    const req: Record<string, unknown> = { logGroupName: group, limit: 100, ...(pattern.trim() ? { filterPattern: pattern } : {}), ...(span ? { startTime: now - span, endTime: now } : {}) };
    setAsked(req);
    void run(req, false);
  };
  const rangeLabel = RANGES.find((r) => r.id === range)?.label.toLowerCase() ?? "";
  return (
    <div className="grid gap-6">
      <Card title="Search log group" lede="Every stream of the group, for events a filter pattern matches. The first page comes back; ask for more while the answer says there is more." testId="group-search">
        <form onSubmit={search} className="flex flex-wrap items-end gap-3">
          <div className="grid min-w-[18rem] flex-1 gap-1.5">
            <Label htmlFor="search-pattern">Filter pattern</Label>
            <Input id="search-pattern" value={pattern} onChange={(e) => setPattern(e.target.value)} placeholder="e.g. ERROR" spellCheck={false} className="bg-card font-mono" data-testid="search-pattern" />
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor="search-range">Time range</Label>
            <Select value={range} onValueChange={setRange}>
              <SelectTrigger id="search-range" className="w-48 bg-card" data-testid="search-range">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {RANGES.map((r) => (
                  <SelectItem key={r.id} value={r.id}>
                    {r.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <Button type="submit" disabled={busy} data-testid="search-run">
            <SearchIcon /> {busy && !found ? "Searching…" : "Search"}
          </Button>
        </form>
        <p className="mt-2 text-[12.5px] text-muted-foreground">An empty pattern matches every event. The pattern is CloudWatch Logs' filter syntax, not free text: see the examples below.</p>
        <div className="mt-4">
          <Refused error={error} operation="FilterLogEvents" testId="write-error" />
        </div>
        {found && (
          <div className="mt-5 grid gap-3" data-testid="search-results">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <p className="text-[13.5px] text-foreground" data-testid="search-count">
                {found.events.length === 0 ? "No event matches" : `${plural(found.events.length, "event")} ${found.events.length === 1 ? "matches" : "match"}`} in the {range === "all" ? "whole group" : rangeLabel}
                {found.next ? ", and there are more." : "."}
              </p>
              <div className="flex items-center gap-2">
                <Switch id="search-wrap" checked={wrap} onCheckedChange={setWrap} />
                <Label htmlFor="search-wrap" className="font-normal">Wrap lines</Label>
              </div>
            </div>
            <EventList events={found.events} group={group} wrap={wrap} withStream empty="Nothing matched." testId="search-events" />
            {found.next && asked && (
              <div className="flex justify-end">
                <Button variant="outline" size="sm" className="bg-card" disabled={busy} onClick={() => void run({ ...asked, nextToken: found.next }, true)} data-testid="search-more">
                  {busy ? "Searching…" : "Search further"}
                </Button>
              </div>
            )}
          </div>
        )}
        <Made className="mt-3" ops={["FilterLogEvents"]} />
      </Card>
      <Card title="Filter pattern examples" lede="What a pattern matches. Terms are case-sensitive." testId="search-examples">
        <dl className="grid max-w-[760px] grid-cols-[minmax(10rem,16rem)_minmax(0,1fr)] [&>div:last-child>*]:border-b-0">
          {EXAMPLES.map(([p, words]) => (
            <div key={p} className="contents">
              <dt className="border-b py-2.5 pr-4">
                <button type="button" className="rounded-sm font-mono text-[13px] text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" onClick={() => setPattern(p)}>
                  {p}
                </button>
              </dt>
              <dd className="border-b py-2.5 text-[13.5px] text-foreground">{words}</dd>
            </div>
          ))}
        </dl>
      </Card>
    </div>
  );
}

import { SearchIcon, XIcon } from "lucide-react";
import { useState } from "react";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { number } from "@doze/ui/lib/format";
import { serviceTitle } from "../../words";
import { filtering, type LiveSearch, OUTCOME_FILTERS, type OutcomeFilter } from "./search";

const ALL = "__all";

const OUTCOME_WORDS: Record<OutcomeFilter, string> = {
  ok: "OK",
  error: "Error",
  denied: "Denied",
  "not-built": "Not built",
  "would-be-denied": "Would be denied",
};

/** The wire's filters: service, outcome, caller, and text in the operation or the resource. Each is in the address. `services`
 * and `callers`: what the calls held and the map's flows name. */
export function Filters({ search, set, services, callers, titles, shown, held }: { search: LiveSearch; set: (patch: Partial<LiveSearch>) => void; services: string[]; callers: { id: string; internal: boolean }[]; titles: ReadonlyMap<string, string>; /** Calls the filters keep, of `held`. */ shown: number; held: number }) {
  const [q, setQ] = useState(search.q ?? "");
  // The address is the truth: a link, Back or Clear changes the box too (during render, as React asks for state that follows a
  // prop). What the person is typing is kept while it says the same thing.
  const [seen, setSeen] = useState(search.q);
  if (seen !== search.q) {
    setSeen(search.q);
    if ((search.q ?? "") !== q.trim()) setQ(search.q ?? "");
  }
  return (
    <div className="mb-3 flex flex-wrap items-center gap-2" role="search" aria-label="Filter the calls" data-testid="live-filters">
      <div className="relative w-[16rem]">
        <SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
        <Input
          value={q}
          onChange={(e) => {
            setQ(e.target.value);
            set({ q: e.target.value.trim() || undefined });
          }}
          placeholder="Operation or resource"
          aria-label="Find an operation or resource"
          className="h-8 bg-card pl-8 text-[13px]"
          data-testid="filter-q"
        />
      </div>
      <Select value={search.service ?? ALL} onValueChange={(v) => set({ service: v === ALL ? undefined : v })}>
        <SelectTrigger size="sm" className="min-w-[10rem] bg-card" aria-label="Service" data-testid="filter-service">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={ALL}>Every service</SelectItem>
          {services.map((s) => (
            <SelectItem key={s} value={s}>
              {serviceTitle(s, titles)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <Select value={search.outcome ?? ALL} onValueChange={(v) => set({ outcome: v === ALL ? undefined : (v as OutcomeFilter) })}>
        <SelectTrigger size="sm" className="min-w-[9.5rem] bg-card" aria-label="Outcome" data-testid="filter-outcome">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={ALL}>Every outcome</SelectItem>
          {OUTCOME_FILTERS.map((o) => (
            <SelectItem key={o} value={o}>
              {OUTCOME_WORDS[o]}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <Select value={search.caller ?? ALL} onValueChange={(v) => set({ caller: v === ALL ? undefined : v })}>
        <SelectTrigger size="sm" className="max-w-[16rem] min-w-[10rem] bg-card" aria-label="Caller" data-testid="filter-caller">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={ALL}>Every caller</SelectItem>
          {callers.map((c) => (
            <SelectItem key={`${c.internal}:${c.id}`} value={c.id}>
              {c.internal ? `${serviceTitle(c.id, titles)} (a service)` : <span className="font-mono text-[12.5px]">{c.id || "No key"}</span>}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      {filtering(search) && (
        <>
          <Button variant="ghost" size="sm" onClick={() => set({ service: undefined, outcome: undefined, caller: undefined, q: undefined })} data-testid="filter-clear">
            <XIcon /> Clear
          </Button>
          <span className="text-[12.5px] text-muted-foreground tabular-nums" data-testid="filter-count">
            {number(shown)} of {number(held)} calls
          </span>
        </>
      )}
    </div>
  );
}

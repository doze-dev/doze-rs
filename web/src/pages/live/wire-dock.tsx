// The docks (boards topic-wire, group-wire; classic pages/live/docks.rs): the same tape, filtered to one topic or one group,
// for the topic and group pages (slices R1a and R2) to mount. A snapshot: it follows the page's RefreshControl, never a tail.
//
//   <WireDock topic="orders" />                the last requests that touched a topic
//   <WireDock group="fulfilment" />            every request that touched a group, with its chips and its filter box
import { Link } from "@tanstack/react-router";
import { RadioIcon, SearchIcon, XCircleIcon } from "lucide-react";
import { useEffect, useState } from "react";
import { useGroupWire, useTopicWire } from "@/api/live";
import type { WireRow } from "@/api/types/WireRow";
import type { WireWindow } from "@/api/types/WireWindow";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { hhmmUtc, number } from "@/lib/format";
import { Tape } from "./tape";
import { clockMs } from "./words";

/** How far back a dock reaches: figures in slots, so the line never changes length. */
function Reach({ w }: { w: WireWindow }) {
  return (
    <>
      Among the last <span className="inline-block min-w-[5ch] text-right font-mono tabular-nums">{number(w.rows)}</span> rows the broker kept, back to{" "}
      <span className="inline-block min-w-[5ch] font-mono tabular-nums">{w.oldest_ms ? hhmmUtc(w.oldest_ms) : "none"}</span> UTC.
    </>
  );
}

function TopicDock({ topic }: { topic: string }) {
  const q = useTopicWire(topic);
  const v = q.data;
  return (
    <section className="flex flex-col gap-3" data-testid="wire-dock" data-topic={topic}>
      <div>
        <h2 className="flex items-center gap-2 text-[19px] font-medium text-bright">
          <RadioIcon className="size-4 text-primary" aria-hidden /> On the wire
        </h2>
        <p className="mt-1 text-[13.5px] text-muted-foreground">Client requests that touched this topic. {v ? <Reach w={v.window} /> : null} A Fetch that returns nothing is folded.</p>
      </div>
      {!v ? (
        <Skeleton className="h-48 rounded-xl" />
      ) : (
        <Tape
          rows={v.rows}
          layout="topic"
          label="Requests that touched this topic"
          maxHeight={480}
          empty={
            <span>
              No client has touched <span className="font-mono">{topic}</span> since the broker started.
            </span>
          }
        />
      )}
      <p className="text-[12.5px] text-muted-foreground">
        Kept in memory and emptied at every restart. The console's own requests are not listed, so this can be quieter than the topic's real traffic. A row opens to its detail in{" "}
        <Link to="/live" search={{ topic: [topic] }} className="font-medium text-primary hover:underline">
          Live
        </Link>
        .
      </p>
    </section>
  );
}

type Kind = "" | "join" | "commits" | "heartbeats" | "refused";

function refusalNote(rows: WireRow[]): WireRow | undefined {
  return rows.find((r) => r.class === "refused" || r.class === "denied");
}

function GroupDock({ group }: { group: string }) {
  const [kind, setKind] = useState<Kind>("");
  const [text, setText] = useState("");
  const [applied, setApplied] = useState("");
  useEffect(() => {
    const t = setTimeout(() => setApplied(text.trim()), 350);
    return () => clearTimeout(t);
  }, [text]);
  const p = new URLSearchParams();
  if (kind === "refused") p.set("refused", "1");
  else if (kind) p.set("kind", kind);
  if (applied) p.set("q", applied);
  const q = useGroupWire(group, p.toString());
  const v = q.data;
  const note = v ? refusalNote(v.rows) : undefined;
  return (
    <section className="flex flex-col gap-3" data-testid="wire-dock" data-group={group}>
      <p className="text-[13.5px] text-muted-foreground">Every client request that touched {group}, newest first. Heartbeats fold into one row; a refusal stays on its own row. The console's own requests are not listed.</p>
      <div className="flex flex-wrap items-center gap-3">
        <ToggleGroup type="single" variant="outline" value={kind || "all"} onValueChange={(k) => k && setKind(k === "all" ? "" : (k as Kind))} aria-label="Which requests" className="bg-card" data-testid="group-wire-kind">
          <ToggleGroupItem value="all" className="px-3">
            All {v && <span className="font-mono text-muted-foreground tabular-nums">{number(v.all)}</span>}
          </ToggleGroupItem>
          <ToggleGroupItem value="join" className="px-3">
            Join and sync
          </ToggleGroupItem>
          <ToggleGroupItem value="commits" className="px-3">
            Commits
          </ToggleGroupItem>
          <ToggleGroupItem value="heartbeats" className="px-3">
            Heartbeats
          </ToggleGroupItem>
          <ToggleGroupItem value="refused" className="px-3">
            Refused {v && <span className="font-mono text-muted-foreground tabular-nums">{number(v.refused)}</span>}
          </ToggleGroupItem>
        </ToggleGroup>
        <span className="flex-1" />
        <div className="relative w-64">
          <SearchIcon className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" aria-hidden />
          <Input value={text} onChange={(e) => setText(e.target.value)} placeholder="Filter by member or client" aria-label="Filter by member or client" className="bg-card pl-9" maxLength={128} />
        </div>
      </div>
      {!v ? (
        <Skeleton className="h-48 rounded-xl" />
      ) : (
        <>
          <Tape rows={v.rows} layout="group" label="Requests that touched this group" maxHeight={480} empty="No request has touched this yet." />
          <p className="text-[12.5px] text-muted-foreground">
            <Reach w={v.window} />
          </p>
          <div className="min-h-[3.25rem]">
            {note && (
              <div className="flex items-start gap-3 rounded-xl border border-refused/30 bg-refused/5 p-4 text-[13.5px]" data-testid="group-refusal">
                <XCircleIcon className="mt-0.5 size-[18px] shrink-0 text-refused" aria-hidden />
                <p>
                  <b className="font-mono font-semibold">
                    {note.code_name} ({note.code})
                  </b>{" "}
                  at <span className="font-mono">{clockMs(note.at_ms).slice(0, 8)}</span>: {note.detail || "refused."}{" "}
                  <Link to="/live" search={{ request: note.id }} className="font-medium text-primary hover:underline">
                    Open the request
                  </Link>
                </p>
              </div>
            )}
          </div>
        </>
      )}
    </section>
  );
}

/** A dock of the tape for a topic or a group page. */
export function WireDock(props: { topic: string } | { group: string }) {
  return "topic" in props ? <TopicDock topic={props.topic} /> : <GroupDock group={props.group} />;
}

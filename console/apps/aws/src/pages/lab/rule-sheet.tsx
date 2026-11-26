import { Link } from "@tanstack/react-router";
import { RadioIcon } from "lucide-react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@doze/ui/components/ui/sheet";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { clockMsUtc, dateTimeUtc, number } from "@doze/ui/lib/format";
import { useLabRule } from "../../api/lab";
import type { LabRule } from "../../api/types/LabRule";
import { END_WORDS } from "./rule-form";

function Row({ label, children, testId }: { label: string; children: React.ReactNode; testId?: string }) {
  return (
    <>
      <dt className="border-b py-2.5 pr-4 text-[12.5px] text-muted-foreground">{label}</dt>
      <dd className="min-w-0 border-b py-2.5 text-[13px] text-bright [overflow-wrap:anywhere]" data-testid={testId}>{children}</dd>
    </>
  );
}

function Details({ r }: { r: LabRule }) {
  return (
    <dl className="grid grid-cols-[8rem_minmax(0,1fr)]" data-testid="rule-details">
      <Row label="State">{r.ended ? END_WORDS[r.ended] : r.enabled ? "In force" : "Switched off (it still runs out)"}</Row>
      <Row label="Applies to">{r.what} · {r.target}</Row>
      <Row label="How often">{r.share_words}</Row>
      <Row label="Matched">{number(r.matched)} calls fitted it</Row>
      <Row label="Hits" testId="sheet-hits">{number(r.hits)}{r.max_hits ? ` of ${number(r.max_hits)}` : ""}</Row>
      <Row label="Started">{dateTimeUtc(r.started_ms)}</Row>
      <Row label={r.ended ? "Ended" : "Ends"}>{dateTimeUtc(r.ends_ms)}</Row>
      {r.answers.length > 0 && <Row label="Answers with">{r.answers.map((a) => `${a.service}: ${a.status} ${a.code}`).join("; ")}</Row>}
      {r.first && <Row label="First hit">{clockMsUtc(r.first.at_ms)} UTC: {r.first.service} {r.first.operation}{r.first.resource ? ` on ${r.first.resource}` : ""}{r.first.sdk ? `, from ${r.first.sdk}` : ""}</Row>}
      {r.callers.length > 0 && <Row label="Callers">{r.callers.join(", ")}{r.more_callers > 0 ? `, and ${number(r.more_callers)} more` : ""}</Row>}
    </dl>
  );
}

/** One rule in full, and the calls it touched that Live still holds (newest first, each opening in Live). */
export function RuleSheet({ id, onClose }: { id: number | null; onClose: () => void }) {
  const view = useLabRule(id);
  const rule = view.data?.rule;
  return (
    <Sheet open={id !== null} onOpenChange={(o) => !o && onClose()}>
      <SheetContent className="w-full gap-0 overflow-y-auto sm:max-w-[520px]" data-testid="rule-sheet" aria-describedby="rule-sheet-description">
        <SheetHeader className="gap-1 px-7 pt-7 pb-4">
          <SheetTitle className="pr-8 text-[19px] leading-7 font-medium break-words text-bright" data-testid="rule-sheet-title">{rule?.name ?? `Rule ${id ?? ""}`}</SheetTitle>
          <SheetDescription id="rule-sheet-description" className="text-[13px]">{rule?.sentence ?? "A Lab rule"}</SheetDescription>
        </SheetHeader>
        <div className="grid grid-cols-[minmax(0,1fr)] gap-5 px-7 pb-8">
          {view.isError ? <ErrorBlock what="The rule could not be read" error={view.error} hint="It may have been forgotten: only the last 200 ended rules are kept." testId="rule-sheet-error" /> : !rule ? <Skeleton className="h-48 rounded-xl" /> : (
            <>
              <Details r={rule} />
              <div>
                <h3 className="text-[12px] font-medium tracking-[0.08em] text-muted-foreground uppercase">Calls it touched</h3>
                {view.data && view.data.calls.length > 0 ? (
                  <ul className="mt-2 divide-y rounded-lg border" data-testid="rule-calls">
                    {view.data.calls.slice(0, 20).map((c) => (
                      <li key={c.seq} className="flex items-center gap-3 px-3 py-2 text-[12.5px]">
                        <span className="font-mono text-muted-foreground">{clockMsUtc(c.at_ms)}</span>
                        <span className="min-w-0 flex-1 truncate font-mono text-bright">{c.action ?? "?"}{c.resource ? ` ${c.resource}` : ""}</span>
                        <Link to="/live" search={{ call: c.seq, console: "shown" }} className="text-link hover:underline">Open</Link>
                      </li>
                    ))}
                  </ul>
                ) : <p className="mt-2 text-[13px] text-muted-foreground">No call it touched is held in Live now.</p>}
                {view.data?.more && <p className="mt-1.5 text-[12.5px] text-muted-foreground">It touched more calls than are shown.</p>}
              </div>
              <Button asChild variant="outline" size="sm" className="w-fit bg-card"><Link to="/live" search={{ lab: rule.id, console: "shown" }} data-testid="rule-sheet-live"><RadioIcon /> See all its calls on Live</Link></Button>
            </>
          )}
        </div>
      </SheetContent>
    </Sheet>
  );
}

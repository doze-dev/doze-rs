import { BracesIcon } from "lucide-react";
import { useState } from "react";
import { errorWords } from "@doze/ui/components/app/alert";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { Tabs, TabsList, TabsTrigger } from "@doze/ui/components/ui/tabs";
import { cn } from "@doze/ui/lib/utils";
import type { Preview, PreviewRequest } from "./contract";
import { usePreview } from "./preview";

export type CallView = "json" | "cli" | "boto3";
const VIEWS: { id: CallView; label: string }[] = [
  { id: "json", label: "JSON" },
  { id: "cli", label: "AWS CLI" },
  { id: "boto3", label: "boto3" },
];

/** The text of one view of a preview: the operation's name over the request body (as the board shows it), the CLI line, or the
 * boto3 snippet. */
export function viewText(p: Preview, operation: string, view: CallView, service?: string): string {
  if (view === "cli") return p.cli;
  if (view === "boto3") return p.boto3;
  return `${service ? `${service} ` : ""}${operation}\n${JSON.stringify(withoutWire(p.json), null, 2)}`;
}

/** A query-protocol request carries the wire's `Action` and `Version` beside its members; the call as a person means it has the members only. */
export function withoutWire(json: unknown): unknown {
  if (typeof json !== "object" || json === null || Array.isArray(json)) return json;
  const { Action, Version, ...members } = json as Record<string, unknown>;
  return typeof Action === "string" && typeof Version === "string" ? members : json;
}

/** The members the call still needs, said as words: `Needs QueueName`, `Needs QueueName and Attributes`. */
export function missingWords(missing: string[]): string {
  if (missing.length === 0) return "";
  if (missing.length === 1) return `Needs ${missing[0]}.`;
  return `Needs ${missing.slice(0, -1).join(", ")} and ${missing[missing.length - 1]}.`;
}

/** The call a form will make, live: the request as the service sees it, as an AWS CLI line and as boto3, from `POST /preview`
 * (asked after a pause, the last answer kept while the next is on its way). Copy takes the view shown; "Copy as" takes the
 * others without switching. `refusal` turns it into the call that was refused, with what came back under it; `note` is the
 * sentence under the code (what the call leaves out, what its answer carries). */
export function CallPanel({ request, refusal, note, title, className, showService = false, testId = "call-panel" }: { request: PreviewRequest | null; /** Name the service before the operation (`sns Subscribe`), for a call to a service other than the page's own. */ showService?: boolean; /** The service's refusal of this call: its code and words. */ refusal?: { code: string; message: string } | null; note?: React.ReactNode; title?: string; className?: string; testId?: string }) {
  const [view, setView] = useState<CallView>("json");
  const preview = usePreview(request);
  const p = preview.data;
  const failed = preview.error ? errorWords(preview.error) : null;
  const heading = title ?? (refusal ? "The call that was refused" : "The call this will make");
  const missing = p ? missingWords(p.missing) : "";
  return (
    <aside className={cn("min-w-0 rounded-[14px] border bg-card p-5", className)} aria-label={heading} data-slot="call-panel" data-refused={refusal ? "" : undefined} data-testid={testId}>
      <div className="flex items-center justify-between gap-3">
        <h2 className="flex items-center gap-2 text-[12px] font-medium tracking-[0.08em] text-muted-foreground uppercase">
          <BracesIcon aria-hidden className="size-3.5" /> {heading}
        </h2>
        {preview.settling && p && <span className="text-[11.5px] text-muted-foreground" data-testid={`${testId}-settling`}>Updating…</span>}
      </div>
      <Tabs value={view} onValueChange={(v) => setView(v as CallView)} className="mt-3">
        <TabsList variant="line" className="h-8 w-full justify-start border-b p-0" aria-label="Show the call as">
          {VIEWS.map((v) => (
            <TabsTrigger key={v.id} value={v.id} className="h-8 flex-none px-3 text-[13px]" data-testid={`${testId}-tab-${v.id}`}>
              {v.label}
            </TabsTrigger>
          ))}
        </TabsList>
      </Tabs>
      <div className="mt-3" aria-live="polite">
        {request === null ? (
          <p className="rounded-[10px] border border-dashed px-4 py-6 text-[13px] text-muted-foreground" data-testid={`${testId}-idle`}>
            The call appears here as soon as there is something to send.
          </p>
        ) : failed && !p ? (
          <div className="rounded-[10px] border border-bad/30 bg-bad/5 px-4 py-3 text-[13px]" data-testid={`${testId}-error`}>
            <p className="font-medium text-bright">The call could not be shown.</p>
            <p className="mt-0.5 text-foreground">{failed.message}</p>
            {failed.code && <p className="mt-1 font-mono text-[12px] text-muted-foreground">{failed.code}</p>}
          </div>
        ) : !p ? (
          <Skeleton className="h-44 rounded-[10px]" data-testid={`${testId}-loading`} />
        ) : (
          <div className="relative">
            <CopyButton text={viewText(p, request.operation, view, showService ? request.service : undefined)} label={`Copy ${VIEWS.find((v) => v.id === view)!.label}`} what="Copied" iconOnly variant="ghost" className="absolute top-1.5 right-1.5 z-10" />
          <pre tabIndex={0} className={cn("max-h-[22rem] overflow-auto rounded-[10px] bg-well py-3 pr-11 pl-4 font-mono text-[12.5px] leading-[21px] text-foreground transition-opacity outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50", preview.settling && "opacity-60", view !== "json" && "whitespace-pre-wrap break-all")} data-testid={`${testId}-code`}>
            {viewText(p, request.operation, view, showService ? request.service : undefined)}
          </pre>
          </div>
        )}
      </div>
      {missing && (
        <p className="mt-2 text-[12.5px] text-warn" data-testid={`${testId}-missing`}>
          {missing}
        </p>
      )}
      {refusal && (
        <div className="mt-3 rounded-[10px] border border-bad/30 bg-bad/5 px-4 py-3 text-[13px]" data-testid={`${testId}-came-back`}>
          <p className="text-[12px] font-medium tracking-[0.06em] text-muted-foreground uppercase">What came back</p>
          <p className="mt-1 font-mono text-[12.5px] font-medium text-bad">{refusal.code}</p>
          <p className="mt-0.5 text-foreground">{refusal.message}</p>
        </div>
      )}
      {note && <div className="mt-3 text-[13px] leading-5 text-muted-foreground">{note}</div>}
      {p && (
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <span className="text-[13px] text-muted-foreground">Copy as</span>
          <CopyButton text={p.cli} label="AWS CLI" what="AWS CLI line copied" className="bg-card" />
          <CopyButton text={p.boto3} label="boto3" what="boto3 snippet copied" className="bg-card" />
        </div>
      )}
    </aside>
  );
}

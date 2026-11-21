import { keepPreviousData, useQueries } from "@tanstack/react-query";
import { BracesIcon } from "lucide-react";
import { useState } from "react";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { Tabs, TabsList, TabsTrigger } from "@doze/ui/components/ui/tabs";
import { cn } from "@doze/ui/lib/utils";
import { type CallView, missingWords, viewText } from "./call-panel";
import type { PreviewRequest } from "./contract";
import { useDebounced } from "./debounce";
import { fetchPreview, PREVIEW_PAUSE } from "./preview";

const VIEWS: { id: CallView; label: string }[] = [
  { id: "json", label: "JSON" },
  { id: "cli", label: "AWS CLI" },
  { id: "boto3", label: "boto3" },
];

/** The calls a form makes when it makes more than one (an edit that sets one attribute a call, then the tags): each as the panel of one
 * call draws it, in the order they are made, with one Copy as for them all. `refusal` is the service's refusal of the one that failed. */
export function CallsPanel({ requests, note, refusal, title = "The call this will make", testId = "call-panel" }: { requests: PreviewRequest[]; note?: React.ReactNode; refusal?: { code: string; message: string } | null; title?: string; testId?: string }) {
  const [view, setView] = useState<CallView>("json");
  const key = JSON.stringify(requests.map((r) => [r.service, r.operation, r.input]));
  const pause = useDebounced(key, PREVIEW_PAUSE);
  const asked = JSON.parse(pause.value || "[]") as [string, string, unknown][];
  const answers = useQueries({ queries: asked.map((a) => ({ queryKey: ["preview", JSON.stringify(a)], queryFn: ({ signal }: { signal: AbortSignal }) => fetchPreview({ service: a[0], operation: a[1], input: a[2] }, signal), retry: false, staleTime: Number.POSITIVE_INFINITY, placeholderData: keepPreviousData })) });
  const done = answers.length > 0 && answers.every((a) => a.data);
  const texts = answers.map((a, i) => (a.data ? viewText(a.data, asked[i]![1], view) : ""));
  const all = (v: CallView) => answers.map((a, i) => (a.data ? viewText(a.data, asked[i]![1], v) : "")).filter(Boolean).join(v === "cli" ? "\n" : "\n\n");
  const missing = answers.flatMap((a) => (a.data ? a.data.missing : []));
  return (
    <aside className="min-w-0 rounded-[14px] border bg-card p-5" aria-label={title} data-slot="call-panel" data-refused={refusal ? "" : undefined} data-testid={testId}>
      <h2 className="flex items-center gap-2 text-[12px] font-medium tracking-[0.08em] text-muted-foreground uppercase"><BracesIcon aria-hidden className="size-3.5" /> {title}</h2>
      <Tabs value={view} onValueChange={(v) => setView(v as CallView)} className="mt-3">
        <TabsList variant="line" className="h-8 w-full justify-start border-b p-0" aria-label="Show the calls as">
          {VIEWS.map((v) => <TabsTrigger key={v.id} value={v.id} className="h-8 flex-none px-3 text-[13px]" data-testid={`${testId}-tab-${v.id}`}>{v.label}</TabsTrigger>)}
        </TabsList>
      </Tabs>
      <div className="mt-3 grid gap-3" aria-live="polite">
        {requests.length === 0 ? (
          <p className="rounded-[10px] border border-dashed px-4 py-6 text-[13px] text-muted-foreground" data-testid={`${testId}-idle`}>The calls appear here as soon as there is something to send.</p>
        ) : !done ? (
          <Skeleton className="h-44 rounded-[10px]" data-testid={`${testId}-loading`} />
        ) : (
          <div className="relative grid gap-3">
            <CopyButton text={all(view)} label={`Copy ${VIEWS.find((v) => v.id === view)!.label}`} what="Copied" iconOnly variant="ghost" className="absolute top-1.5 right-1.5 z-10" />
            {texts.map((t, i) => <pre key={i} tabIndex={0} className={cn("max-h-[22rem] overflow-auto rounded-[10px] bg-well py-3 pr-11 pl-4 font-mono text-[12.5px] leading-[21px] text-foreground outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50", view !== "json" && "whitespace-pre-wrap break-all")} data-testid={`${testId}-code`}>{t}</pre>)}
          </div>
        )}
      </div>
      {missing.length > 0 && <p className="mt-2 text-[12.5px] text-warn" data-testid={`${testId}-missing`}>{missingWords([...new Set(missing)])}</p>}
      {refusal && (
        <div className="mt-3 rounded-[10px] border border-bad/30 bg-bad/5 px-4 py-3 text-[13px]" data-testid={`${testId}-came-back`}>
          <p className="text-[12px] font-medium tracking-[0.06em] text-muted-foreground uppercase">What came back</p>
          <p className="mt-1 font-mono text-[12.5px] font-medium text-bad">{refusal.code}</p>
          <p className="mt-0.5 text-foreground">{refusal.message}</p>
        </div>
      )}
      {note && <div className="mt-3 text-[13px] leading-5 text-muted-foreground">{note}</div>}
      {done && (
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <span className="text-[13px] text-muted-foreground">Copy as</span>
          <CopyButton text={all("cli")} label="AWS CLI" what="AWS CLI lines copied" className="bg-card" />
          <CopyButton text={all("boto3")} label="boto3" what="boto3 snippet copied" className="bg-card" />
        </div>
      )}
    </aside>
  );
}

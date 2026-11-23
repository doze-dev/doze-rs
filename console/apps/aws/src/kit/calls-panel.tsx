import { keepPreviousData, useQueries } from "@tanstack/react-query";
import { BracesIcon } from "lucide-react";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { type CallFormat, CallCode, CopyCalls, callJson, callText, missingWords } from "./call-panel";
import type { PreviewRequest } from "./contract";
import { useDebounced } from "./debounce";
import { fetchPreview, PREVIEW_PAUSE } from "./preview";

/** The calls a form makes when it makes more than one (an edit that sets one attribute a call, then the tags): each as the panel of one
 * call draws it, in the order they are made, with one Copy as for them all. `refusal` is the service's refusal of the one that failed. */
export function CallsPanel({ requests, note, refusal, title = "The call this will make", testId = "call-panel", showInput = false, shown, format = "json" }: { requests: PreviewRequest[]; note?: React.ReactNode; refusal?: { code: string; message: string } | null; title?: string; testId?: string; /** The panel shows each request's own input, as the SDK takes it (see CallPanel). */ showInput?: boolean; /** What the panel shows of a request's input, where the input names a member as the wire does and the SDK does not. */ shown?: (operation: string, input: unknown) => unknown; /** The SNS boards draw the calls as an outline, every other board as JSON. */ format?: CallFormat }) {
  const key = JSON.stringify(requests.map((r) => [r.service, r.operation, r.input]));
  const pause = useDebounced(key, PREVIEW_PAUSE);
  const asked = JSON.parse(pause.value || "[]") as [string, string, unknown][];
  // What the panel shows of the i-th call asked (the requests may have changed since the pause).
  const shownInput = (i: number) => (showInput ? (shown ? shown(asked[i]![1], asked[i]![2]) : asked[i]![2]) : undefined);
  const answers = useQueries({ queries: asked.map((a) => ({ queryKey: ["preview", JSON.stringify(a)], queryFn: ({ signal }: { signal: AbortSignal }) => fetchPreview({ service: a[0], operation: a[1], input: a[2] }, signal), retry: false, staleTime: Number.POSITIVE_INFINITY, placeholderData: keepPreviousData })) });
  const done = answers.length > 0 && answers.every((a) => a.data);
  const texts = answers.map((a, i) => (a.data ? callText(a.data, asked[i]![1], { format, input: shownInput(i) }) : ""));
  const all = (pick: (p: NonNullable<(typeof answers)[number]["data"]>, i: number) => string, sep: string) => answers.map((a, i) => (a.data ? pick(a.data, i) : "")).filter(Boolean).join(sep);
  const missing = answers.flatMap((a) => (a.data ? a.data.missing : []));
  const helpText = note && <div className="text-[13px] leading-5 text-muted-foreground">{note}</div>;
  const copy = done && <CopyCalls cli={all((p) => p.cli, "\n")} boto3={all((p) => p.boto3, "\n\n")} json={all((p, i) => callJson(p, shownInput(i)), "\n\n")} format={format} plural />;
  return (
    <aside className="min-w-0 rounded-[14px] border bg-card p-5" aria-label={title} data-slot="call-panel" data-refused={refusal ? "" : undefined} data-testid={testId}>
      <h2 className="flex items-center gap-2 text-[12px] font-medium tracking-[0.08em] text-muted-foreground uppercase"><BracesIcon aria-hidden className="size-3.5" /> {title}</h2>
      <div className="mt-3 grid gap-3" aria-live="polite">
        {requests.length === 0 ? (
          <p className="rounded-[10px] border border-dashed px-4 py-6 text-[13px] text-muted-foreground" data-testid={`${testId}-idle`}>The calls appear here as soon as there is something to send.</p>
        ) : !done ? (
          <Skeleton className="h-44 rounded-xl" data-testid={`${testId}-loading`} />
        ) : (
          texts.map((t, i) => <CallCode key={i} text={t} testId={`${testId}-code`} />)
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
      {format === "outline" ? (
        <>
          {copy && <div className="mt-4">{copy}</div>}
          {helpText && <div className="mt-4">{helpText}</div>}
        </>
      ) : (
        <>
          {helpText && <div className="mt-3">{helpText}</div>}
          {copy && <div className="mt-3">{copy}</div>}
        </>
      )}
    </aside>
  );
}

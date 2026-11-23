// A function's Monitor tab (lambda__function-monitor): the four metrics doze-aws publishes to AWS/Lambda for it (Invocations,
// Duration, Errors, Throttles), each as a figure and a line over the last three hours, and where its logs are.
import { ScrollTextIcon } from "lucide-react";
import { Link } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { Callout } from "@doze/ui/components/app/callout";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useResourceList } from "../../../api/resources";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { type MetricQuery, useMetricData } from "../../../kit";
import { Card } from "../deep/parts";
import { HOUR, lastHour, linePath, mean, milliseconds, pointsAt, total } from "./monitor-model";

const SPAN = 3 * HOUR;
const W = 280;
const H = 56;

function Tile({ label, value, note, path, dots, testId }: { label: string; value: string; note: React.ReactNode; path: string; dots: { x: number; y: number }[]; testId: string }) {
  return (
    <div className="rounded-[var(--radius-card)] border bg-card p-6" data-testid={testId}>
      <p className="text-[12px] font-medium tracking-[0.08em] text-muted-foreground uppercase">{label}</p>
      <p className="mt-2 text-[22px] leading-8 font-semibold text-bright tabular-nums" data-testid={`${testId}-value`}>{value}</p>
      <p className="mt-1 text-[13.5px] text-muted-foreground">{note}</p>
      <svg role="img" aria-label={`${label} over the last 3 hours`} viewBox={`0 0 ${W} ${H}`} width={W} height={H} className="mt-4 block max-w-full text-primary">
        <path d={`M0 ${H - 3} L${W} ${H - 3}`} fill="none" stroke="currentColor" strokeWidth="1" opacity="0.25" />
        <path d={path} fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinejoin="round" strokeLinecap="round" />
        {dots.map((d, i) => <circle key={i} cx={d.x} cy={d.y} r="2.5" fill="currentColor" />)}
      </svg>
    </div>
  );
}

export function MonitorTab({ d }: { d: ResourceDetail }) {
  const [now] = useState(() => Date.now());
  const range = useMemo(() => ({ from: now - SPAN, to: now }), [now]);
  const dimensions = { FunctionName: d.name };
  const queries: MetricQuery[] = useMemo(
    () => [
      { id: "inv", label: "Invocations", namespace: "AWS/Lambda", metric: "Invocations", dimensions, stat: "Sum", period: 300 },
      { id: "dur", label: "Duration", namespace: "AWS/Lambda", metric: "Duration", dimensions, stat: "Average", period: 300 },
      { id: "err", label: "Errors", namespace: "AWS/Lambda", metric: "Errors", dimensions, stat: "Sum", period: 300 },
      { id: "thr", label: "Throttles", namespace: "AWS/Lambda", metric: "Throttles", dimensions, stat: "Sum", period: 300 },
    ],
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [d.name],
  );
  const m = useMetricData(queries, range);
  const by = (id: string) => m.series.find((s) => s.id === id)?.points ?? [];
  const path = (id: string) => linePath(by(id), range.from, range.to, W, H);
  const dots = (id: string) => pointsAt(by(id), range.from, range.to, W, H);
  const inv = total(by("inv"));
  const err = total(by("err"));
  const thr = total(by("thr"));
  const dur = mean(by("dur"));
  const hour = lastHour(by("inv"), now);
  const group = `/aws/lambda/${d.name}`;
  // The log group is made by the first invocation: until it is there the button has nowhere to go.
  const groups = useResourceList("logs", "groups");
  const logsMade = !groups.isSuccess || groups.hasNextPage || groups.data.pages.some((pg) => pg.rows.some((r) => r.name === group));
  const n = (v: number | null) => (v === null ? "0" : String(Math.round(v)));
  return (
    <div className="grid gap-6" data-testid="monitor-tab">
      <div>
        <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">Metrics</h2>
        <p className="mt-1 text-[13.5px] text-muted-foreground">Lambda sends metric data to CloudWatch in 1-minute intervals. The lines show the last 3 hours.</p>
        {m.isPending ? (
          <Skeleton className="mt-5 h-72 rounded-[var(--radius-card)]" data-testid="metrics-loading" />
        ) : (
          <div className="mt-5 grid gap-4 md:grid-cols-2">
            <Tile testId="metric-invocations" label="Invocations" value={n(inv)} note={<>Sum. {n(hour)} in the last hour.</>} path={path("inv")} dots={dots("inv")} />
            <Tile testId="metric-duration" label="Duration" value={dur === null ? "–" : milliseconds(dur)} note="Average. Cold start time is not included." path={path("dur")} dots={dots("dur")} />
            <Tile testId="metric-errors" label="Errors" value={n(err)} note={<>Sum. {n(err)} {err === 1 ? "error" : "errors"} in {n(inv)} {inv === 1 ? "invocation" : "invocations"}.</>} path={path("err")} dots={dots("err")} />
            <Tile testId="metric-throttles" label="Throttles" value={n(thr)} note="Sum." path={path("thr")} dots={dots("thr")} />
          </div>
        )}
        <p className="mt-4 max-w-[46rem] border-l-2 pl-3.5 text-[13px] leading-5 text-foreground" data-testid="metrics-words">
          doze-aws publishes these four metrics to the <span className="font-mono">AWS/Lambda</span> namespace, by function name, and ConcurrentExecutions beside them (read it by Maximum).{" "}
          <span className="text-muted-foreground">Other Lambda metrics, such as DestinationDeliveryFailures, are not published. Throttles has data only once a call has been throttled.</span>
        </p>
      </div>
      <Card title="Logs" lede="Each execution environment writes its own log stream." actions={logsMade ? <Button asChild variant="outline" className="bg-card"><Link to="/resources/$service/$kind/$name" params={{ service: "logs", kind: "groups", name: group }} data-testid="view-logs"><ScrollTextIcon /> View CloudWatch logs</Link></Button> : <Button variant="outline" className="bg-card" disabled data-testid="view-logs"><ScrollTextIcon /> View CloudWatch logs</Button>} testId="logs-card">
        <dl className="grid grid-cols-[minmax(7rem,12rem)_minmax(0,1fr)] [&>div:last-child>*]:border-b-0">
          {[["Log group", group, true], ["Log streams", "Named YYYY/MM/DD[Function version][Execution environment GUID]", false], ["Lines per invocation", "START, END and REPORT, plus anything the code prints", false]].map(([k, v, mono]) => (
            <div key={String(k)} className="contents">
              <dt className="border-b py-3 pr-4 text-[13.5px] text-muted-foreground">{k}</dt>
              <dd className={`border-b py-3 text-[13.5px] text-bright ${mono ? "font-mono text-[13px]" : ""}`} data-testid={`logs-${String(k).toLowerCase().replace(/ /g, "-")}`}>{v}</dd>
            </div>
          ))}
        </dl>
        <Callout tone="note" className="mt-5" testId="logs-note">Metrics and logs are kept by CloudWatch, so this tab changes nothing in Lambda. View CloudWatch logs opens the log group on its own page{logsMade ? "" : "; it is made by the function's first invocation, so it is off until then"}.</Callout>
      </Card>
    </div>
  );
}

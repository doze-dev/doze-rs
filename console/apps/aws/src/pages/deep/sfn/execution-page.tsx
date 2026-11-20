// One execution: its status in a sentence, the machine's graph with the path it took coloured by status (choose a state for
// its input, output and error), its event history newest first with each event's details on opening, and its input and
// output. Start a new execution, or stop this one while it runs. Read with DescribeExecution,
// DescribeStateMachineForExecution (the definition it ran) and GetExecutionHistory, as the console.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { CircleStopIcon, PlayIcon } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@doze/ui/components/ui/tabs";
import { deepCall, useDeepRead } from "../../../api/deep";
import { CodeBlock, ReadWith, Section, pretty, when } from "../parts";
import { StartDialog, StopDialog } from "./dialogs";
import { statusOf } from "./executions-tab";
import { parseDefinition } from "./graph";
import { GraphView, Legend } from "./graph-view";
import { type HistoryEvent, readRun } from "./history";
import { EventsTable, StepDetails } from "./steps";

type Execution = { executionArn: string; stateMachineArn: string; name: string; status: string; startDate: number | string; stopDate?: number | string; input?: string; output?: string; error?: string; cause?: string };

/** The most history pages read (1,000 events each). */
const PAGES = 25;

/** Every event of an execution, oldest first. */
async function history(arn: string, signal: AbortSignal): Promise<HistoryEvent[]> {
  const out: HistoryEvent[] = [];
  let token: string | undefined;
  for (let i = 0; i < PAGES; i++) {
    const p = await deepCall<{ events?: HistoryEvent[]; nextToken?: string }>("stepfunctions", "GetExecutionHistory", { executionArn: arn, maxResults: 1000, ...(token ? { nextToken: token } : {}) }, signal);
    out.push(...(p.events ?? []));
    token = p.nextToken;
    if (!token) break;
  }
  return out;
}

/** The sentence under the title. */
export function executionSentence(e: Execution, machine: string, current: string | null, last: string | null): string {
  const started = `Started ${when(e.startDate)} on ${machine}.`;
  switch (e.status) {
    case "RUNNING":
      return `${started} ${current ? `It is running ${current}.` : "It is running."}`;
    case "SUCCEEDED":
      return `${started} It succeeded at ${when(e.stopDate)}.`;
    case "FAILED":
      return `${started} It failed${last ? ` at ${last}` : ""}${e.error ? ` with ${e.error}` : ""}.`;
    case "TIMED_OUT":
      return `${started} It timed out${last ? ` at ${last}` : ""}.`;
    case "ABORTED":
      return `${started} It was stopped${last ? ` at ${last}` : ""}, at ${when(e.stopDate)}.`;
    default:
      return `${started} Its status is ${e.status}.`;
  }
}

export function ExecutionPage({ arn }: { arn: string }) {
  const qc = useQueryClient();
  const exec = useDeepRead<Execution>("stepfunctions", "DescribeExecution", { executionArn: arn });
  const machine = useDeepRead<{ definition?: string; name?: string; stateMachineArn?: string }>("stepfunctions", "DescribeStateMachineForExecution", { executionArn: arn });
  const events = useQuery({ queryKey: ["deep", "stepfunctions", "history", arn], queryFn: ({ signal }) => history(arn, signal), retry: false, staleTime: Number.POSITIVE_INFINITY });
  const [selected, setSelected] = useState<string | undefined>();
  const [view, setView] = useState("graph");
  const [dialog, setDialog] = useState<"start" | "stop" | null>(null);
  const [opened, setOpened] = useState(0);
  const refresh = useCallback(() => void qc.invalidateQueries({ queryKey: ["deep", "stepfunctions"] }), [qc]);
  const e = exec.data;
  const run = useMemo(() => (e && events.data ? readRun(events.data, e.status) : undefined), [e, events.data]);
  const definition = useMemo(() => (machine.data?.definition ? parseDefinition(machine.data.definition) : null), [machine.data]);
  const steps = run ? [...run.steps.values()].sort((a, b) => a.entered - b.entered) : [];
  const current = steps.filter((s) => s.status === "running").map((s) => s.name).pop() ?? null;
  const last = steps[steps.length - 1]?.name ?? null;
  const machineName = machine.data?.name ?? e?.stateMachineArn.split(":").pop() ?? "";
  const status = e ? statusOf(e.status) : null;
  const open = (d: "start" | "stop") => {
    setOpened((n) => n + 1);
    setDialog(d);
  };
  const shownStep = selected ?? (e?.status === "FAILED" ? steps.find((s) => s.status === "failed")?.name : current) ?? undefined;
  return (
    <>
      <PageHeader
        title={e?.name ?? arn.split(":").pop() ?? arn}
        mono
        crumbs={[
          { label: "Resources", to: "/resources" },
          { label: "Step Functions", to: "/resources/stepfunctions" },
          { label: "State machines", to: "/resources/stepfunctions/machines" },
          ...(machineName ? [{ label: machineName, to: `/resources/stepfunctions/machines/${encodeURIComponent(machineName)}` }] : []),
        ]}
        sub={e && status ? <span className="inline-flex flex-wrap items-center gap-x-3 gap-y-1"><StatusDot tone={status.tone} data-testid="execution-status">{status.word}</StatusDot><span data-testid="execution-sentence">{executionSentence(e, machineName, current, last)}</span></span> : undefined}
        refresh={<RefreshControl updatedAt={exec.dataUpdatedAt} fetching={exec.isFetching || events.isFetching} onRefresh={refresh} />}
        actions={
          e && (
            <>
              <Button variant="outline" className="bg-card" onClick={() => open("start")} data-testid="start-open"><PlayIcon /> Start new execution</Button>
              {e.status === "RUNNING" && <Button variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" onClick={() => open("stop")} data-testid="stop-open"><CircleStopIcon /> Stop execution</Button>}
            </>
          )
        }
      />
      {exec.isError && !e ? (
        <ErrorBlock what="This execution could not be read" error={exec.error} onRetry={refresh} retrying={exec.isFetching} testId="execution-error" />
      ) : !e ? (
        <Skeleton className="h-80 rounded-[14px]" data-testid="execution-loading" />
      ) : (
        <div className="grid gap-6" data-testid="execution">
          {(e.error || e.cause) && (
            <div className="rounded-[12px] border border-bad/40 bg-bad/5 px-5 py-4 text-[13.5px]" data-testid="execution-failure">
              <b className="font-semibold text-bright">{e.error ?? "Failed"}</b>
              {e.cause && <CodeBlock text={pretty(e.cause)} className="mt-2 bg-card" max="max-h-48" />}
            </div>
          )}
          <Tabs value={view} onValueChange={setView} className="gap-0">
            <TabsList variant="line" className="mb-6 h-auto w-full justify-start gap-6 border-b p-0" aria-label="Views of this execution">
              {[["graph", "Graph"], ["events", `Events${events.data ? ` (${events.data.length})` : ""}`], ["io", "Input and output"]].map(([id, title]) => (
                <TabsTrigger key={id} value={id} className="flex-none px-0.5 pt-2 pb-3 text-[14px]" data-testid={`tab-${id}`}>{title}</TabsTrigger>
              ))}
            </TabsList>
            <TabsContent value="graph" className="grid items-start gap-6 xl:grid-cols-[minmax(0,1fr)_400px]">
              <Section title="Graph" lede={definition ? "The definition this execution ran. Choose a state to see what it took and gave." : undefined} testId="graph-card">
                {machine.error ? <ErrorBlock what="The definition could not be read" error={machine.error} /> : !definition || !events.data ? <Skeleton className="h-72 rounded-[12px]" /> : <GraphView definition={definition} run={run} selected={shownStep} onSelect={setSelected} />}
                <Legend />
              </Section>
              <StepDetails step={shownStep ? run?.steps.get(shownStep) : undefined} name={shownStep} events={events.data ?? []} />
            </TabsContent>
            <TabsContent value="events">
              <Section title="Event history" lede="Every event of the execution, the newest first. Open one for its details." testId="execution-events">
                {events.error ? <ErrorBlock what="The history could not be read" error={events.error} /> : !events.data ? <Skeleton className="h-72 rounded-[12px]" /> : <EventsTable events={events.data} />}
              </Section>
            </TabsContent>
            <TabsContent value="io" className="grid gap-6 lg:grid-cols-2">
              <Section title="Input" testId="execution-input"><CodeBlock text={pretty(e.input) || "None."} /></Section>
              <Section title="Output" testId="execution-output"><CodeBlock text={e.output ? pretty(e.output) : e.status === "RUNNING" ? "Not yet: it is running." : "None."} /></Section>
            </TabsContent>
          </Tabs>
          <Section title="Details" testId="execution-details">
            <dl className="grid grid-cols-[minmax(9rem,13rem)_minmax(0,1fr)] [&>div:last-child>*]:border-b-0">
              {[["Execution ARN", <span key="a" className="font-mono text-[13px] break-all">{e.executionArn}</span>], ["Started", when(e.startDate)], ["Stopped", when(e.stopDate)], ["State machine", <span key="m" className="font-mono text-[13px] break-all">{e.stateMachineArn}</span>]].map(([k, v]) => (
                <div key={String(k)} className="contents">
                  <dt className="border-b py-3 pr-4 text-[13.5px] text-muted-foreground">{k}</dt>
                  <dd className="border-b py-3 text-[13.5px] text-bright">{v}</dd>
                </div>
              ))}
            </dl>
            <ReadWith ops={["DescribeExecution", "DescribeStateMachineForExecution", "GetExecutionHistory"]} />
          </Section>
          <StartDialog key={`s${opened}`} machine={e.stateMachineArn} name={machineName} input={e.input ? pretty(e.input) : "{}"} open={dialog === "start"} onOpenChange={(o) => !o && setDialog(null)} />
          <StopDialog key={`x${opened}`} arn={e.executionArn} name={e.name} open={dialog === "stop"} onOpenChange={(o) => !o && setDialog(null)} />
        </div>
      )}
    </>
  );
}

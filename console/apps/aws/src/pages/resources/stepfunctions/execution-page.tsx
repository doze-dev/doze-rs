// One execution (stepfunctions__execution-running, execution-failed, execution-table, execution-events, execution-stop, execution-redrive):
// its status in a sentence, why it failed and the way to recover, its summary, and three views of what it did: the graph of the
// definition it ran coloured by status, a table of its steps, and its event history. Choose a step for its input, output, details,
// definition, Retry rules and events. Read with DescribeExecution, DescribeStateMachineForExecution and GetExecutionHistory.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { ChevronDownIcon, CircleAlertIcon, CircleStopIcon, PlayIcon, RotateCcwIcon } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Button } from "@doze/ui/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@doze/ui/components/ui/dropdown-menu";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { Tabs, TabsList, TabsTrigger } from "@doze/ui/components/ui/tabs";
import { DetailError, Segmented, useFeeders, withCurrent } from "../../../kit";
import { callOp, deepKey, machineOf, on, useOpRead } from "../deep/api";
import { at, Card, CodeBlock, pretty } from "../deep/parts";
import { RedriveDialog, StartDialog, StopDialog } from "./dialogs";
import { statusOf } from "./executions-tab";
import { executionWords, graphWords, redriveWarning, stepNote } from "./execution-model";
import { parseDefinition, statesOf } from "./graph";
import { GraphView, Legend } from "./graph-view";
import { type HistoryEvent, readRun, tsMs } from "./history";
import { machineHref, useWorkerCalls } from "./hooks";
import { aliasOf, taskResource, taskTarget, taskWords, versionOf } from "./model";
import { EventsView, StepDetails } from "./steps";
import { TableView } from "./table-view";

type Execution = { executionArn: string; stateMachineArn: string; stateMachineVersionArn?: string; stateMachineAliasArn?: string; name: string; status: string; startDate: number | string; stopDate?: number | string; input?: string; output?: string; error?: string; cause?: string; redriveCount?: number; redriveStatus?: string; redriveStatusReason?: string };

/** The most history pages read (1,000 events each). */
const PAGES = 25;

/** Every event of an execution, oldest first. */
async function history(arn: string, signal: AbortSignal): Promise<HistoryEvent[]> {
  const out: HistoryEvent[] = [];
  let token: string | undefined;
  for (let i = 0; i < PAGES; i++) {
    const p = await callOp<{ events?: HistoryEvent[]; nextToken?: string }>(on.machine(machineOf(arn)), "GetExecutionHistory", { executionArn: arn, maxResults: 1000, ...(token ? { nextToken: token } : {}) }, signal);
    out.push(...(p.events ?? []));
    token = p.nextToken;
    if (!token) break;
  }
  return out;
}

const when = (ms: number | null) => (ms === null ? "–" : at(ms));
const msOf = (v: number | string | undefined): number | null => (v === undefined || v === "" ? null : tsMs(v));

type View = "graph" | "table" | "events";
type Summary = "details" | "io" | "definition";

export function ExecutionPage({ arn }: { arn: string }) {
  const qc = useQueryClient();
  const t = on.machine(machineOf(arn));
  const exec = useOpRead<Execution>(t, "DescribeExecution", { executionArn: arn });
  const machine = useOpRead<{ definition?: string; name?: string; stateMachineArn?: string }>(t, "DescribeStateMachineForExecution", { executionArn: arn });
  const events = useQuery({ queryKey: deepKey(t, "history", arn), queryFn: ({ signal }) => history(arn, signal), retry: false, staleTime: Number.POSITIVE_INFINITY });
  const workers = useWorkerCalls();
  const [selected, setSelected] = useState<string | undefined>();
  const [view, setView] = useState<View>("graph");
  const [summary, setSummary] = useState<Summary>("details");
  const [dialog, setDialog] = useState<"start" | "stop" | "redrive" | null>(null);
  const [cause, setCause] = useState(false);
  const [opened, setOpened] = useState(0);
  const refresh = useCallback(() => void qc.invalidateQueries({ queryKey: deepKey(t) }), [qc, t]);
  const e = exec.data;
  const run = useMemo(() => (e && events.data ? readRun(events.data, e.status) : undefined), [e, events.data]);
  const definition = useMemo(() => (machine.data?.definition ? parseDefinition(machine.data.definition) : null), [machine.data]);
  const states = useMemo(() => statesOf(definition ?? undefined), [definition]);
  const steps = useMemo(() => (run ? [...run.steps.values()].sort((a, b) => a.entered - b.entered) : []), [run]);
  const current = steps.filter((s) => s.status === "running").map((s) => s.name).pop() ?? null;
  const last = steps[steps.length - 1]?.name ?? null;
  const failedStep = steps.find((s) => s.status === "failed");
  const machineName = machine.data?.name ?? e?.stateMachineArn.split(":").pop() ?? machineOf(arn);
  const status = e ? statusOf(e.status) : null;
  const version = versionOf(e?.stateMachineVersionArn);
  const alias = aliasOf(e?.stateMachineAliasArn);
  const via = alias ? `${alias}, version ${version ?? "?"}` : version ? `version ${version}` : "current revision";
  const open = (d: "start" | "stop" | "redrive") => {
    setOpened((n) => n + 1);
    setDialog(d);
  };
  const shownStep = selected ?? (failedStep ? failedStep.name : (current ?? undefined));
  const activityName = current && states[current] ? taskResource(states[current]?.Resource)?.activity : undefined;
  const pollers = useFeeders((workers.data?.calls ?? []).filter((c) => c.service === "stepfunctions" && (c.resource ?? "").endsWith(`:activity:${activityName ?? "\u0000"}`)).reverse(), ["GetActivityTask"]);
  const startMs = msOf(e?.startDate);
  const stopMs = msOf(e?.stopDate);
  const redrivable = e?.redriveStatus === "REDRIVABLE";
  const note = shownStep && run?.steps.get(shownStep) && e ? stepNote(definition ?? undefined, run.steps.get(shownStep) as never, { status: e.status, redrivable: redrivable && run.steps.get(shownStep)?.status === "failed", pollers }) : null;
  const left = useMemo(() => {
    const done = steps.filter((s) => s.status === "succeeded" && states[s.name]?.Type === "Task");
    if (done.length === 0) return undefined;
    const said = done.map((s) => `${s.name} (${taskWords(states[s.name] ?? {})}${taskTarget(states[s.name] ?? {}) ? ` on ${taskTarget(states[s.name] ?? {})}` : ""})`);
    return { text: `${said.join(", ")} already ran. Stopping does not undo what they did.`, names: done.map((s) => s.name) };
  }, [steps, states]);
  if (exec.isError && !e) return <DetailError service="stepfunctions" kind="executions" name={arn.split(":").pop() ?? arn} error={exec.error} onRetry={refresh} retrying={exec.isFetching} testId="execution-error" />;
  const sentence = e ? executionWords({ status: e.status, startMs, stopMs }, machineName, { version, alias }, { current, last, activity: Boolean(activityName) }, when) : null;
  const names = [machineName, ...(last ? [last] : []), ...(current ? [current] : [])];
  const bad = e && (e.status === "FAILED" || e.status === "TIMED_OUT") && (e.error || e.cause || failedStep);
  return (
    <>
      <PageHeader
        title={e?.name ?? arn.split(":").pop() ?? arn}
        mono
        crumbs={withCurrent([
          { label: "Resources", to: "/resources" },
          
          { label: "Step Functions", to: "/resources/stepfunctions" },
          { label: "State machines", to: "/resources/stepfunctions/machines" },
          ...(machineName ? [{ label: machineName, to: machineHref(machineName) }, { label: "Executions", to: machineHref(machineName) }] : []),
        ], e?.name ?? arn.split(":").pop() ?? arn)}
        titleExtra={status ? <StatusDot tone={status.tone} data-testid="execution-status">{status.word}</StatusDot> : undefined}
        sub={e && sentence ? <span data-testid="execution-sentence"><Emphasis text={sentence} names={names} /></span> : undefined}
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
      {!e ? (
        <Skeleton className="h-80 rounded-[var(--radius-card)]" data-testid="execution-loading" />
      ) : (
        <div className="grid gap-6" data-testid="execution">
          {/* The failure and the summary belong to the graph view; the table and the events are the views of the steps alone, as the boards draw them. */}
          {bad && view === "graph" && (
            <div role="alert" className="rounded-xl border border-bad/30 bg-bad/10 px-5 py-4 text-[14px] leading-[22px]" data-testid="execution-failure">
              <div className="flex items-start gap-3">
                <CircleAlertIcon aria-hidden className="mt-[3px] size-[18px] shrink-0 text-bad" />
                <div className="min-w-0 flex-1">
                  <p className="font-semibold text-bright">The execution {e.status === "TIMED_OUT" ? "timed out" : "failed"}{failedStep ? <> at <span data-testid="failed-at">{failedStep.name}</span></> : null}.</p>
                  {(e.error || e.cause) && <p className="mt-1 font-mono text-[13px] break-words" data-testid="failure-error">{e.error}{e.error && causeLine(e.cause) ? ": " : ""}{causeLine(e.cause)}</p>}
                  <div className="mt-3 flex flex-wrap items-center gap-2">
                    {e.cause && <Button type="button" variant="outline" size="sm" className="bg-card" aria-expanded={cause} onClick={() => setCause(!cause)} data-testid="failure-cause">Cause</Button>}
                    {failedStep && <Button type="button" variant="outline" size="sm" className="bg-card" onClick={() => { setView("graph"); setSelected(failedStep.name); setTimeout(() => document.querySelector("[data-testid=step-details]")?.scrollIntoView({ block: "start", behavior: "smooth" }), 60); }} data-testid="failure-step">View step details</Button>}
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild>
                        <Button type="button" variant="outline" size="sm" className="bg-card" data-testid="recover-menu">Recover <ChevronDownIcon /></Button>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="start" className="min-w-56">
                        <DropdownMenuItem disabled={!redrivable} onSelect={() => open("redrive")} data-testid="recover-redrive" title={redrivable ? undefined : (e.redriveStatusReason ?? "This execution cannot be redriven.")}><RotateCcwIcon /> Redrive from failure</DropdownMenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </div>
                  {cause && e.cause && <CodeBlock text={pretty(e.cause)} className="mt-3 bg-card" max="max-h-48" testId="failure-cause-text" />}
                </div>
              </div>
            </div>
          )}
          {view === "graph" && <Card title="Execution summary" actions={<Segmented value={summary} onChange={setSummary} options={[{ value: "details", label: "Details" }, { value: "io", label: "Execution input and output" }, { value: "definition", label: "Definition" }]} label="Execution summary" testId="summary-views" />} testId="execution-details">
            {summary === "details" && (
              <dl className="grid grid-cols-[minmax(9rem,13rem)_minmax(0,1fr)] [&>div:last-child>*]:border-b-0">
                {([
                  ["Execution ARN", <span key="a" className="font-mono text-[13px] break-all" data-testid="execution-arn">{e.executionArn}</span>],
                  ["Status", status ? <StatusDot key="s" tone={status.tone}>{status.word}</StatusDot> : null],
                  ["Start date", when(startMs)],
                  ...(stopMs !== null ? [["Stop date", when(stopMs)]] : []),
                  ["State machine", <Link key="m" to="/resources/$service/$kind/$name" params={{ service: "stepfunctions", kind: "machines", name: machineName }} className="rounded-sm font-mono text-[13px] text-link hover:underline">{machineName}</Link>],
                  ["Started with", via],
                  ...((e.redriveCount ?? 0) > 0 ? [["Redrive count", String(e.redriveCount)]] : []),
                ] as [string, React.ReactNode][]).map(([k, v]) => (
                  <div key={k} className="contents">
                    <dt className="border-b py-3 pr-4 text-[13.5px] text-muted-foreground">{k}</dt>
                    <dd className="border-b py-3 text-[13.5px] text-bright">{v}</dd>
                  </div>
                ))}
              </dl>
            )}
            {summary === "io" && (
              <div className="grid gap-6 lg:grid-cols-2">
                <div data-testid="execution-input"><h3 className="mb-2 text-[13.5px] font-medium text-bright">Input</h3><CodeBlock text={pretty(e.input) || "None."} /></div>
                <div data-testid="execution-output"><h3 className="mb-2 text-[13.5px] font-medium text-bright">Output</h3><CodeBlock text={e.output ? pretty(e.output) : e.status === "RUNNING" ? "Not yet: it is running." : "None."} /></div>
              </div>
            )}
            {summary === "definition" && (machine.data?.definition ? <CodeBlock text={pretty(machine.data.definition)} max="max-h-96" testId="execution-definition" /> : <Skeleton className="h-40 rounded-[var(--radius-panel)]" />)}
          </Card>}
          <Tabs value={view} onValueChange={(v) => setView(v as View)} className="gap-0">
            <TabsList variant="line" className="h-11 w-full justify-start gap-2 border-b p-0" aria-label="Views of this execution">
              {([["graph", "Graph view"], ["table", "Table view"], ["events", "Event view"]] as const).map(([id, title]) => (
                <TabsTrigger key={id} value={id} className="h-11 flex-none px-3 text-[14px] after:bottom-[-1px]" data-testid={`tab-${id}`}>{title}</TabsTrigger>
              ))}
            </TabsList>
          </Tabs>
          {view === "graph" && (
            <Card title="Graph view" lede={run && events.data ? graphWords(steps, e.status) : undefined} testId="graph-card">
              {machine.error ? <p className="text-[13.5px] text-bad">The definition could not be read.</p> : !definition || !events.data ? <Skeleton className="h-72 rounded-[var(--radius-panel)]" /> : <GraphView definition={definition} targets run={run} selected={shownStep} onSelect={setSelected} />}
              <Legend cancelled={steps.some((s) => s.status === "cancelled")} notRun={Object.keys(definition?.States ?? {}).some((n) => !steps.some((s) => s.name === n))} />
            </Card>
          )}
          {view === "table" && <TableView steps={steps} states={states} startMs={startMs} selected={shownStep} onSelect={setSelected} />}
          {view === "events" && (events.error ? <p className="text-[13.5px] text-bad">The history could not be read.</p> : !events.data ? <Skeleton className="h-72 rounded-[var(--radius-panel)]" /> : <EventsView events={events.data} startMs={startMs} />)}
          {view !== "events" && <StepDetails key={shownStep} step={shownStep ? run?.steps.get(shownStep) : undefined} name={shownStep} state={shownStep ? states[shownStep] : undefined} events={events.data ?? []} first={failedStep?.name === shownStep ? "details" : "input"} />}
          {view !== "events" && note && <p className="max-w-[46rem] border-l-2 border-input pl-3.5 text-[13px] leading-5 text-muted-foreground" data-testid="step-note"><Emphasis text={note.text} names={note.names} bold={[/\bHeartbeatSeconds \d+/]} /></p>}
          <StartDialog key={`s${opened}`} machine={e.stateMachineArn} name={machineName} startAt={definition?.StartAt} input={e.input ? pretty(e.input) : "{}"} open={dialog === "start"} onOpenChange={(o) => !o && setDialog(null)} />
          <StopDialog key={`x${opened}`} arn={e.executionArn} name={e.name} left={left} open={dialog === "stop"} onOpenChange={(o) => !o && setDialog(null)} />
          <RedriveDialog key={`r${opened}`} arn={e.executionArn} name={e.name} at={failedStep?.name ?? last} how={e.status === "TIMED_OUT" ? "timed out" : e.status === "ABORTED" ? "was stopped" : "failed"} warning={redriveWarning({ step: failedStep?.name ?? last ?? "The step", state: failedStep ? states[failedStep.name] : undefined, error: failedStep?.error ?? e.error, cause: failedStep?.cause ?? e.cause, input: failedStep?.input ?? e.input })} succeeded={steps.filter((s) => s.status === "succeeded").length} via={via} redrives={e.redriveCount ?? 0} open={dialog === "redrive"} onOpenChange={(o) => !o && setDialog(null)} />
        </div>
      )}
    </>
  );
}

/** The first line of an error's cause, or the message of a Lambda error's cause (`'email'`). */
function causeLine(cause: string | undefined): string {
  if (!cause) return "";
  try {
    const c = JSON.parse(cause) as { errorMessage?: unknown };
    if (typeof c.errorMessage === "string") return c.errorMessage;
  } catch {
    // As it is.
  }
  return cause.split("\n")[0] ?? cause;
}

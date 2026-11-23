// A dead-letter queue's redrive status (sqs__dlq-detail, dlq-redrive-running, -completed, -cancelled, -failed): the newest
// ListMessageMoveTasks entry for the queue in two columns, with Start, Refresh and (while RUNNING) Cancel under a rule.
import { useQueryClient } from "@tanstack/react-query";
import { RefreshCwIcon } from "lucide-react";
import { Link } from "@tanstack/react-router";
import { Alert } from "@doze/ui/components/app/alert";
import { Callout } from "@doze/ui/components/app/callout";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { dateTimeUtc } from "@doze/ui/lib/format";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { useOp, useOpQuery } from "../deep/api";
import { Refused } from "../deep/parts";
import { Pair } from "./parts";
import type { Queue } from "./queue-model";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { allowWords, cancellable, destinationWords, latest, rateWords, RUNNING, statusNote, statusTone, type Task } from "./redrive-model";

export const TASKS_INPUT = { MaxResults: 10 };

/** How often a task that is running is read again. */
export const TASK_POLL_MS = 2000;

/** The newest move tasks of the queue, read when the page opens and when Refresh is pressed; while the newest is RUNNING, every
 * `TASK_POLL_MS` too, so its count moves and its end shows without a click. */
export function useTasks(d: ResourceDetail, enabled = true) {
  const t = { service: d.service, kind: d.kind, name: d.name };
  return useOpQuery<{ Results?: Task[] }>(t, "ListMessageMoveTasks", TASKS_INPUT, ["tasks"], enabled, (data) => (latest(data?.Results)?.Status === RUNNING ? TASK_POLL_MS : false));
}

const grid = "grid content-start [&>div:last-child>*]:border-b-0";
const cols = "grid-cols-[minmax(9rem,12rem)_minmax(0,1fr)]";

export function RedriveStatus({ d, q, sources, go }: { d: ResourceDetail; q: Queue; sources: string[]; go: (view: string) => void }) {
  const t = { service: d.service, kind: d.kind, name: d.name };
  const tasks = useTasks(d);
  const cancel = useOp(t, "CancelMessageMoveTask");
  const qc = useQueryClient();
  const task = latest(tasks.data?.Results);
  const running = task?.Status === RUNNING;
  const note = task ? statusNote(task, q.name) : null;
  const refresh = () => {
    void tasks.refetch();
    void qc.invalidateQueries({ queryKey: ["resources", "one", d.service, d.kind, d.name] });
  };
  // A task can end between the last read and the click: read it again first, and cancel only one that is still running (AWS refuses the
  // cancel of a finished one, and a refusal for something the person did right is noise).
  const stop = async (handle: string | undefined) => {
    const again = latest((await tasks.refetch()).data?.Results);
    if (!cancellable(again, handle)) return;
    cancel.mutate({ TaskHandle: handle }, { onSuccess: () => void tasks.refetch() });
  };
  const row = (label: string, v: React.ReactNode) => <Pair key={label} label={label} testId={`redrive-${label}`}>{v}</Pair>;
  const left = task
    ? [
        row("Status", <StatusDot tone={statusTone(task.Status)} className="text-[13.5px]">{task.Status}</StatusDot>),
        row("Messages moved", `${(task.ApproximateNumberOfMessagesMoved ?? 0).toLocaleString("en-US")} of ${(task.ApproximateNumberOfMessagesToMove ?? 0).toLocaleString("en-US")}`),
        row("Destination", destinationWords(task)),
        row("Rate", rateWords(task)),
      ]
    : [
        row("Status", <StatusDot tone="idle" className="text-[13.5px]">No tasks</StatusDot>),
        row("Source queues", <span className="flex flex-wrap gap-1.5">{sources.map((s) => <Link key={s} to="/resources/$service/$kind/$name" params={{ service: "sqs", kind: "queues", name: s }} className="rounded-full border bg-well px-2.5 py-0.5 font-mono text-[12.5px] hover:bg-accent">{s}</Link>)}</span>),
      ];
  const right = task
    ? [
        ...(task.StartedTimestamp ? [row("Started", dateTimeUtc(task.StartedTimestamp))] : []),
        row("Source", <span className="font-mono">{task.SourceArn}</span>),
        row("Task handle", task.TaskHandle ? <span className="font-mono">{task.TaskHandle.slice(0, 24)}…</span> : "Not returned (only while RUNNING)"),
        ...(task.FailureReason ? [row("Failure reason", <span className="font-mono">{task.FailureReason}</span>)] : []),
      ]
    : [row("Redrive allow policy", allowWords(q.allow)), row("Approx. messages", q.available.toLocaleString("en-US"))];
  return (
    <section className="rounded-[14px] border bg-card p-6 lg:p-7" aria-label="Dead-letter queue redrive status" data-testid="redrive-status">
      <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">Dead-letter queue redrive status</h2>
      <p className="mt-1 text-[13.5px] text-muted-foreground">{task ? "The fields of the most recent ListMessageMoveTasks entry for this queue." : "The status of the most recent redrive task for this queue, from ListMessageMoveTasks."}</p>
      <Refused operation="CancelMessageMoveTask" error={cancel.error} />
      {tasks.isPending ? (
        <Skeleton className="mt-5 h-32 rounded-[14px]" />
      ) : tasks.error ? (
        <Alert tone="bad" title="ListMessageMoveTasks was refused" className="mt-5">{String((tasks.error as Error).message)}</Alert>
      ) : (
        <div className="mt-4 grid lg:grid-cols-2 lg:gap-x-12" data-testid="redrive-fields" data-status={task?.Status ?? "none"}>
          <dl className={`${grid} ${cols}`}>{left}</dl>
          <dl className={`${grid} ${cols}`}>{right}</dl>
        </div>
      )}
      {note && <Callout tone={task?.Status === "FAILED" ? "bad" : "note"} lead={note.lead} className="mt-5" testId="redrive-note"><Emphasis text={note.text} bold={["Redrive to custom destination"]} /></Callout>}
      <div className="mt-6 flex flex-wrap gap-2 border-t pt-6">
        {running && task?.TaskHandle && (
          <Button type="button" variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={cancel.isPending} data-testid="redrive-cancel" onClick={() => void stop(task.TaskHandle)}>Cancel DLQ redrive</Button>
        )}
        <Button type="button" variant="outline" className="bg-card" onClick={() => go("redrive")} disabled={running} title={running ? "Only one task can run on a queue at a time" : undefined} data-testid="redrive-start">Start DLQ redrive</Button>
        <Button type="button" variant="outline" className="bg-card" onClick={refresh} disabled={tasks.isFetching} data-testid="redrive-refresh"><RefreshCwIcon className={tasks.isFetching ? "animate-spin" : ""} /> Refresh</Button>
      </div>
      {running && <p className="mt-4 border-l-2 pl-3 text-[13px] leading-5 text-muted-foreground">Only a RUNNING task can be cancelled. Cancelling does not undo the move: messages already sent to the destination stay there. A task is FAILED when a message had nowhere to return to; the status then shows the failure reason.</p>}
    </section>
  );
}

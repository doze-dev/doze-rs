// A dead-letter queue's redrive status (sqs__dlq-detail, dlq-redrive-running, -completed, -cancelled, -failed): the newest
// ListMessageMoveTasks entry for the queue, with Start, Refresh and (while RUNNING) Cancel.
import { useQueryClient } from "@tanstack/react-query";
import { RefreshCwIcon } from "lucide-react";
import { useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { dateTimeUtc } from "@doze/ui/lib/format";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { useOp, useOpQuery } from "../deep/api";
import { Refused } from "../deep/parts";
import { Block } from "./parts";
import type { Queue } from "./queue-model";
import { destinationWords, latest, rateWords, RUNNING, statusNote, type Task } from "./redrive-model";

export const TASKS_INPUT = { MaxResults: 10 };

/** The newest move tasks of the queue, read when the page opens and when Refresh is pressed. */
export function useTasks(d: ResourceDetail, enabled = true) {
  const t = { service: d.service, kind: d.kind, name: d.name };
  return useOpQuery<{ Results?: Task[] }>(t, "ListMessageMoveTasks", TASKS_INPUT, ["tasks"], enabled);
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <>
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="min-w-0 break-words text-bright" data-testid={`redrive-${label}`}>{children}</dd>
    </>
  );
}

export function RedriveStatus({ d, q, sources, go }: { d: ResourceDetail; q: Queue; sources: string[]; go: (view: string) => void }) {
  const t = { service: d.service, kind: d.kind, name: d.name };
  const tasks = useTasks(d);
  const cancel = useOp(t, "CancelMessageMoveTask");
  const qc = useQueryClient();
  const [cancelled, setCancelled] = useState<string | null>(null);
  const task = latest(tasks.data?.Results);
  const running = task?.Status === RUNNING;
  const note = task ? statusNote(task) : null;
  const refresh = () => {
    setCancelled(null);
    void tasks.refetch();
    void qc.invalidateQueries({ queryKey: ["resources", "one", d.service, d.kind, d.name] });
  };
  return (
    <Block title="Dead-letter queue redrive status" lede={task ? "The fields of the most recent ListMessageMoveTasks entry for this queue." : "The status of the most recent redrive task for this queue, from ListMessageMoveTasks."} testId="redrive-status" actions={
      <div className="flex flex-wrap gap-2">
        {running && task?.TaskHandle && (
          <Button type="button" variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={cancel.isPending} data-testid="redrive-cancel" onClick={() => cancel.mutate({ TaskHandle: task.TaskHandle }, { onSuccess: () => { setCancelled(task.TaskHandle ?? ""); void tasks.refetch(); } })}>Cancel DLQ redrive</Button>
        )}
        <Button type="button" variant="outline" className="bg-card" onClick={() => go("redrive")} disabled={running} title={running ? "Only one task can run on a queue at a time" : undefined} data-testid="redrive-start">Start DLQ redrive</Button>
        <Button type="button" variant="outline" className="bg-card" onClick={refresh} disabled={tasks.isFetching} data-testid="redrive-refresh"><RefreshCwIcon className={tasks.isFetching ? "animate-spin" : ""} /> Refresh</Button>
      </div>
    }>
      {cancelled !== null && <ResultBanner tone="info" title="Redrive cancelled." returned="CancelMessageMoveTask stopped the task." left="Messages it had already moved stay at the destination." onDismiss={() => setCancelled(null)} testId="redrive-cancelled" />}
      <Refused operation="CancelMessageMoveTask" error={cancel.error} />
      {tasks.isPending ? (
        <Skeleton className="h-32 rounded-[14px]" />
      ) : tasks.error ? (
        <Alert tone="bad" title="ListMessageMoveTasks was refused">{String((tasks.error as Error).message)}</Alert>
      ) : (
        <dl className="grid grid-cols-[minmax(9rem,12rem)_minmax(0,1fr)] gap-x-6 gap-y-3 rounded-[14px] border bg-card px-6 py-5 text-[13.5px]" data-testid="redrive-fields" data-status={task?.Status ?? "none"}>
          {task ? (
            <>
              <Row label="Status"><span className="font-mono font-semibold">{task.Status}</span></Row>
              <Row label="Messages moved">{(task.ApproximateNumberOfMessagesMoved ?? 0).toLocaleString("en-US")} of {(task.ApproximateNumberOfMessagesToMove ?? 0).toLocaleString("en-US")}</Row>
              <Row label="Destination">{destinationWords(task)}</Row>
              <Row label="Rate">{rateWords(task)}</Row>
              {task.StartedTimestamp ? <Row label="Started">{dateTimeUtc(task.StartedTimestamp)}</Row> : null}
              <Row label="Source"><span className="font-mono">{task.SourceArn}</span></Row>
              <Row label="Task handle">{task.TaskHandle ? <span className="font-mono">{task.TaskHandle.slice(0, 24)}…</span> : "Not returned (only while RUNNING)"}</Row>
              {task.FailureReason && <Row label="Failure reason"><span className="font-mono">{task.FailureReason}</span></Row>}
            </>
          ) : (
            <>
              <Row label="Status">No tasks</Row>
              <Row label="Source queues">{sources.join(", ")}</Row>
              <Row label="Approx. messages">{q.available.toLocaleString("en-US")}</Row>
            </>
          )}
        </dl>
      )}
      {note && <Alert tone={task?.Status === "FAILED" ? "bad" : "info"} title={`Status ${task?.Status}.`} testId="redrive-note">{note}</Alert>}
      {running && <p className="text-[13px] text-muted-foreground">Only a RUNNING task can be cancelled. Cancelling does not undo the move: messages already sent to the destination stay there. A task is FAILED when a message had nowhere to return to; the status then shows the failure reason.</p>}
    </Block>
  );
}

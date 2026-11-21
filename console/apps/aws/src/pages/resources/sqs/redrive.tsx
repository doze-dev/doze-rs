// A dead-letter queue's redrive: move its messages back to the queues they came from, or to another queue
// (StartMessageMoveTask), and the moves of the last two weeks (ListMessageMoveTasks), a running one cancelled
// (CancelMessageMoveTask). Shown on a queue that some queue uses as its dead-letter queue.
import { RefreshCwIcon, Undo2Icon } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { Alert } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { dateTimeUtc, number } from "@doze/ui/lib/format";
import { cn } from "@doze/ui/lib/utils";
import { useOp, useOpQuery } from "../deep/api";
import type { DeepProps } from "../deep/registry";
import { Card, Made, Refused } from "../deep/parts";
import { failedNote, type MoveTask } from "./redrive-note";
import { fieldOf } from "./messages";

type Task = MoveTask;

const tail = (arn?: string) => (arn ? arn.slice(arn.lastIndexOf(":") + 1) : "");

const TONE: Record<string, string> = { RUNNING: "text-cool", COMPLETED: "text-good", CANCELLING: "text-warn", CANCELLED: "text-muted-foreground", FAILED: "text-bad" };

export function QueueRedrive({ d }: DeepProps) {
  const t = { service: d.service, kind: d.kind, name: d.name };
  const sources = d.subs.find((s) => s.title.includes("dead-letter queue"))?.rows.map((r) => r.name) ?? [];
  const [to, setTo] = useState<"source" | "custom">("source");
  const [queue, setQueue] = useState("");
  const [rate, setRate] = useState("");
  const tasks = useOpQuery<{ Results?: Task[] }>(t, "ListMessageMoveTasks", { MaxResults: 10 }, ["tasks"]);
  const start = useOp<{ TaskHandle: string }>(t, "StartMessageMoveTask");
  const cancel = useOp(t, "CancelMessageMoveTask");
  const arn = fieldOf(d, "ARN") ?? "";
  const arnOf = (q: string) => `${arn.slice(0, arn.lastIndexOf(":") + 1)}${q}`;
  const wrong = to === "custom" && !queue.trim() ? "Name the queue to move the messages to." : rate.trim() && !(Number.isInteger(Number(rate)) && Number(rate) >= 1 && Number(rate) <= 500) ? "Messages per second: a whole number from 1 to 500." : null;
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (wrong || start.isPending) return;
    const input: Record<string, unknown> = {};
    if (to === "custom") input.DestinationArn = arnOf(queue.trim());
    if (rate.trim()) input.MaxNumberOfMessagesPerSecond = Number(rate);
    start.mutate(input, { onSuccess: () => toast.success("Redrive started", { description: "Refresh the moves to follow it." }) });
  };
  const failed = failedNote(tasks.data?.Results?.[0]);
  const columns: DataColumn<Task>[] = [
    { id: "status", header: "Status", size: 140, accessorKey: "Status", cell: (c) => <span className={cn("font-medium", TONE[c.row.original.Status])}>{c.row.original.Status.toLowerCase()}</span> },
    { id: "started", header: "Started (UTC)", size: 190, min: 160, accessorFn: (r) => r.StartedTimestamp ?? 0, cell: (c) => (c.row.original.StartedTimestamp ? dateTimeUtc(c.row.original.StartedTimestamp) : "–") },
    { id: "to", header: "To", accessorFn: (r) => tail(r.DestinationArn), meta: { mono: true }, cell: (c) => tail(c.row.original.DestinationArn) || <span className="text-muted-foreground">their source queues</span> },
    { id: "moved", header: "Moved", size: 150, min: 110, meta: { numeric: true }, accessorFn: (r) => r.ApproximateNumberOfMessagesMoved ?? 0, cell: (c) => `${number(c.row.original.ApproximateNumberOfMessagesMoved ?? 0)} of ${number(c.row.original.ApproximateNumberOfMessagesToMove ?? 0)}` },
    { id: "why", header: "Failure", size: 220, min: 110, accessorFn: (r) => r.FailureReason ?? "", cell: (c) => c.row.original.FailureReason ?? "–" },
    {
      id: "act",
      header: "",
      size: 110,
      enableSorting: false,
      cell: (c) =>
        c.row.original.Status === "RUNNING" && c.row.original.TaskHandle ? (
          <Button variant="outline" size="sm" className="bg-card" disabled={cancel.isPending} onClick={() => cancel.mutate({ TaskHandle: c.row.original.TaskHandle }, { onSuccess: () => toast.success("Cancelling the redrive") })} data-testid="cancel-move">
            Cancel
          </Button>
        ) : null,
    },
  ];
  return (
    <div className="grid gap-6">
      <Card title="Start a redrive" lede={<>Messages in <span className="font-mono">{d.name}</span> move out at once, oldest first. Queues that use it as their dead-letter queue: <span className="font-mono">{sources.join(", ")}</span>.</>} testId="redrive-card">
        <form onSubmit={submit} className="grid max-w-[720px] gap-5">
          <RadioGroup value={to} onValueChange={(v) => setTo(v as "source" | "custom")} className="grid gap-3">
            <div className="flex items-start gap-3">
              <RadioGroupItem value="source" id="to-source" className="mt-0.5" />
              <Label htmlFor="to-source" className="grid gap-0.5 leading-5 font-normal">
                <span className="font-medium">Back to their source queues</span>
                <span className="text-[12.5px] leading-[18px] text-muted-foreground">Each message returns to the queue it was dead-lettered from. A message sent straight to this queue has none, and the move fails.</span>
              </Label>
            </div>
            <div className="flex items-start gap-3">
              <RadioGroupItem value="custom" id="to-custom" className="mt-0.5" />
              <Label htmlFor="to-custom" className="grid gap-0.5 leading-5 font-normal">
                <span className="font-medium">To another queue</span>
                <span className="text-[12.5px] leading-[18px] text-muted-foreground">A queue of this account and region.</span>
              </Label>
            </div>
          </RadioGroup>
          {to === "custom" && <Input aria-label="Destination queue" value={queue} onChange={(e) => setQueue(e.target.value)} placeholder={`e.g. ${sources[0] ?? "orders"}`} className="max-w-sm bg-card font-mono text-[13px]" data-testid="redrive-queue" />}
          <div className="grid max-w-56 gap-1.5">
            <Label htmlFor="redrive-rate">
              Messages per second<span className="ml-1.5 font-normal text-muted-foreground">optional</span>
            </Label>
            <Input id="redrive-rate" inputMode="numeric" value={rate} onChange={(e) => setRate(e.target.value)} placeholder="e.g. 50" className="bg-card font-mono text-[13px]" />
            <p className="text-[12.5px] text-muted-foreground">1 to 500. Default: as fast as the destination takes them.</p>
          </div>
          {wrong && (queue || rate) ? <p className="text-[13px] text-bad">{wrong}</p> : null}
          <Refused operation="StartMessageMoveTask" error={start.error} testId="redrive-error" />
          <div className="flex items-center justify-between gap-3 border-t pt-5">
            <Made ops={["StartMessageMoveTask"]} />
            <Button type="submit" disabled={!!wrong || start.isPending} data-testid="redrive-start">
              <Undo2Icon /> {start.isPending ? "Starting…" : "Start redrive"}
            </Button>
          </div>
        </form>
      </Card>
      <Card
        title="Moves"
        lede={<>The redrives of the last 14 days, newest first, read with <span className="font-mono">ListMessageMoveTasks</span> when this page opened or when refreshed.</>}
        actions={
          <Button variant="outline" size="sm" className="bg-card" onClick={() => void tasks.refetch()} disabled={tasks.isFetching} data-testid="moves-refresh">
            <RefreshCwIcon className={cn(tasks.isFetching && "animate-spin")} /> Refresh
          </Button>
        }
        testId="moves-card"
      >
        <Refused operation="ListMessageMoveTasks" error={tasks.error} />
        {failed ? (
          <Alert tone="bad" title={failed.title} testId="move-failed">
            {failed.body}
          </Alert>
        ) : null}
        <Refused operation="CancelMessageMoveTask" error={cancel.error} />
        <DataTable aria-label="Moves" data={tasks.data?.Results ?? []} columns={columns} rowId={(r) => r.TaskHandle ?? `${r.StartedTimestamp}`} maxHeight={360} empty={tasks.isPending ? "Reading…" : "No redrive of this queue in the last 14 days."} />
      </Card>
    </div>
  );
}

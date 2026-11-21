// Start an execution (a name, optional, and its input as JSON) and stop a running one: each a dialog over its page, each one
// call as the console (StartExecution, StopExecution) that Live shows.
import { useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { toast } from "sonner";
import { JsonEditor, checkJson } from "@doze/ui/components/app/json-editor";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { machineOf, on, useOp } from "../api";
import { Refused } from "../parts";

/** A name StartExecution takes: 1 to 80 characters, none of the ones AWS refuses (whitespace, brackets, wildcards, punctuation it reserves). */
export const NAME = /^[^\s<>{}[\]?*"#%\\^|~`$&,;:/]{1,80}$/;

export function StartDialog({ machine, name, open, onOpenChange, input: first = "{}" }: { machine: string; name: string; open: boolean; onOpenChange: (o: boolean) => void; input?: string }) {
  const [run, setRun] = useState("");
  const [input, setInput] = useState(first);
  const start = useOp<{ executionArn: string; startDate: number }>(on.machine(name), "StartExecution");
  const navigate = useNavigate();
  const check = checkJson(input, "invalid");
  const named = run.trim() === "" || NAME.test(run.trim());
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!check.ok || !named || start.isPending) return;
    start.mutate(
      { stateMachineArn: machine, input, ...(run.trim() ? { name: run.trim() } : {}) },
      {
        onSuccess: (r) => {
          onOpenChange(false);
          toast.success("Execution started");
          void navigate({ to: "/resources/$service/$kind/$name", params: { service: "stepfunctions", kind: "executions", name: r.executionArn }, search: {} });
        },
      },
    );
  };
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[680px]" data-testid="start-dialog">
        <form onSubmit={submit} className="grid gap-5">
          <DialogHeader>
            <DialogTitle className="text-[21px] font-semibold">Start execution</DialogTitle>
            <DialogDescription>
              Starts the current revision of <span className="font-mono text-foreground">{name}</span>.
            </DialogDescription>
          </DialogHeader>
          <div className="grid gap-1.5">
            <Label htmlFor="start-name">
              Name <span className="ml-1 font-normal text-muted-foreground">optional</span>
            </Label>
            <Input id="start-name" value={run} onChange={(e) => setRun(e.target.value)} placeholder="e.g. order-0421" aria-invalid={!named} className="bg-card font-mono" spellCheck={false} data-testid="start-name" />
            <p className={named ? "text-[12.5px] text-muted-foreground" : "text-[12.5px] text-bad"}>Leave it empty for a generated name. Up to 80 characters, no spaces, brackets or wildcards; unique among the machine's executions.</p>
          </div>
          <JsonEditor label="Input" value={input} onChange={setInput} empty="invalid" rows={8} testId="start-input" help="The JSON the first state receives. Send {} when there is none. Up to 256 KiB." />
          <Refused error={start.error} operation="StartExecution" testId="write-error" />
          <DialogFooter>
            <Button type="button" variant="outline" className="bg-card" onClick={() => onOpenChange(false)}>Cancel</Button>
            <Button type="submit" disabled={!check.ok || !named || start.isPending} data-testid="start-submit">{start.isPending ? "Starting…" : "Start execution"}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function StopDialog({ arn, name, open, onOpenChange }: { arn: string; name: string; open: boolean; onOpenChange: (o: boolean) => void }) {
  const [cause, setCause] = useState("");
  const stop = useOp<{ stopDate: number }>(on.machine(machineOf(arn)), "StopExecution");
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (stop.isPending) return;
    stop.mutate({ executionArn: arn, ...(cause.trim() ? { cause: cause.trim() } : {}) }, { onSuccess: () => { onOpenChange(false); toast.success("Execution stopped"); } });
  };
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[600px]" data-testid="stop-dialog">
        <form onSubmit={submit} className="grid gap-5">
          <DialogHeader>
            <DialogTitle className="text-[21px] font-semibold">Stop execution</DialogTitle>
            <DialogDescription className="text-[14px] leading-[22px] text-foreground">
              Stop <b className="font-mono font-semibold break-all text-bright">{name}</b> while it runs? It ends with the status Aborted.
            </DialogDescription>
          </DialogHeader>
          <p className="border-l-2 border-input pl-3.5 text-[13px] leading-5 text-foreground">
            Stopping does not undo what finished states already did: a message sent stays sent. <span className="text-muted-foreground">Called as <span className="font-mono">StopExecution</span>; Live shows it.</span>
          </p>
          <div className="grid gap-1.5">
            <Label htmlFor="stop-cause">
              Cause <span className="ml-1 font-normal text-muted-foreground">optional</span>
            </Label>
            <Input id="stop-cause" value={cause} onChange={(e) => setCause(e.target.value)} placeholder="e.g. stopped by hand" className="bg-card" />
          </div>
          <Refused error={stop.error} operation="StopExecution" testId="write-error" />
          <DialogFooter>
            <Button type="button" variant="outline" className="bg-card" onClick={() => onOpenChange(false)}>Cancel</Button>
            <Button type="submit" variant="destructive" disabled={stop.isPending} data-testid="stop-submit">{stop.isPending ? "Stopping…" : "Stop execution"}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

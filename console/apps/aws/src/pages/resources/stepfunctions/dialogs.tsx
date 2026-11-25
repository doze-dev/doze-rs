// Start an execution (a name, optional, and its input as JSON), stop a running one and redrive a failed one: each a dialog over its
// page, each one call as the console (StartExecution, StopExecution, RedriveExecution) that Live shows.
import { useNavigate } from "@tanstack/react-router";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { Callout } from "@doze/ui/components/app/callout";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { JsonEditor, checkJson } from "@doze/ui/components/app/json-editor";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Input } from "@doze/ui/components/ui/input";
import { Field } from "../../../kit";
import { machineOf, on, useOp } from "../deep/api";
import { pretty, Refused } from "../deep/parts";
import { useLastGoodInput } from "./hooks";
import { NAME, nameProblem } from "./model";

export { NAME };

/** The words under the input: what it is for, and where what it opens with came from (`from`: the run it was read from; `null`: no run has succeeded, so it is a skeleton; `undefined`: nothing to say). */
function InputHelp({ startAt, from }: { startAt?: string; from: string | null | undefined }) {
  const state = startAt ? <b className="font-mono font-semibold">{startAt}</b> : "the first state";
  return (
    <>
      The JSON that {state} receives. Send <span className="font-mono">{"{}"}</span> when there is no input. Up to 256 KiB as UTF-8.
      {from ? <> It opens with the input of <b className="font-mono font-semibold">{from}</b>, the newest execution that succeeded.</> : null}
      {from === null ? <> No execution has succeeded yet, so it opens with the paths {state} reads, each holding its own name: replace them with values that exist.</> : null}
    </>
  );
}

/** What an execution is started from: the machine's current revision, one of its versions, or an alias. */
export type StartTarget = { arn: string; words: React.ReactNode };

export function StartDialog({ machine, name, open, onOpenChange, input: first = "{}", target, startAt, reuse = false }: { machine: string; name: string; open: boolean; onOpenChange: (o: boolean) => void; /** What the input opens with. */ input?: string; target?: StartTarget; /** The state the execution begins at, which the input is for. */ startAt?: string; /** `input` is a skeleton: the input of the newest run that succeeded replaces it once read, unless it was edited. */ reuse?: boolean }) {
  const [run, setRun] = useState("");
  const [input, setInput] = useState(first);
  const [touched, setTouched] = useState(false);
  const last = useLastGoodInput(name, open && reuse).data;
  useEffect(() => {
    if (last && !touched) setInput(pretty(last.input));
  }, [last, touched]);
  const start = useOp<{ executionArn: string; startDate: number }>(on.machine(name), "StartExecution");
  const navigate = useNavigate();
  const check = checkJson(input, "invalid");
  const named = run.trim() === "" || nameProblem(run.trim()) === null;
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!check.ok || !named || start.isPending) return;
    start.mutate(
      { stateMachineArn: target?.arn ?? machine, input, ...(run.trim() ? { name: run.trim() } : {}) },
      {
        onSuccess: (r) => {
          onOpenChange(false);
          // An Express execution keeps no history to open: its events go to a log group only if the machine's logging is on.
          if (r.executionArn.includes(":express:")) {
            toast.success("Express execution started", { description: "It keeps no history to open. Its events are written to a log group only if the machine's logging is on." });
            return;
          }
          void navigate({ to: "/resources/$service/$kind/$name", params: { service: "stepfunctions", kind: "executions", name: r.executionArn }, search: { done: "started" } });
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
            <DialogDescription className="text-[14px] leading-[22px] text-foreground" data-testid="start-words">
              {target ? target.words : <>Starts the current revision of <span className="font-mono font-semibold text-bright">{name}</span>. To start a version or an alias instead, choose it on the <b className="font-semibold">Versions</b> or <b className="font-semibold">Aliases</b> tab.</>}
            </DialogDescription>
          </DialogHeader>
          <Field label="Name" optional error={named ? undefined : (nameProblem(run.trim()) ?? undefined)} hint="Leave it empty for a generated name. A name is unique per state machine and can be used again 90 days after its execution closes. Starting the same name with the same input while it runs returns that run instead of a second one.">
            {(id) => <Input id={id} value={run} onChange={(e) => setRun(e.target.value)} placeholder="e.g. order-0421" aria-invalid={!named} className="bg-card font-mono" spellCheck={false} data-testid="start-name" />}
          </Field>
          <JsonEditor label="Input" value={input} onChange={(v) => { setTouched(true); setInput(v); }} empty="invalid" rows={8} testId="start-input" help={<InputHelp startAt={startAt} from={reuse && !touched ? (last ? last.name : last === null && first !== "{}" ? null : undefined) : undefined} />} />
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

/** Stop an execution while it runs (stepfunctions__execution-stop): what stopping does and does not undo. `sent` is what the steps
 * that already ran left behind (a pick list in a queue). */
export function StopDialog({ arn, name, open, onOpenChange, left }: { arn: string; name: string; open: boolean; onOpenChange: (o: boolean) => void; left?: { text: string; names: string[] } }) {
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
              Stop <b className="font-mono font-semibold break-all text-bright">{name}</b> while it runs? The execution ends with the status <b className="font-semibold">Aborted</b>.
            </DialogDescription>
          </DialogHeader>
          {left && <Callout tone="warn" testId="stop-left"><Emphasis text={left.text} names={left.names} /></Callout>}
          <p className="border-l-2 border-input pl-3.5 text-[13px] leading-5 text-foreground">
            Only Standard executions can be stopped, and only while they run. An aborted execution can be redriven for 14 days after it ends. Stop does not undo what finished steps already did.
          </p>
          <Field label="Cause" optional>
            {(id) => <Input id={id} value={cause} onChange={(e) => setCause(e.target.value)} placeholder="e.g. stopped by hand" className="bg-card" />}
          </Field>
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

/** Redrive an execution that failed, timed out or was stopped (stepfunctions__execution-redrive): it carries on from the step that
 * stopped, with the same input, as the same execution. */
export function RedriveDialog({ arn, name, at, how, succeeded, warning, via, redrives, open, onOpenChange }: { arn: string; name: string; /** The step it stopped at. */ at: string | null; /** How it stopped: `failed`, `timed out`, `stopped`. */ how: string; succeeded: number; warning: { lead: string; text: string }; /** `PROD, version 2`, or `current revision`. */ via: string; redrives: number; open: boolean; onOpenChange: (o: boolean) => void }) {
  const redrive = useOp<{ redriveDate: number }>(on.machine(machineOf(arn)), "RedriveExecution");
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (redrive.isPending) return;
    redrive.mutate({ executionArn: arn }, { onSuccess: () => { onOpenChange(false); toast.success("Execution redriven"); } });
  };
  return (
    <Dialog open={open} onOpenChange={(o) => { onOpenChange(o); if (!o) redrive.reset(); }}>
      <DialogContent className="sm:max-w-[620px]" data-testid="redrive-dialog">
        <form onSubmit={submit} className="grid gap-5">
          <DialogHeader>
            <DialogTitle className="text-[21px] font-semibold">Redrive</DialogTitle>
            <DialogDescription className="text-[14px] leading-[22px] text-foreground" data-testid="redrive-words">
              Redrive continues <b className="font-mono font-semibold text-bright">{name}</b> from the step that {how}{at ? <>, <b className="font-mono font-semibold text-bright">{at}</b></> : null}, with the same input. {succeeded > 0 ? <>The {succeeded} {succeeded === 1 ? "step that succeeded is" : "steps that succeeded are"} not run again and their results stay in the history.</> : null}
            </DialogDescription>
          </DialogHeader>
          {at && <Callout tone="warn" testId="redrive-warning"><b className="font-semibold">{warning.lead}</b> {warning.text}</Callout>}
          <p className="border-l-2 border-input pl-3.5 text-[13px] leading-5 text-muted-foreground">Same definition ({via}) and same execution ARN. The state machine timeout and the retry count of the rerun step start over. Possible for 14 days after the execution ended. The execution summary then shows <b className="font-semibold text-foreground">Redrive count {redrives + 1}</b>.</p>
          <Refused error={redrive.error} operation="RedriveExecution" testId="write-error" />
          <DialogFooter>
            <Button type="button" variant="outline" className="bg-card" onClick={() => onOpenChange(false)}>Cancel</Button>
            <Button type="submit" disabled={redrive.isPending} data-testid="redrive-submit">{redrive.isPending ? "Redriving…" : "Redrive execution"}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

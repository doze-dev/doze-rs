// Deploy API (apigateway__rest-deploy): a deployment of the API and the stage it is pointed at (a new one, an existing one, or none).
// A REST stage serves its deployment's snapshot, so a change to a method shows on a stage only after it is deployed to it.
import { useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Input } from "@doze/ui/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Field, RuleHelp } from "../../../kit";
import { REST, useStages, useWrite } from "./api";
import { count, list, stageNameProblem } from "./words";

const NEW = "__new__";
const NONE = "__none__";

export function Deploy({ api, apiName, open, onOpenChange, onDone }: { api: string; apiName: string; open: boolean; onOpenChange: (o: boolean) => void; onDone: (stage: string | undefined) => void }) {
  const stages = useStages(api).data?.item ?? [];
  const [pick, setPick] = useState<string | null>(null);
  const [fresh, setFresh] = useState("");
  const [description, setDescription] = useState("");
  const [tried, setTried] = useState(false);
  const make = useWrite(REST, "CreateDeployment");
  // The stage the form shows: what was chosen, else the one stage there is, else a new one.
  const stage = pick ?? (stages.length === 1 ? stages[0]!.stageName : stages.length === 0 ? NEW : stages[0]!.stageName);
  const problem = stage === NEW ? stageNameProblem(fresh) : null;
  const failed = make.error ? errorWords(make.error) : null;
  const submit = () => {
    setTried(true);
    if (problem || make.isPending) return;
    const target = stage === NEW ? fresh : stage === NONE ? undefined : stage;
    make.mutate({ restApiId: api, ...(target ? { stageName: target } : {}), ...(description ? { description } : {}) } as never, { onSuccess: () => onDone(target) });
  };
  return (
    <Dialog open={open} onOpenChange={(o) => { onOpenChange(o); if (!o) { make.reset(); setTried(false); } }}>
      <DialogContent className="sm:max-w-[600px]" data-testid="deploy-dialog">
        <DialogHeader>
          <DialogTitle className="text-[21px] font-semibold">Deploy API</DialogTitle>
          <DialogDescription className="text-[14px] leading-[22px] text-foreground">Create a deployment of <b className="font-mono font-semibold text-bright">{apiName}</b> and point a stage at it.</DialogDescription>
        </DialogHeader>
        <form className="grid gap-5" onSubmit={(e) => { e.preventDefault(); submit(); }} noValidate>
          <Field label="Stage" hint={<>Pick <b className="font-semibold text-foreground">New stage</b> and name it, pick an existing stage, or pick <b className="font-semibold text-foreground">No stage</b> to attach the deployment to a stage later. {apiName} has {stages.length === 0 ? "no stage yet" : stages.length === 1 ? <>one stage, <span className="font-mono">{stages[0]!.stageName}</span></> : <>{count(stages.length, "stage")}: <span className="font-mono">{list(stages.map((s) => s.stageName))}</span></>}.</>}>
            {(id) => (
              <Select value={stage} onValueChange={setPick}>
                <SelectTrigger id={id} className="w-full bg-card" data-testid="deploy-stage"><SelectValue /></SelectTrigger>
                <SelectContent>
                  {stages.map((s) => <SelectItem key={s.stageName} value={s.stageName} data-testid={`deploy-stage-${s.stageName}`}>{s.stageName}</SelectItem>)}
                  <SelectItem value={NEW} data-testid="deploy-stage-new">New stage</SelectItem>
                  <SelectItem value={NONE} data-testid="deploy-stage-none">No stage</SelectItem>
                </SelectContent>
              </Select>
            )}
          </Field>
          {stage === NEW && (
            <Field label="New stage name" error={tried ? (problem ?? undefined) : undefined}>{(id) => <Input id={id} value={fresh} onChange={(e) => setFresh(e.target.value)} placeholder="prod" autoComplete="off" spellCheck={false} className="font-mono" data-testid="deploy-new-name" />}</Field>
          )}
          <Field label="Deployment description" optional>{(id) => <Input id={id} value={description} onChange={(e) => setDescription(e.target.value)} autoComplete="off" data-testid="deploy-description" />}</Field>
          <RuleHelp>A stage serves the deployment it points at, a snapshot of the API: a change to a resource or a method shows on the stage only after you deploy. A deployment is recorded either way, so the control plane, CloudFormation and Terraform behave as they do on AWS.</RuleHelp>
          {failed && <Alert tone="bad" title={`CreateDeployment was refused${failed.code ? `: ${failed.code}` : ""}`} testId="deploy-error">{failed.message}</Alert>}
          <DialogFooter>
            <Button type="button" variant="outline" className="bg-card" onClick={() => onOpenChange(false)}>Cancel</Button>
            <Button type="submit" disabled={make.isPending} data-testid="deploy-submit">{make.isPending ? "Deploying…" : "Deploy"}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

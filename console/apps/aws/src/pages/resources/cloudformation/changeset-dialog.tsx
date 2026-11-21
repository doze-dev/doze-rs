// The name of a change set, asked over the review step (cloudformation__stack-create-changeset): what it makes, and the call.
import { useState } from "react";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Input } from "@doze/ui/components/ui/input";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { CallPanel, Field, type PreviewRequest, RuleHelp } from "../../../kit";
import { changeSetNameProblem } from "./wizard-model";

export function ChangeSetDialog({ open, onOpenChange, stack, creating, request, taken, pending, error, onMake }: { open: boolean; onOpenChange: (o: boolean) => void; stack: string; creating: boolean; request: (name: string) => PreviewRequest; taken: readonly string[]; pending: boolean; error: unknown; onMake: (name: string) => void }) {
  const [name, setName] = useState(creating ? "initial-review" : "");
  const [tried, setTried] = useState(false);
  const problem = changeSetNameProblem(name, taken);
  const failed = error ? errorWords(error) : null;
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[92vh] overflow-y-auto sm:max-w-[640px]" data-testid="changeset-name-dialog">
        <DialogHeader>
          <DialogTitle className="text-[21px] font-semibold">Create change set</DialogTitle>
          <DialogDescription className="text-[14px] leading-[22px] text-foreground">Preview what CloudFormation would {creating ? "create" : "change"} for <b className="font-mono text-[13px] text-bright">{stack}</b> before it {creating ? "creates" : "changes"} anything.</DialogDescription>
        </DialogHeader>
        <form id="changeset-form" onSubmit={(e) => { e.preventDefault(); setTried(true); if (!problem) onMake(name); }} className="grid gap-4">
          <Field label="Change set name" error={tried ? (problem ?? undefined) : undefined} help="Letters, digits and hyphens, starting with a letter, 1 to 128 characters, and unique among this stack's change sets." testId="field-changeset-name">
            {(id) => <Input id={id} value={name} onChange={(e) => setName(e.target.value)} className="bg-card font-mono text-[13px]" autoComplete="off" spellCheck={false} autoFocus data-testid="changeset-name" />}
          </Field>
          {failed && <Alert tone="bad" title={`CreateChangeSet was refused${failed.code ? `: ${failed.code}` : ""}`} testId="changeset-error">{failed.message}</Alert>}
        </form>
        <CallPanel request={tried && problem ? null : request(name || "<name>")} testId="changeset-call" />
        <RuleHelp>{creating ? <>This makes the stack now, in REVIEW_IN_PROGRESS, together with a change set that lists what the template would create. Nothing is deployed until you execute it. The name {stack} is taken from this moment on.</> : <>This makes a change set on {stack} that lists what the update would do. Nothing changes until you execute it.</>}</RuleHelp>
        <DialogFooter>
          <Button type="button" variant="outline" className="bg-card" onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button type="submit" form="changeset-form" disabled={pending} data-testid="changeset-make">{pending ? "Working…" : "Create change set"}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

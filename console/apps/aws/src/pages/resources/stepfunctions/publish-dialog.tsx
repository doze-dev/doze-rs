// Publish version (stepfunctions__version-publish): the saved revision becomes the next version, an immutable snapshot.
import { useState } from "react";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Input } from "@doze/ui/components/ui/input";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { Field } from "../../../kit";
import { on, useOp } from "../deep/api";
import { Refused } from "../deep/parts";
import { fieldOf } from "./details-card";
import { type VersionRow, useVersions } from "./hooks";

export function PublishDialog({ d, open, onOpenChange, done, unsaved = false }: { d: ResourceDetail; unsaved?: boolean; open: boolean; onOpenChange: (o: boolean) => void; done: () => void }) {
  const [text, setText] = useState("");
  const versions = useVersions(d.name);
  const publish = useOp<{ stateMachineVersionArn?: string }>(on.machine(d.name), "PublishStateMachineVersion");
  const revision = fieldOf(d, "Revision ID") ?? "";
  const list: VersionRow[] = versions.data ?? [];
  const highest = list.reduce((n, v) => Math.max(n, Number(v.number) || 0), 0);
  const same = list.find((v) => v.revisionId === revision && Number(v.number) === highest);
  return (
    <Dialog open={open} onOpenChange={(o) => { onOpenChange(o); if (!o) publish.reset(); }}>
      <DialogContent className="sm:max-w-[600px]" data-testid="publish-dialog">
        <form className="grid gap-5" onSubmit={(e) => { e.preventDefault(); publish.mutate({ revisionId: revision, ...(text ? { description: text } : {}) }, { onSuccess: done }); }}>
          <DialogHeader>
            <DialogTitle className="text-[21px] font-semibold">Publish version</DialogTitle>
            <DialogDescription className="text-[14px] leading-[22px] text-foreground" data-testid="publish-words">
              {same ? <>The saved revision <b className="font-mono font-semibold">{revision.slice(0, 8)}</b> already has <b className="font-semibold">version {same.number}</b>, so publishing returns it and makes no new one.</> : <>Publishes the saved revision <b className="font-mono font-semibold">{revision.slice(0, 8)}</b> as <b className="font-semibold">version {highest + 1}</b>, an immutable snapshot you can start by its version ARN or through an alias.</>}{unsaved ? <> <span className="text-muted-foreground">Your unsaved changes are not in it: choose Save first.</span></> : null}
            </DialogDescription>
          </DialogHeader>
          <Field label="Description" optional hint="Up to 256 characters. Version numbers go up by one and are never reused; AWS allows up to 1,000 per state machine and doze-aws does not enforce that. Publishing again without changing the definition returns the version that already exists.">
            {(id) => <Input id={id} value={text} onChange={(e) => setText(e.target.value)} maxLength={256} autoComplete="off" className="bg-card" data-testid="publish-description" />}
          </Field>
          <Refused error={publish.error} operation="PublishStateMachineVersion" testId="publish-error" />
          <DialogFooter>
            <Button type="button" variant="outline" className="bg-card" onClick={() => onOpenChange(false)}>Cancel</Button>
            <Button type="submit" disabled={publish.isPending} data-testid="publish-submit">{publish.isPending ? "Publishing…" : "Publish"}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

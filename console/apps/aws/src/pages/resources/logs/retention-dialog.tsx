// The retention setting of a log group (logs__retention-edit): a dialog with the periods CloudWatch Logs accepts and Never Expire, which
// removes the policy (DeleteRetentionPolicy).
import { useState } from "react";
import { Callout } from "@doze/ui/components/app/callout";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { RuleHelp, SectionEdit } from "../../../kit";
import { useGroupWrite } from "./api";
import { type GroupRow, RETENTIONS, retentionWords } from "./model";

const NEVER = "never";

export function RetentionDialog({ group, onClose }: { group: Pick<GroupRow, "name" | "retention"> | null; onClose: () => void }) {
  return group ? <Inner key={group.name} group={group} onClose={onClose} /> : null;
}

function Inner({ group, onClose }: { group: Pick<GroupRow, "name" | "retention">; onClose: () => void }) {
  const [value, setValue] = useState(group.retention === null ? NEVER : String(group.retention));
  const put = useGroupWrite("PutRetentionPolicy");
  const drop = useGroupWrite("DeleteRetentionPolicy");
  const never = value === NEVER;
  const save = () => {
    const done = { onSuccess: onClose };
    if (never) drop.mutate({ group: group.name, input: {} }, done);
    else put.mutate({ group: group.name, input: { retentionInDays: Number(value) } }, done);
  };
  const unchanged = (group.retention === null && never) || String(group.retention) === value;
  return (
    <SectionEdit
      open
      onOpenChange={(o) => { if (!o) onClose(); }}
      title="Retention setting"
      saveLabel="Save"
      request={never ? { service: "logs", operation: "DeleteRetentionPolicy", input: { logGroupName: group.name } } : { service: "logs", operation: "PutRetentionPolicy", input: { logGroupName: group.name, retentionInDays: Number(value) } }}
      onSave={save}
      pending={put.isPending || drop.isPending}
      error={put.error ?? drop.error}
      disabled={unchanged}
      testId="retention-dialog"
    >
      <div className="grid gap-1.5">
        <Label htmlFor="retention-group">Log group</Label>
        <Input id="retention-group" value={group.name} readOnly tabIndex={-1} className="bg-muted font-mono" data-testid="retention-group" />
      </div>
      <div className="grid gap-1.5">
        <Label htmlFor="retention-select">Expire events after</Label>
        <Select value={value} onValueChange={setValue}>
          <SelectTrigger id="retention-select" className="w-full bg-card" data-testid="retention-select"><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value={NEVER}>Never Expire</SelectItem>
            {RETENTIONS.map((d) => <SelectItem key={d} value={String(d)}>{retentionWords(d)}</SelectItem>)}
          </SelectContent>
        </Select>
        <RuleHelp>Choose <b>Never Expire</b>, or one of 1, 3, 5, 7, 14, 30, 60, 90, 120, 150, 180, 365, 400, 545, 731, 1096, 1827, 2192, 2557, 2922, 3288 or 3653 days. Never Expire removes the policy (DeleteRetentionPolicy).</RuleHelp>
      </div>
      <Callout tone="note" testId="retention-note">
        On AWS, events aren't deleted the moment they expire: it typically takes up to <b className="font-semibold text-bright">72 hours</b>. doze-aws drops expired events when the group is written or read, at most once a minute. <b className="font-semibold text-bright">Never Expire</b> means no policy: on AWS the events are kept indefinitely, and doze-aws drops those older than its <span className="font-mono">--logs-retention-days</span> (7 by default).
      </Callout>
    </SectionEdit>
  );
}

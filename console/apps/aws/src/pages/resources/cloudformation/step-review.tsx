// Step 4 (cloudformation__stack-create-review, stack-update-review): what was chosen, with an Edit link to each step, and for an update the
// change set the console made to preview it (CreateChangeSet and DescribeChangeSet), as a table of what it would add, modify or remove.
import { Callout } from "@doze/ui/components/app/callout";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { ReviewSection } from "@doze/ui/components/app/stepper";
import { Badge } from "@doze/ui/components/ui/badge";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { RuleHelp } from "../../../kit";
import { filled } from "@doze/ui/components/app/key-values";
import { errorWords } from "@doze/ui/components/app/alert";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { type ChangeSet, changeRows } from "./changeset-parts";
import { nameList } from "./model";
import { type Summary, type Wizard, valueOf } from "./wizard-model";

const mono = "font-mono text-[13px]";

export function StepReview({ w, summary, edit, preview, previewError, stackName }: { w: Wizard; summary: Summary; edit: (step: number) => void; preview?: { data?: ChangeSet; pending: boolean }; previewError?: unknown; stackName: string }) {
  const update = w.mode !== "create";
  const tags = filled(w.tags);
  const params = summary.Parameters ?? [];
  const rows = preview?.data ? changeRows(preview.data) : [];
  const columns: DataColumn<(typeof rows)[number]>[] = [
    { id: "a", header: "Action", size: 110, accessorKey: "Action", cell: (c) => <StatusDot tone={c.row.original.Action === "Add" ? "ok" : c.row.original.Action === "Remove" ? "bad" : "info"}>{c.row.original.Action}</StatusDot> },
    { id: "l", header: "Logical ID", size: 200, accessorKey: "LogicalResourceId", cell: (c) => <span className={`${mono} font-medium text-bright`}>{c.row.original.LogicalResourceId}</span> },
    { id: "t", header: "Resource type", size: 200, accessorKey: "ResourceType", cell: (c) => <span className={mono}>{c.row.original.ResourceType}</span> },
    { id: "r", header: "Replacement", accessorFn: (r) => r.Replacement ?? "", cell: (c) => c.row.original.Replacement ?? <span className="text-muted-foreground">–</span> },
  ];
  const refused = previewError ? errorWords(previewError) : null;
  return (
    <>
      <ReviewSection title={update ? "Update stack" : "Create stack"} onEdit={() => edit(0)} rows={[
        { label: "Prepare template", value: update ? (w.keepTemplate ? "Use existing template" : "Replace existing template") : "Choose an existing template" },
        ...(update && w.keepTemplate ? [] : [{ label: w.source === "url" ? "Amazon S3 URL" : "Template file", value: <span className={`${mono} [overflow-wrap:anywhere]`}>{w.source === "url" ? w.url : w.file?.name}</span> }]),
        ...(summary.Description ? [{ label: "Description", value: summary.Description }] : []),
      ]} />
      <ReviewSection title="Specify stack details" onEdit={() => edit(1)} rows={[
        { label: "Stack name", value: <span className={mono}>{update ? stackName : w.name}</span> },
        ...params.map((d) => ({ label: d.ParameterKey, value: update && w.keep[d.ParameterKey] ? "Use existing value" : d.NoEcho ? "****" : <span className={`${mono} [overflow-wrap:anywhere]`}>{valueOf(w, d)}</span> })),
      ]} />
      <ReviewSection title="Configure stack options" onEdit={() => edit(2)} rows={[
        { label: "Tags", value: tags.length === 0 ? "None" : <span className="flex flex-wrap gap-2">{tags.map((t) => <Badge key={t.key} variant="outline" className="font-mono font-normal">{t.key}: {t.value}</Badge>)}</span> },
        { label: update ? "Behavior on provisioning failure" : "Stack failure options", value: w.failure === "rollback" ? "Roll back all stack resources" : "Preserve successfully provisioned resources" },
        ...(w.role ? [{ label: "IAM role", value: <span className={`${mono} [overflow-wrap:anywhere]`}>{w.role}</span> }] : []),
        { label: "Notification options", value: w.topic.trim() ? <span className={`${mono} [overflow-wrap:anywhere]`}>{w.topic.trim()}</span> : "None" },
        ...(w.policy.trim() ? [{ label: "Stack policy", value: "Set" }] : []),
        ...(update ? [{ label: "Capabilities", value: (summary.Capabilities ?? []).length > 0 ? nameList(summary.Capabilities ?? []) : "None needed" }] : [{ label: "Termination protection", value: w.protect ? "Enabled" : "Disabled" }, ...(w.timeout ? [{ label: "Timeout", value: `${w.timeout} minutes` }] : [])]),
      ]} />
      {update && (
        <section className="min-w-0 rounded-[14px] border bg-card p-6 lg:p-7" aria-label="Change set preview" data-testid="preview">
          <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">Change set preview</h2>
          <p className="mt-1 text-[13.5px] text-muted-foreground">CloudFormation compared this update with the running stack by making a change set. Nothing has been changed yet.</p>
          <div className="mt-4 grid gap-3">
            {preview?.pending ? <Skeleton className="h-24 rounded-[12px]" /> : refused ? (
              <Callout tone="bad" lead="The preview could not be made." testId="preview-refused">{refused.code ? `${refused.code}: ` : ""}{refused.message}</Callout>
            ) : preview?.data?.Status === "FAILED" ? (
              <Callout tone="warn" lead="This update changes nothing." testId="preview-empty">{preview.data.StatusReason} UpdateStack would be refused with the same words.</Callout>
            ) : (
              <>
                <DataTable aria-label="Change set preview" data={rows} columns={columns} rowId={(r) => r.LogicalResourceId} rowHeight={48} maxHeight={320} empty="No resource would change." />
                <RuleHelp>Modify means the resource's properties change in place or it is replaced, as its Replacement says. Choose View change set to read the same preview in full and execute it from there, or Update stack to deploy now. Either way the update runs in the background.</RuleHelp>
              </>
            )}
          </div>
        </section>
      )}
    </>
  );
}

// A state machine's tags: the Tags tab (stepfunctions__machine-tags, machine-tags-none) and the Manage tags page
// (machine-tags-manage), which sends the added and edited tags with TagResource and the removed ones with UntagResource.
import { TagsIcon } from "lucide-react";
import { useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { Alert, ErrorBlock, errorWords } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { newRow, rowsFrom, rowsValid, type KeyValueRow } from "@doze/ui/components/app/key-values";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { FormPage, FormSection, RuleHelp, type Tag, TAG_RULES, diffTags, noChange, useServiceCrumbs, useSetTags, useTags } from "../../../kit";
import type { OwnProps } from "../deep/registry";
import { machineHref, useWhere } from "./hooks";

const COLUMNS: DataColumn<Tag>[] = [
  { id: "key", header: "Key", accessorKey: "key", size: 320, min: 140, meta: { mono: true }, cell: (c) => <b className="font-mono text-[13px] font-medium text-bright">{c.row.original.key}</b> },
  { id: "value", header: "Value", accessorKey: "value", meta: { mono: true }, cell: (c) => c.row.original.value || <span className="text-muted-foreground">(empty)</span> },
];

/** AWS keeps the aws: prefix of a tag key for itself; Step Functions (and doze-aws) refuse it. */
export const RULES = { ...TAG_RULES, reservedPrefix: "aws:" };
const RULE = "Up to 50 tags per resource. Keys are 1 to 128 characters, values 0 to 256; letters, digits, spaces and _ . : / = + - @ . The aws: prefix is reserved.";

export function TagsPanel({ d }: { d: ResourceDetail }) {
  const navigate = useNavigate();
  const tags = useTags({ service: d.service, kind: d.kind, name: d.name });
  const manage = () => void navigate({ to: "/resources/$service/$kind/$name", params: { service: d.service, kind: d.kind, name: d.name }, search: { do: "tags" } });
  if (tags.isError && !tags.data) return <ErrorBlock what="The tags could not be read" error={tags.error} onRetry={() => void tags.refetch()} testId="tags-error" />;
  if (!tags.data) return <Skeleton className="h-40 w-full rounded-[var(--radius-card)]" data-testid="tags-loading" />;
  const list = tags.data.tags;
  if (list.length === 0) {
    return (
      <EmptyTab icon={TagsIcon} title="No tags" action={<Button type="button" variant="outline" className="bg-card" onClick={manage} data-testid="tags-manage-empty">Manage tags</Button>} testId="tags-none">
        {d.name} has no tags.
      </EmptyTab>
    );
  }
  return (
    <div data-testid="tags-view">
      <div className="mb-4 flex justify-end"><Button type="button" variant="outline" className="bg-card" onClick={manage} data-testid="tags-manage">Manage tags</Button></div>
      <DataTable aria-label="Tags" data={list} columns={COLUMNS} rowId={(t) => t.key} maxHeight={420} />
      <p className="mt-5 max-w-[46rem] border-l-2 border-input pl-3.5 text-[13px] leading-5 text-muted-foreground" data-testid="tags-note">{RULE} Executions inherit the tags of their state machine for access control.</p>
    </div>
  );
}

export function ManageTags({ name }: { name: string }) {
  const navigate = useNavigate();
  const base = useServiceCrumbs("stepfunctions");
  const target = { service: "stepfunctions", kind: "machines", name };
  const where = useWhere();
  const arn = `arn:aws:states:${where.region}:${where.account}:stateMachine:${name}`;
  const tags = useTags(target);
  const set = useSetTags(target);
  const [rows, setRows] = useState<KeyValueRow[] | null>(null);
  const [tried, setTried] = useState(false);
  const saved = tags.data?.tags ?? [];
  const live = rows ?? (tags.data ? (saved.length > 0 ? rowsFrom(saved) : [newRow()]) : null);
  const back = (done?: string) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "stepfunctions", kind: "machines", name }, search: done ? { tab: "tags", done } : { tab: "tags" } });
  if (!live) return <Skeleton className="h-80 rounded-[var(--radius-card)]" />;
  const change = diffTags(saved, live);
  const valid = rowsValid(live, RULES);
  const requests = [
    ...(change.set.length > 0 ? [{ service: "stepfunctions", operation: "TagResource", input: { resourceArn: arn, tags: change.set } }] : []),
    ...(change.remove.length > 0 ? [{ service: "stepfunctions", operation: "UntagResource", input: { resourceArn: arn, tagKeys: change.remove } }] : []),
  ];
  const failed = set.error ? errorWords(set.error) : null;
  return (
    <FormPage
      title="Manage tags"
      sub={<>Edit the Key and Value of a tag, remove a tag, or add a new one. Then choose Save. Tags apply to <span className="font-mono font-semibold text-bright">{name}</span>.</>}
      crumbs={[...base, { label: "State machines", to: "/resources/stepfunctions/machines" }, { label: name, to: machineHref(name) }, { label: "Tags", to: machineHref(name, { tab: "tags" }) }]}
      request={requests[0] ?? { service: "stepfunctions", operation: "TagResource", input: { resourceArn: arn, tags: [] } }}
      requests={requests.length > 1 ? requests : undefined}
      callNote={<>A removed tag is sent separately with <span className="font-mono">UntagResource</span> and <span className="font-mono">tagKeys</span>.</>}
      banner={failed ? <Alert tone="bad" title={`The tags were not saved${failed.code ? `: ${failed.code}` : ""}`} testId="tags-refused">{failed.message}</Alert> : undefined}
      submitLabel="Save"
      pending={set.isPending}
      disabled={(tried && !valid) || noChange(change)}
      disabledWhy={noChange(change) ? "Nothing changed." : "Fix the tags first."}
      onCancel={() => back()}
      onSubmit={() => { setTried(true); if (valid && !noChange(change)) set.mutate(change, { onSuccess: () => back("tags") }); }}
      testId="manage-tags-page"
    >
      <FormSection title="Tags">
        <KeyValueEditor rows={live} onChange={setRows} rules={RULES} showErrors={tried} addLabel="Add tag" removeLabel="Remove tag" testId="tags-kv" />
        <RuleHelp>{RULE} <b className="font-semibold">Save</b> sends the added and edited tags with <span className="font-mono">TagResource</span> and the removed ones with <span className="font-mono">UntagResource</span>.</RuleHelp>
      </FormSection>
    </FormPage>
  );
}

export function TagsPage({ name }: OwnProps) {
  return <ManageTags name={name ?? ""} />;
}

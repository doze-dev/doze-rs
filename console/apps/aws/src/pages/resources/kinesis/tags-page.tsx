// Manage tags (kinesis__stream-tags-edit): the stream's tags as rows to change, add and remove; saving makes AddTagsToStream for what is new or changed and
// RemoveTagsFromStream for the keys that are gone.
import { useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { newRow, rowsFrom, rowsValid, type KeyValueRow } from "@doze/ui/components/app/key-values";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { FormPage, FormSection, RuleHelp, TAG_RULES, diffTags, noChange, useSetTags, useTags } from "../../../kit";
import { count } from "./model";

export function ManageTags({ name }: { name: string }) {
  const navigate = useNavigate();
  const target = { service: "kinesis", kind: "streams", name };
  const tags = useTags(target);
  const set = useSetTags(target);
  const [rows, setRows] = useState<KeyValueRow[] | null>(null);
  const [tried, setTried] = useState(false);
  const saved = tags.data?.tags ?? [];
  const live = rows ?? (tags.data ? (saved.length > 0 ? rowsFrom(saved) : [newRow()]) : null);
  const back = (done?: string) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "kinesis", kind: "streams", name }, search: done ? { done } : {} });
  if (!live) return <Skeleton className="h-80 rounded-[14px]" />;
  const change = diffTags(saved, live);
  const valid = rowsValid(live, TAG_RULES);
  const added = change.set.filter((t) => !saved.some((x) => x.key === t.key));
  const requests = [
    ...(change.set.length > 0 ? [{ service: "kinesis", operation: "AddTagsToStream", input: { StreamName: name, Tags: Object.fromEntries(change.set.map((t) => [t.key, t.value])) } }] : []),
    ...(change.remove.length > 0 ? [{ service: "kinesis", operation: "RemoveTagsFromStream", input: { StreamName: name, TagKeys: change.remove } }] : []),
  ];
  const failed = set.error ? errorWords(set.error) : null;
  return (
    <FormPage
      title="Manage tags"
      mono={false}
      sub={<span className="font-mono">{name}</span>}
      crumbs={[{ label: "Resources", to: "/resources" }, { label: "Messaging and events", to: "/resources" }, { label: "Kinesis", to: "/resources/kinesis" }, { label: name, to: `/resources/kinesis/streams/${encodeURIComponent(name)}` }]}
      request={requests[0] ?? null}
      requests={requests.length > 1 ? requests : undefined}
      callNote="Removing a row calls RemoveTagsFromStream with that key."
      banner={failed ? <Alert tone="bad" title={`The tags were not saved${failed.code ? `: ${failed.code}` : ""}`} testId="tags-refused">{failed.message}</Alert> : undefined}
      submitLabel="Save changes"
      pending={set.isPending}
      disabled={(tried && !valid) || noChange(change)}
      disabledWhy={noChange(change) ? "Nothing changed." : "Fix the tags first."}
      onCancel={() => back()}
      onSubmit={() => { setTried(true); if (valid && !noChange(change)) set.mutate(change, { onSuccess: () => back("tags") }); }}
      testId="manage-tags-page"
    >
      <FormSection title="Tags" description={<>{count(saved.length, "tag")} {saved.length === 1 ? "is" : "are"} saved on this stream.{added.length > 0 ? ` The ${added.map((t) => t.key).join(", ")} ${added.length === 1 ? "row is" : "rows are"} new.` : ""} Changes are saved when you choose Save changes.</>}>
        <KeyValueEditor rows={live} onChange={setRows} rules={TAG_RULES} showErrors={tried} testId="tags-kv" />
        <RuleHelp>Keys and values are case-sensitive. Each key is unique: adding a key that exists overwrites its value.</RuleHelp>
      </FormSection>
    </FormPage>
  );
}

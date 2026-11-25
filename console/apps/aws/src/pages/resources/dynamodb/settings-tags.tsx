// A table's tags, in its Additional settings (dynamodb__table-settings, table-tags-none): the tags with a Delete each, and Add new tag.
// Each change is one TagResource or UntagResource through the console's tags endpoint (contract §3).
import { PlusIcon, TagsIcon } from "lucide-react";
import { useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { type Tag, TAG_RULES, useSetTags, useTags } from "../../../kit";
import { Card } from "../deep/parts";
import { KIND } from "./parts";

const LEDE = "Key 1 to 128 characters; value up to 256.";

/** What a saved tag change says: `Tag tier added to orders-table.` */
export function tagSaid(what: "added" | "set" | "deleted", key: string, table: string): string {
  return what === "added" ? `Tag ${key} added to ${table}.` : what === "set" ? `Tag ${key} of ${table} has its new value.` : `Tag ${key} deleted from ${table}.`;
}

/** `onSaved`: what to say once a tag is added or deleted (the settings page's banner: the one it held before is no longer true). */
export function SettingsTags({ name, onSaved }: { name: string; onSaved: (said: string) => void }) {
  const target = { ...KIND, name };
  const q = useTags(target);
  const set = useSetTags(target);
  const [adding, setAdding] = useState(false);
  const [key, setKey] = useState("");
  const [value, setValue] = useState("");
  const tags = q.data?.tags ?? [];
  const failed = set.error ? errorWords(set.error) : null;
  const clash = tags.some((t) => t.key === key && t.value !== value);
  const bad = key.length < 1 ? "Give the tag a key." : key.length > TAG_RULES.maxKey! ? `A key is at most ${TAG_RULES.maxKey} characters.` : value.length > TAG_RULES.maxValue! ? `A value is at most ${TAG_RULES.maxValue} characters.` : null;
  const add = () => {
    if (bad) return;
    set.mutate({ set: [{ key, value }], remove: [] }, { onSuccess: () => (onSaved(tagSaid(clash || tags.some((t) => t.key === key) ? "set" : "added", key, name)), setAdding(false), setKey(""), setValue("")) });
  };
  const columns: DataColumn<Tag>[] = [
    { id: "key", header: "Key", accessorKey: "key", size: 320, min: 140, meta: { mono: true }, cell: (c) => <b className="font-medium text-bright">{c.row.original.key}</b> },
    { id: "value", header: "Value", accessorKey: "value", meta: { mono: true }, cell: (c) => c.row.original.value || <span className="text-muted-foreground">(empty)</span> },
    { id: "delete", header: "", size: 110, enableSorting: false, cell: (c) => (
        <Button variant="outline" size="sm" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={set.isPending} onClick={() => set.mutate({ set: [], remove: [c.row.original.key] }, { onSuccess: () => onSaved(tagSaid("deleted", c.row.original.key, name)) })} data-testid={`tag-delete-${c.row.original.key}`}>
          Delete
        </Button>
      ) },
  ];
  const addButton = (
    <Button variant="outline" className="bg-card" onClick={() => setAdding(true)} disabled={adding || !q.data?.editable} data-testid="tag-add">
      <PlusIcon /> Add new tag
    </Button>
  );
  return (
    <Card title="Tags" lede={LEDE} testId="table-tags">
      <div className="grid gap-4">
        {q.isPending ? (
          <Skeleton className="h-24 rounded-xl" />
        ) : (
          <>
            {tags.length > 0 && <div>{addButton}</div>}
            {adding && (
              <form className="grid max-w-[48rem] items-start gap-3 sm:grid-cols-[1fr_1fr_auto]" onSubmit={(e) => (e.preventDefault(), add())} data-testid="tag-form">
                <Input value={key} onChange={(e) => setKey(e.target.value)} placeholder="Key" aria-label="Tag key" autoComplete="off" className="bg-card font-mono" data-testid="tag-key" />
                <Input value={value} onChange={(e) => setValue(e.target.value)} placeholder="Value" aria-label="Tag value" autoComplete="off" className="bg-card font-mono" data-testid="tag-value" />
                <div className="flex gap-2">
                  <Button type="submit" disabled={Boolean(bad) || set.isPending} data-testid="tag-save">{set.isPending ? "Adding…" : "Add"}</Button>
                  <Button type="button" variant="outline" className="bg-card" onClick={() => setAdding(false)}>Cancel</Button>
                </div>
                {clash && <p className="text-[13px] text-muted-foreground sm:col-span-3">A tag with this key exists; adding it replaces its value.</p>}
              </form>
            )}
            {failed && (
              <Alert tone="bad" title={`The tags were not saved${failed.code ? `: ${failed.code}` : ""}`} testId="tags-refused">
                {failed.message}
              </Alert>
            )}
            {tags.length === 0 && !adding ? (
              <EmptyTab icon={TagsIcon} title="No tags" action={addButton} testId="tags-none">
                {name} has no tags. Tags are key-value labels for organising tables and tracking their cost.
              </EmptyTab>
            ) : (
              tags.length > 0 && <DataTable aria-label="Tags" data={tags} columns={columns} rowId={(t) => t.key} maxHeight={420} />
            )}
          </>
        )}
      </div>
    </Card>
  );
}

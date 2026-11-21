import { PencilIcon, PlusIcon, TagsIcon } from "lucide-react";
import { useState } from "react";
import { Alert, ErrorBlock, errorWords } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { newRow, rowsFrom, rowsValid, type KeyValueRow } from "@doze/ui/components/app/key-values";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import type { Tag, TagsAnswer, TagsChange } from "./contract";
import { type Target, useSetTags, useTags } from "./ops";
import { changeWords, diffTags, noChange, TAG_RULES, tagWords } from "./tags";

const COLUMNS: DataColumn<Tag>[] = [
  { id: "key", header: "Key", accessorKey: "key", size: 320, min: 140, meta: { mono: true }, cell: (c) => <b className="font-medium text-bright">{c.row.original.key}</b> },
  { id: "value", header: "Value", accessorKey: "value", meta: { mono: true }, cell: (c) => c.row.original.value || <span className="text-muted-foreground">(empty)</span> },
];

/** A resource's tags as a tab: what there are (or that there are none, and what to do), and the edit in place. Presentational:
 * the data and the save are given, so every state can be drawn without a server. {@link TagsTab} is it with the data. */
export function TagsView({ answer, saving = false, error, saved, onSave, noun = "tag", countNoun, note, subject, explain, emptyAction, loading = false, loadError, onRetry }: { answer: TagsAnswer | undefined; saving?: boolean; /** The service's refusal of the save. */ error?: unknown; /** What the last save did, said: shown until the next edit. */ saved?: string | null; onSave: (change: TagsChange) => Promise<unknown> | void; /** `cost allocation tag`. */ noun?: string; /** What the count of tags calls them, when it is not `noun` (`3 cost allocation tags` over an empty state that says `No tags`). */ countNoun?: string; /** Under the table, when there are tags. */ note?: React.ReactNode; /** The resource's name, for the sentence when there are none (`email-outbox has no tags.`). */ subject?: string; /** What a tag is for, said when there are none. */ explain?: React.ReactNode; /** The way on from nothing, when it is not the inline Add (the board sends a queue to its Edit page). */ emptyAction?: React.ReactNode; loading?: boolean; loadError?: unknown; onRetry?: () => void }) {
  const [rows, setRows] = useState<KeyValueRow[] | null>(null);
  const [tried, setTried] = useState(false);
  if (loadError && !answer) return <ErrorBlock what="The tags could not be read" error={loadError} onRetry={onRetry} className="w-full" testId="tags-error" />;
  if (loading || !answer) return <Skeleton className="h-40 w-full rounded-[14px]" data-testid="tags-loading" />;
  const tags = answer.tags;
  const edit = () => {
    setTried(false);
    setRows(tags.length > 0 ? rowsFrom(tags) : [newRow()]);
  };
  if (rows) {
    const change = diffTags(tags, rows);
    const valid = rowsValid(rows, TAG_RULES);
    const failed = error ? errorWords(error) : null;
    return (
      <form
        className="grid max-w-[56rem] gap-5"
        onSubmit={(e) => {
          e.preventDefault();
          setTried(true);
          if (valid && !noChange(change)) void Promise.resolve(onSave(change)).then(() => setRows(null), () => undefined);
        }}
        data-testid="tags-edit"
      >
        <p className="text-[13.5px] text-muted-foreground">Tags are case-sensitive, and a new tag with an existing key overwrites it. Up to {TAG_RULES.maxRows} tags.</p>
        <KeyValueEditor rows={rows} onChange={setRows} rules={TAG_RULES} showErrors={tried} disabled={saving} testId="tags-kv" />
        {failed && (
          <Alert tone="bad" title={`The tags were not saved${failed.code ? `: ${failed.code}` : ""}`} testId="tags-refused">
            {failed.message}
          </Alert>
        )}
        <div className="flex flex-wrap items-center gap-3">
          <Button type="submit" disabled={saving || (tried && !valid)} data-testid="tags-save">
            {saving ? "Saving…" : "Save changes"}
          </Button>
          <Button type="button" variant="outline" className="bg-card" onClick={() => setRows(null)} disabled={saving} data-testid="tags-cancel">
            Cancel
          </Button>
          <span className="text-[13px] text-muted-foreground" data-testid="tags-change">
            {changeWords(change, tags)}
          </span>
        </div>
      </form>
    );
  }
  return (
    <div className="grid gap-4" data-testid="tags-view">
      {saved && <ResultBanner title="Tags saved." returned={saved} testId="tags-saved" />}
      {tags.length === 0 ? (
        <EmptyTab icon={TagsIcon} title={`No ${noun}s`} action={emptyAction ?? (answer.editable ? <Button type="button" variant="outline" className="bg-card" onClick={edit} data-testid="tags-add"><PlusIcon /> Add {noun}</Button> : undefined)}>
          {subject ? `${subject} has no ${noun}s.` : `Nothing is tagged.`} {explain ?? "A tag is a key and a value attached to a resource to organise it or track its cost."}{answer.editable ? "" : " This kind cannot be tagged from the console."}
        </EmptyTab>
      ) : (
        <>
          <div className="flex flex-wrap items-center justify-between gap-3">
            <p className="text-[13.5px] text-muted-foreground" data-testid="tags-count">
              {tagWords(tags.length, countNoun ?? noun)}
              {!answer.editable && " The console reads these but cannot change them."}
            </p>
            {answer.editable && (
              <Button type="button" variant="outline" className="bg-card" onClick={edit} data-testid="tags-edit-open">
                <PencilIcon /> Edit tags
              </Button>
            )}
          </div>
          <DataTable aria-label="Tags" data={tags} columns={COLUMNS} rowId={(t) => t.key} maxHeight={420} />
          {note && <p className="text-[13px] leading-5 text-muted-foreground" data-testid="tags-note">{note}</p>}
        </>
      )}
    </div>
  );
}

/** {@link TagsView} with a resource's tags read and saved through the console (contract §3). */
export function TagsTab({ target, noun, countNoun, note, explain, emptyAction }: { target: Target; noun?: string; countNoun?: string; note?: React.ReactNode; explain?: React.ReactNode; emptyAction?: React.ReactNode }) {
  const q = useTags(target);
  const set = useSetTags(target);
  const [saved, setSaved] = useState<string | null>(null);
  return <TagsView answer={q.data} loading={q.isPending && !q.isError} loadError={q.error} onRetry={() => void q.refetch()} saving={set.isPending} error={set.error} saved={saved} noun={noun} countNoun={countNoun} note={note} subject={target.name} explain={explain} emptyAction={emptyAction} onSave={async (c) => {
      const a = await set.mutateAsync(c);
      setSaved(`The resource now has ${tagWords(a.tags.length, noun ?? "tag")}.`);
    }} />;
}

// Copy, Move and Rename an object: the source, where the copy goes (any bucket of this account), and which settings it keeps.
// One CopyObject (the kind's own operation, any bucket); Move and Rename follow it with the delete of the source.
import { useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { DataTable } from "@doze/ui/components/app/data-table";
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { filled, pairsOf, type KeyValueRow } from "@doze/ui/components/app/key-values";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Input } from "@doze/ui/components/ui/input";
import { Field, FormPage, FormSection, ResourcePicker, TAG_RULES, callKindOp } from "../../../kit";
import { useOpQuery } from "../deep/api";
import { callOp } from "../deep/api";
import type { OwnProps } from "../deep/registry";
import { bucketOf, crumbs, NONE, useBucketNav, useSetting } from "./api";
import { encryptionOf, SSE_WORDS } from "./config";
import { RadioCards } from "./controls";
import { setFlash } from "./flash";
import { Mono, Rows } from "./parts";
import { baseName, copySource, s3Uri, shortDate, size } from "./words";

type Head = { ContentLength?: number; LastModified?: string | number };
const MODES = { copy: "Copy", move: "Move", rename: "Rename" } as const;

/** `s3://bucket/a/b/` as its bucket and prefix. */
export function parseUri(text: string): { bucket: string; prefix: string } | null {
  const m = /^s3:\/\/([^/]+)(?:\/(.*))?$/.exec(text.trim());
  return m ? { bucket: m[1]!, prefix: m[2] ?? "" } : null;
}

export function ObjectCopy({ name, search }: OwnProps) {
  const key = search.key ?? "";
  const mode = (search.mode === "move" || search.mode === "rename" ? search.mode : "copy") as keyof typeof MODES;
  const go = useBucketNav(name!);
  const folder = key.includes("/") ? key.slice(0, key.lastIndexOf("/") + 1) : "";
  const head = useOpQuery<Head>(bucketOf(name!), "HeadObject", { Key: key }, ["head", key]);
  const [dest, setDest] = useState(s3Uri(name!, folder));
  const [newName, setNewName] = useState(baseName(key));
  const [settings, setSettings] = useState<"copy" | "none" | "specify">("copy");
  const [tags, setTags] = useState<KeyValueRow[]>([]);
  const [browse, setBrowse] = useState(false);
  const [pick, setPick] = useState<string | null>(null);
  const [pickPrefix, setPickPrefix] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const parsed = mode === "rename" ? { bucket: name!, prefix: folder } : parseUri(dest);
  const destKey = parsed ? (mode === "rename" ? `${folder}${newName}` : `${parsed.prefix}${baseName(key)}`) : "";
  const dv = useSetting<{ Status?: string }>(parsed?.bucket ?? name!, "GetBucketVersioning", []);
  const de = useSetting(parsed?.bucket ?? name!, "GetBucketEncryption", NONE.encryption);
  const dl = useSetting<{ ObjectLockConfiguration?: { ObjectLockEnabled?: string } }>(parsed?.bucket ?? name!, "GetObjectLockConfiguration", NONE.lock);
  const kept = filled(tags);
  const input = parsed && destKey ? { Bucket: parsed.bucket, Key: destKey, CopySource: copySource(name!, key), ...(settings === "none" ? { MetadataDirective: "REPLACE", TaggingDirective: "REPLACE" } : settings === "specify" ? { MetadataDirective: "COPY", TaggingDirective: "REPLACE", ...(kept.length > 0 ? { Tagging: pairsOf(kept).map((t) => `${encodeURIComponent(t.key)}=${encodeURIComponent(t.value)}`).join("&") } : {}) } : {}) } : null;
  const same = parsed?.bucket === name && destKey === key;
  const said = error ? errorWords(error) : null;
  const back = () => go(folder ? { tab: "objects", p: folder } : {});
  const run = async () => {
    if (!input) return;
    setBusy(true);
    setError(null);
    try {
      await callKindOp({ service: "s3", kind: "buckets" }, "CopyObject", input);
      if (mode !== "copy") await callOp(bucketOf(name!), "DeleteObject", { Key: key });
      setFlash({ title: `${mode === "copy" ? "Copied" : mode === "move" ? "Moved" : "Renamed"} ${baseName(key)} to ${s3Uri(input.Bucket, input.Key)}.`, returned: mode === "copy" ? "CopyObject answered with the new object's ETag." : "CopyObject, then DeleteObject of the source." });
      back();
    } catch (e) {
      setError(e);
      setBusy(false);
    }
  };
  const notes = [dv.data?.Status ? dv.data.Status : "Disabled", de.data ? (encryptionOf(de.data) ? SSE_WORDS[encryptionOf(de.data)!.algorithm] : "None") : "Server-side encryption with Amazon S3 managed keys (SSE-S3)", dl.data?.ObjectLockConfiguration?.ObjectLockEnabled === "Enabled" ? "Enabled" : "Disabled"];
  return (
    <FormPage title={MODES[mode]} sub={mode === "rename" ? <>Give <Mono>{baseName(key)}</Mono> a new name. It stays in {s3Uri(name!, folder)}.</> : <>{MODES[mode]} <Mono>{baseName(key)}</Mono> to a new location. Choose where the copy goes and which settings it keeps.</>}
      crumbs={[...crumbs(name!), { label: baseName(key), to: `/resources/s3/buckets/${encodeURIComponent(name!)}` }, { label: MODES[mode], to: `/resources/s3/buckets/${encodeURIComponent(name!)}` }]}
      request={input ? { service: "s3", operation: "CopyObject", input } : null} showInput
      callNote={<>The copy resets the creation date and any system-controlled metadata. {mode === "copy" ? "Moving is the same call followed by a delete of the source." : "This is the same call followed by a delete of the source."}</>}
      refusal={said ? { code: said.code?.split(" · ")[0] ?? "Error", message: said.message } : null}
      banner={said ? <Alert tone="bad" title="Not done." testId="copy-refused">{said.message}</Alert> : undefined}
      submitLabel={MODES[mode]} pending={busy} disabled={!input || same} disabledWhy={same ? "The destination is the object itself." : "Give a destination like s3://bucket/folder/."} onCancel={back} onSubmit={() => void run()}>
      <FormSection title="Source">
        <DataTable aria-label="Source" data={[key]} rowId={(k) => k} columns={[{ id: "n", header: "Name", accessorFn: (k) => k, meta: { mono: true }, min: 200 }, { id: "m", header: "Last modified", size: 260, accessorFn: () => "", cell: () => shortDate(head.data?.LastModified) }, { id: "s", header: "Size", size: 110, meta: { numeric: true }, accessorFn: () => 0, cell: () => size(head.data?.ContentLength) }]} />
      </FormSection>
      <FormSection title="Destination">
        <p className="text-[13px] text-muted-foreground">General purpose bucket. Directory buckets are an express-zone feature that doze-aws does not have, so this is the only destination type.</p>
        {mode === "rename" ? (
          <Field label="New name">{(id) => <Input id={id} value={newName} onChange={(e) => setNewName(e.target.value)} className="font-mono" data-testid="rename-name" />}</Field>
        ) : (
          <Field label="Destination" error={parsed ? undefined : "Write it as s3://bucket/folder/."} hint="A copy is written as a new object, so a bucket that notifies on creation notifies again. Objects of less than 5 GB are copied.">
            {(id) => <div className="flex gap-2"><Input id={id} value={dest} onChange={(e) => setDest(e.target.value)} className="font-mono" spellCheck={false} data-testid="copy-dest" /><Button type="button" variant="outline" className="bg-card" onClick={() => setBrowse(true)} data-testid="browse-s3">Browse S3</Button></div>}
          </Field>
        )}
        {parsed && destKey && <p className="text-[13px] text-muted-foreground">The new key will be <Mono>{s3Uri(parsed.bucket, destKey)}</Mono></p>}
      </FormSection>
      <FormSection title="Destination details"><Rows rows={[["Bucket Versioning", notes[0]!], ["Default encryption", notes[1]!], ["Object Lock", notes[2]!]]} /></FormSection>
      <FormSection title="Additional copy settings">
        <RadioCards name="copy-settings" value={settings} onChange={setSettings} options={[{ value: "copy", title: "Copy source settings", description: "The default." }, { value: "none", title: "Don’t specify settings", description: "Copy the object without the source settings attributes." }, { value: "specify", title: "Specify settings", description: "Set the object tags for the copy." }]} />
        {settings === "specify" && <Field label="Tags"><KeyValueEditor rows={tags} onChange={setTags} rules={TAG_RULES} addLabel="Add tag" testId="copy-tags" /></Field>}
      </FormSection>
      <Dialog open={browse} onOpenChange={setBrowse}>
        <DialogContent className="sm:max-w-[520px]" data-testid="browse-dialog">
          <DialogHeader><DialogTitle className="text-[21px] font-semibold">Browse S3</DialogTitle><DialogDescription>Choose a bucket, and a folder inside it if you like.</DialogDescription></DialogHeader>
          <ResourcePicker service="s3" kind="buckets" noun="bucket" by="name" value={pick} onChange={setPick} />
          <Input value={pickPrefix} onChange={(e) => setPickPrefix(e.target.value)} placeholder="folder/ (optional)" className="font-mono" aria-label="Folder" />
          <DialogFooter><Button variant="outline" className="bg-card" onClick={() => setBrowse(false)}>Cancel</Button><Button disabled={!pick} onClick={() => (setDest(s3Uri(pick!, pickPrefix)), setBrowse(false))} data-testid="browse-choose">Choose</Button></DialogFooter>
        </DialogContent>
      </Dialog>
    </FormPage>
  );
}

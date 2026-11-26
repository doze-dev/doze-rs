// Copy, Move and Rename an object: the source, where the copy goes (any bucket of this account), and which settings it keeps.
// One CopyObject (the kind's own operation, any bucket); Move and Rename follow it with the delete of the source.
import { useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useResourceList } from "../../../api/resources";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { DataTable } from "@doze/ui/components/app/data-table";
import { filled, pairsOf, rowsFrom, rowsValid } from "@doze/ui/components/app/key-values";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Input } from "@doze/ui/components/ui/input";
import { Field, FormPage, FormSection, ResourcePicker, TAG_RULES, callKindOp, forgetDeleted, scopeOf } from "../../../kit";
import { useOpQuery } from "../deep/api";
import { callOp } from "../deep/api";
import type { OwnProps } from "../deep/registry";
import { bucketOf, crumbs, NONE, useBucketNav, useSetting, useStale } from "./api";
import { bpaOf, encryptionOf, notificationRules, ownershipOf, SSE_WORDS } from "./config";
import { type SpecifyState, SpecifySettings } from "./copy-settings";
import { aclChoices, sourceMetadata, specifyProps } from "./copy-props";
import { RadioCards } from "./controls";
import { setFlash } from "./flash";
import { NO_SSE, sseOf } from "./upload-controls";
import { Mono, Rows } from "./parts";
import { baseName, copyNotifies, copySource, s3Uri, shortDate, size } from "./words";

type Head = { ContentLength?: number; LastModified?: string | number; StorageClass?: string; ContentType?: string; CacheControl?: string; ContentDisposition?: string; ContentEncoding?: string; ContentLanguage?: string; Metadata?: Record<string, string> };
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
  const stale = useStale(name!);
  const qc = useQueryClient();
  const folder = key.includes("/") ? key.slice(0, key.lastIndexOf("/") + 1) : "";
  const head = useOpQuery<Head>(bucketOf(name!), "HeadObject", { Key: key }, ["head", key]);
  const sourceTags = useOpQuery<{ TagSet?: { Key: string; Value: string }[] }>(bucketOf(name!), "GetObjectTagging", { Key: key }, ["tags", key]);
  const [dest, setDest] = useState(`s3://${name}/${folder}`);
  const [newName, setNewName] = useState(baseName(key));
  const [settings, setSettings] = useState<"copy" | "none" | "specify">("copy");
  const [edits, setEdits] = useState<Partial<SpecifyState>>({});
  const [browse, setBrowse] = useState(false);
  const [pick, setPick] = useState<string | null>(null);
  const [pickPrefix, setPickPrefix] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const parsed = mode === "rename" ? { bucket: name!, prefix: folder } : parseUri(dest);
  const destKey = parsed ? (mode === "rename" ? `${folder}${newName}` : `${parsed.prefix}${baseName(key)}`) : "";
  // A destination typed half-way names no bucket yet: its settings are not read (S3 would answer NoSuchBucket for each letter).
  const buckets = useResourceList("s3", "buckets");
  const destBucket = parsed?.bucket ?? name!;
  const known = destBucket === name || buckets.hasNextPage === true || (buckets.data?.pages.some((p) => p.rows.some((r) => r.name === destBucket)) ?? false);
  const dv = useSetting<{ Status?: string }>(destBucket, "GetBucketVersioning", [], {}, known);
  const de = useSetting(destBucket, "GetBucketEncryption", NONE.encryption, {}, known);
  const dl = useSetting<{ ObjectLockConfiguration?: { ObjectLockEnabled?: string } }>(destBucket, "GetObjectLockConfiguration", NONE.lock, {}, known);
  const dn = useSetting(destBucket, "GetBucketNotificationConfiguration", [], {}, known);
  const dow = useSetting(destBucket, "GetBucketOwnershipControls", NONE.ownership, {}, known);
  const dpa = useSetting(destBucket, "GetPublicAccessBlock", NONE.bpa, {}, known);
  // What the source has, as the settings start; what the person changed is laid over it.
  const start = useMemo<SpecifyState>(() => ({ metadata: "copy", metaRows: rowsFrom(sourceMetadata(head.data)), storageClass: head.data?.StorageClass ?? "STANDARD", acl: "", tags: rowsFrom((sourceTags.data?.TagSet ?? []).map((t) => ({ key: t.Key, value: t.Value }))), sse: NO_SSE, checksum: "" }), [head.data, sourceTags.data]);
  const enforced = ownershipOf(dow.data) === "BucketOwnerEnforced";
  const blockPublicAcls = !dpa.absent && dpa.data !== undefined && bpaOf(dpa.data).BlockPublicAcls;
  const chosen = { ...start, ...edits };
  // An ACL the destination does not take (the person moved the destination after choosing one) is not sent.
  const acl = aclChoices(enforced, blockPublicAcls).some((a) => a.value === chosen.acl && !a.why) ? chosen.acl : "";
  const state: SpecifyState = { ...chosen, acl };
  const reading = settings === "specify" && (head.isPending || sourceTags.isPending);
  const specify = settings === "specify" && !reading && rowsValid(state.tags, { ...TAG_RULES, maxRows: 10 }) ? specifyProps({ metadata: state.metadata, metaRows: pairsOf(filled(state.metaRows)), storageClass: state.storageClass, sourceClass: start.storageClass, acl: state.acl, tags: pairsOf(filled(state.tags)), encryption: sseOf(state.sse), kmsKey: state.sse.kmsKey, checksum: state.checksum }) : null;
  const input = parsed && destKey && (settings !== "specify" || specify) ? { Bucket: parsed.bucket, Key: destKey, CopySource: copySource(name!, key), ...(settings === "none" ? { MetadataDirective: "REPLACE", TaggingDirective: "REPLACE" } : specify ?? {}) } : null;
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
      // A move or a rename deleted the source: what was read of it is not read again (S3 answers 404 for it).
      if (mode === "copy") stale();
      else await forgetDeleted(qc, scopeOf("s3"), [key]);
      setFlash({ title: `${mode === "copy" ? "Copied" : mode === "move" ? "Moved" : "Renamed"} ${baseName(key)} to ${s3Uri(input.Bucket, input.Key)}.`, returned: mode === "copy" ? "CopyObject answered with the new object's ETag." : "CopyObject, then DeleteObject of the source." });
      back();
    } catch (e) {
      setError(e);
      setBusy(false);
    }
  };
  const notes = !known ? [null, null, null] : [dv.data?.Status ? dv.data.Status : "Disabled", de.data ? (encryptionOf(de.data) ? SSE_WORDS[encryptionOf(de.data)!.algorithm] : "None") : "Server-side encryption with Amazon S3 managed keys (SSE-S3)", dl.data?.ObjectLockConfiguration?.ObjectLockEnabled === "Enabled" ? "Enabled" : "Disabled"];
  return (
    <FormPage title={MODES[mode]} sub={mode === "rename" ? <>Give <Mono>{baseName(key)}</Mono> a new name. It stays in {s3Uri(name!, folder)}.</> : <>{MODES[mode]} <Mono>{baseName(key)}</Mono> to a new location. Choose where the copy goes and which settings it keeps.</>}
      crumbs={[...crumbs(name!), { label: baseName(key), to: `/resources/s3/buckets/${encodeURIComponent(name!)}` }, { label: MODES[mode], to: `/resources/s3/buckets/${encodeURIComponent(name!)}` }]}
      request={input ? { service: "s3", operation: "CopyObject", input } : null} showInput
      callNote={<>The copy resets the creation date and any system-controlled metadata. {mode === "copy" ? "Moving is the same call followed by a delete of the source." : "This is the same call followed by a delete of the source."}</>}
      refusal={said ? { code: said.code?.split(" · ")[0] ?? "Error", message: said.message } : null}
      banner={said ? <Alert tone="bad" title="Not done." testId="copy-refused">{said.message}</Alert> : undefined}
      submitLabel={MODES[mode]} pending={busy} disabled={!input || same} disabledWhy={same ? "The destination is the object itself." : reading ? "Reading the source's settings." : "Give a destination like s3://bucket/folder/, and settings that are complete."} onCancel={back} onSubmit={() => void run()}>
      <FormSection title="Source">
        <DataTable aria-label="Source" data={[key]} rowId={(k) => k} columns={[{ id: "n", header: "Name", accessorFn: (k) => k, meta: { mono: true, wrap: true }, min: 200 }, { id: "m", header: "Last modified", size: 300, accessorFn: () => "", cell: () => shortDate(head.data?.LastModified) }, { id: "s", header: "Size", size: 110, meta: { numeric: true }, accessorFn: () => 0, cell: () => size(head.data?.ContentLength) }]} />
      </FormSection>
      <FormSection title="Destination">
        <div className="grid gap-1" data-testid="destination-type">
          <span className="text-[14px] font-medium text-bright">Destination type</span>
          <span className="text-[14px] text-foreground">General purpose bucket</span>
          <p className="text-[13px] text-muted-foreground">Not available in doze-aws: directory buckets are an express-zone feature, so this is the only destination type.</p>
        </div>
        {mode === "rename" ? (
          <Field label="New name">{(id) => <Input id={id} value={newName} onChange={(e) => setNewName(e.target.value)} className="font-mono" data-testid="rename-name" />}</Field>
        ) : (
          <Field label="Destination" testId="field-destination" error={parsed ? undefined : "Write it as s3://bucket/folder/."} hint={<>{known && destKey && !dn.isPending ? copyNotifies(destBucket, destKey, notificationRules(dn.data)) : "A copy is written as a new object, so a bucket that notifies on creation notifies again."} Copied objects do not keep the Object Lock settings of the original, and the console copies objects of less than 5 GB.</>}>
            {(id) => <div className="flex gap-2"><Input id={id} value={dest} onChange={(e) => setDest(e.target.value)} className="font-mono" spellCheck={false} data-testid="copy-dest" /><Button type="button" variant="outline" className="bg-card" onClick={() => setBrowse(true)} data-testid="browse-s3">Browse S3</Button></div>}
          </Field>
        )}
        {parsed && destKey && <p className="text-[13px] text-muted-foreground">The new key will be <Mono>{s3Uri(parsed.bucket, destKey)}</Mono></p>}
      </FormSection>
      <FormSection title="Destination details"><Rows rows={[["Bucket Versioning", notes[0]], ["Default encryption", notes[1]], ["Object Lock", notes[2]]]} /></FormSection>
      <FormSection title="Additional copy settings">
        <RadioCards name="copy-settings" value={settings} onChange={setSettings} options={[{ value: "copy", title: "Copy source settings", description: "The default." }, { value: "none", title: "Don’t specify settings", description: "Copy the object without the source settings attributes." }, { value: "specify", title: "Specify settings", description: "Set storage class, ACLs, object tags, metadata, server-side encryption and additional checksums for the copy." }]} />
        {settings === "specify" && <SpecifySettings state={state} set={(p) => setEdits((e) => ({ ...e, ...p }))} enforced={enforced} blockPublicAcls={blockPublicAcls} />}
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

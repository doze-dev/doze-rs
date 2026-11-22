// Upload: the files and folders to put (chosen, or dropped), where they go, and the properties they get; each file is one PutObject
// through the console, and the Status page that follows lists each one's answer. Create folder is the same call with an empty body.
import { FilePlusIcon, FolderUpIcon, UploadCloudIcon } from "lucide-react";
import { useRef, useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { DataTable } from "@doze/ui/components/app/data-table";
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { pairsOf, type KeyValueRow } from "@doze/ui/components/app/key-values";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { cn } from "@doze/ui/lib/utils";
import { ApiFailure } from "@doze/ui/lib/api";
import { Field, FormSection, FormPage, LocalStatus, TAG_RULES } from "../../../kit";
import { UPLOAD_MAX, putObject } from "../deep/api";
import type { OwnProps } from "../deep/registry";
import { Calls } from "../detail-page";
import { crumbs, useBucket, useBucketNav, useStale } from "./api";
import { RadioCards } from "./controls";
import { setFlash } from "./flash";
import { ActionPage, Mono } from "./parts";
import { Card } from "../deep/parts";
import { CHECKSUMS, STORAGE_CLASSES, type Sent, sentOf, setSent, uploadProps } from "./upload-props";
import { callKey, count, listWords, s3Uri, size, storageWords, uploadKey, versioningWords } from "./words";

type Chosen = { file: File; key: string };

export function Upload({ name, search }: OwnProps) {
  const prefix = search.p ?? "";
  const go = useBucketNav(name!);
  const stale = useStale(name!);
  const bucket = useBucket(name!);
  const filesIn = useRef<HTMLInputElement>(null);
  const folderIn = useRef<HTMLInputElement>(null);
  const [chosen, setChosen] = useState<Chosen[]>([]);
  const [over, setOver] = useState(false);
  const [storageClass, setStorageClass] = useState("STANDARD");
  const [encryption, setEncryption] = useState<"bucket" | "AES256" | "aws:kms">("bucket");
  const [kmsKey, setKmsKey] = useState("");
  const [checksum, setChecksum] = useState("");
  const [tags, setTags] = useState<KeyValueRow[]>([]);
  const [meta, setMeta] = useState<KeyValueRow[]>([]);
  const [busy, setBusy] = useState<number | null>(null);
  const add = (files: FileList | File[] | null) => {
    const list = [...(files ?? [])].map((file) => ({ file, key: uploadKey(prefix, file) }));
    setChosen((c) => [...c, ...list.filter((l) => !c.some((x) => x.key === l.key))]);
  };
  const props = uploadProps({ storageClass, encryption, kmsKey, checksum, tags: pairsOf(tags), metadata: pairsOf(meta) });
  const total = chosen.reduce((a, c) => a + c.file.size, 0);
  const big = chosen.filter((c) => c.file.size > UPLOAD_MAX);
  const first = chosen[0];
  const typeOf = (f: File) => f.type || "application/octet-stream";
  const run = async () => {
    const rows: Sent[] = [];
    for (const [i, c] of chosen.entries()) {
      setBusy(i);
      try {
        await putObject(name!, c.key, c.file, c.file.type, props);
        rows.push({ key: c.key, type: typeOf(c.file), size: c.file.size, status: 200 });
      } catch (e) {
        rows.push({ key: c.key, type: typeOf(c.file), size: c.file.size, status: e instanceof ApiFailure ? e.status : 0, message: `${errorWords(e).code?.split(" · ")[0] ?? ""} ${errorWords(e).message}`.trim() });
      }
    }
    setSent(name!, rows);
    stale();
    setBusy(null);
    go({ do: "upload-status", ...(prefix ? { p: prefix } : {}) });
  };
  return (
    <FormPage
      title="Upload"
      sub={`Choose the files and folders to upload to this bucket. Through the console a file can be at most ${size(UPLOAD_MAX)}; for a larger one, use the AWS CLI, an AWS SDK or the Amazon S3 REST API.`}
      crumbs={[...crumbs(name!), { label: "Upload", to: `/resources/s3/buckets/${encodeURIComponent(name!)}` }]}
      request={first ? { service: "s3", operation: "PutObject", input: { Bucket: name, Key: first.key, ContentType: typeOf(first.file), ...props } } : null}
      showInput
      callNote={<>This console sends one PutObject per file; the key is the folder name plus the file name. Anything you leave at its default is left out of the call. {chosen.length > 1 ? `The call shown is the first of ${chosen.length}.` : ""}</>}
      submitLabel="Upload"
      pending={busy !== null}
      disabled={chosen.length === 0 || big.length > 0}
      disabledWhy={chosen.length === 0 ? "Choose at least one file." : `${big.map((b) => b.key).join(", ")} is over ${size(UPLOAD_MAX)}.`}
      onCancel={() => go(prefix ? { p: prefix } : {})}
      onSubmit={() => void run()}
    >
      <FormSection title="Upload" description="Drag and drop files here, or choose Add files or Add folder. A folder uploads its files and subfolders, and each key starts with the folder name.">
        <div onDragOver={(e) => (e.preventDefault(), setOver(true))} onDragLeave={() => setOver(false)} onDrop={(e) => (e.preventDefault(), setOver(false), add(e.dataTransfer.files))} className={cn("grid justify-items-center gap-3 rounded-xl border-2 border-dashed px-6 py-10 text-center", over ? "border-primary bg-pick/40" : "bg-well/50")} data-testid="drop-zone">
          <UploadCloudIcon aria-hidden className="size-8 text-muted-foreground" />
          <p className="text-[14px] text-foreground">Drag and drop files and folders here</p>
          <div className="flex gap-3">
            <Button type="button" variant="outline" className="bg-card" onClick={() => filesIn.current?.click()} data-testid="add-files"><FilePlusIcon /> Add files</Button>
            <Button type="button" variant="outline" className="bg-card" onClick={() => folderIn.current?.click()} data-testid="add-folder"><FolderUpIcon /> Add folder</Button>
          </div>
          <input ref={filesIn} type="file" multiple hidden onChange={(e) => (add(e.target.files), (e.target.value = ""))} data-testid="upload-files" />
          <input ref={folderIn} type="file" multiple hidden {...{ webkitdirectory: "" }} onChange={(e) => (add(e.target.files), (e.target.value = ""))} data-testid="upload-folder" />
        </div>
      </FormSection>
      <FormSection title="Files and folders" description={`${count(chosen.length, "file")} total${chosen.length > 0 ? `, ${size(total)}` : ""}. The key of each object is the folder name plus the file name.`}>
        {chosen.length === 0 ? <p className="text-[13.5px] text-muted-foreground">Nothing chosen yet.</p> : <DataTable aria-label="Files and folders" data={chosen} rowId={(c) => c.key} maxHeight={320} columns={[{ id: "key", header: "Key", accessorFn: (c) => c.key, meta: { mono: true }, min: 200 }, { id: "type", header: "Content type", size: 180, accessorFn: (c) => typeOf(c.file), meta: { mono: true } }, { id: "size", header: "Size", size: 110, meta: { numeric: true }, accessorFn: (c) => c.file.size, cell: (c) => <span className={c.row.original.file.size > UPLOAD_MAX ? "text-bad" : undefined}>{size(c.row.original.file.size)}</span> }]} />}
      </FormSection>
      <FormSection title="Destination">
        <div className="grid gap-1"><span className="text-[13px] text-muted-foreground">Destination</span><Mono>{s3Uri(name!, prefix)}</Mono></div>
        <div className="grid gap-1"><span className="text-[13px] text-muted-foreground">Bucket Versioning</span><span>{bucket.versioning === "Disabled" ? "Disabled" : bucket.versioning}</span>
          <p className="text-[13px] text-muted-foreground">{bucket.versioning === "Enabled" ? "An upload to a key that already exists creates another version of the object instead of replacing it." : bucket.versioning === "Suspended" ? `Versioning is ${versioningWords("Suspended")}: an upload to a key that exists replaces its null version.` : "An upload to a key that already exists replaces the object."}</p></div>
      </FormSection>
      <FormSection title="Permissions" collapsible defaultOpen={false} summary="Access control list (ACL) permissions for the uploaded objects. Stored, not applied locally when the bucket owner is enforced.">
        <p className="text-[13px] text-muted-foreground">An upload from the console sets no ACL: the object gets the bucket&apos;s ownership rules. Change Object Ownership on the bucket&apos;s Permissions tab.</p>
      </FormSection>
      <FormSection title="Properties" collapsible defaultOpen={false} summary={`Storage class ${storageWords(storageClass)}, ${encryption === "bucket" ? "the bucket's default encryption" : encryption}${checksum ? `, ${checksum} checksum` : ""}.`}>
        <Field label="Storage class">
          {(id) => <select id={id} value={storageClass} onChange={(e) => setStorageClass(e.target.value)} className="h-9 w-full max-w-sm rounded-md border bg-card px-3 text-[14px]" data-testid="storage-class">{STORAGE_CLASSES.map((c) => <option key={c} value={c}>S3 {storageWords(c)}</option>)}</select>}
        </Field>
        <LocalStatus state="stored" reason="Every object is a plain file whatever class you name; the class is kept on the object and listed, and no lifecycle transition moves it." />
        <RadioCards name="upload-sse" value={encryption} onChange={setEncryption} options={[{ value: "bucket", title: "Use bucket settings for default encryption" }, { value: "AES256", title: "Override: Amazon S3 managed key (SSE-S3)" }, { value: "aws:kms", title: "Override: AWS Key Management Service key (SSE-KMS)" }]} />
        {encryption === "aws:kms" && <Field label="KMS key">{(id) => <Input id={id} value={kmsKey} onChange={(e) => setKmsKey(e.target.value)} placeholder="alias/larder-app or a key ARN" className="font-mono" />}</Field>}
        <LocalStatus state="stored" reason="The encryption setting is echoed back on the object; the bytes are plain files either way." />
        <Field label="Additional checksums" help="Choose a checksum function. S3 calculates and stores it after receiving the whole object, and refuses the upload if a value you precalculated does not match.">
          {(id) => <select id={id} value={checksum} onChange={(e) => setChecksum(e.target.value)} className="h-9 w-full max-w-sm rounded-md border bg-card px-3 text-[14px]" data-testid="checksum"><option value="">Off</option>{CHECKSUMS.map((c) => <option key={c} value={c}>{c}</option>)}</select>}
        </Field>
        <Field label="Tags (optional)" help="Each tag is a key-value pair, applied to every file in this upload. 10 tags per object at most; a key can be up to 128 and a value up to 255 Unicode characters, and both are case sensitive."><KeyValueEditor rows={tags} onChange={setTags} rules={{ ...TAG_RULES, maxRows: 10 }} addLabel="Add tag" testId="upload-tags" /></Field>
        <Field label="Metadata (optional)" help="System defined (Cache-Control, Content-Disposition, Content-Encoding, Content-Language) or user defined (a key that starts with x-amz-meta-, or any other key, stored as user metadata). User-defined metadata can be as large as 2 KB and must be US-ASCII."><KeyValueEditor rows={meta} onChange={setMeta} addLabel="Add metadata" keyLabel="Key" testId="upload-meta" /></Field>
      </FormSection>
    </FormPage>
  );
}

export function UploadStatus({ name, search }: OwnProps) {
  const go = useBucketNav(name!);
  const rows = sentOf(name!);
  const ok = rows?.filter((r) => r.status >= 200 && r.status < 300).length ?? 0;
  const bucket = useBucket(name!);
  const prefix = search.p ?? "";
  return (
    <ActionPage title="Upload: status" crumbs={[...crumbs(name!), { label: "Upload: status", to: `/resources/s3/buckets/${encodeURIComponent(name!)}` }]} actions={<Button onClick={() => (setFlash({ title: `${count(ok, "file")} uploaded.`, returned: `${count(ok, "PutObject call")} succeeded.` }), go(prefix ? { p: prefix } : {}))} data-testid="status-exit">Exit</Button>} testId="upload-status-page">
      {rows ? (
        <>
          <Alert tone={ok === rows.length ? "good" : "warn"} title={`${count(ok, "PutObject call")} succeeded in ${name}.${ok < rows.length ? ` ${rows.length - ok} failed.` : ""}`} testId="upload-summary">
            {bucket.detail?.sentence?.includes("notify") ? `Each ObjectCreated event was sent to ${listWords((bucket.detail.wired ?? []).filter((w) => w.label.startsWith("Notifies")).map((w) => w.to.name))}.` : "Nothing notifies on these objects: the bucket has no event notification rule."}
          </Alert>
          <Card title="Files and folders" lede={`${count(rows.length, "file")} total, ${size(rows.reduce((a, r) => a + r.size, 0))}, uploaded to ${s3Uri(name!, prefix)}.`}>
            <DataTable aria-label="Uploaded files" data={rows} rowId={(r) => r.key} maxHeight={360} columns={[{ id: "key", header: "Key", accessorKey: "key", meta: { mono: true }, min: 200 }, { id: "type", header: "Content type", size: 180, accessorKey: "type", meta: { mono: true } }, { id: "size", header: "Size", size: 110, meta: { numeric: true }, accessorFn: (r) => r.size, cell: (c) => size(c.row.original.size) }, { id: "status", header: "PutObject", size: 260, accessorFn: (r) => r.status, cell: (c) => (c.row.original.status >= 200 && c.row.original.status < 300 ? <span className="text-good">{c.row.original.status}</span> : <span className="text-bad" title={c.row.original.message}>{c.row.original.status || "–"} {c.row.original.message}</span>) }]} />
          </Card>
          {bucket.detail && <Calls d={bucket.detail} pin={["PutObject"]} note={(c) => callKey(name!, c.resource)} />}
        </>
      ) : (
        <p className="text-[13.5px] text-muted-foreground">This page has no upload to report: it was not made in this window.</p>
      )}
    </ActionPage>
  );
}

export function FolderCreate({ name, search }: OwnProps) {
  const prefix = search.p ?? "";
  const go = useBucketNav(name!);
  const stale = useStale(name!);
  const [folder, setFolder] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const wrong = folder.includes("/") ? "A folder's name cannot hold a /: make one folder, then open it to make the next." : new TextEncoder().encode(prefix + folder).length > 1000 ? "Too long: a key is at most 1,024 bytes." : null;
  const key = `${prefix}${folder.trim()}/`;
  const said = error ? errorWords(error) : null;
  return (
    <FormPage title="Create folder" sub={<>Folders organise objects by a shared prefix. The folder is created in <Mono>{s3Uri(name!, prefix)}</Mono>.</>} crumbs={[...crumbs(name!), { label: "Create folder", to: `/resources/s3/buckets/${encodeURIComponent(name!)}` }]}
      request={folder.trim() ? { service: "s3", operation: "PutObject", input: { Bucket: name, Key: key, ContentLength: 0 } } : null} showInput
      callNote="There is no folder in S3. The console writes a 0-byte object whose key is the folder name plus a trailing slash, and shows any key that ends in a slash as a folder."
      refusal={said ? { code: said.code?.split(" · ")[0] ?? "Error", message: said.message } : null}
      submitLabel="Create folder" pending={busy} disabled={!folder.trim() || Boolean(wrong)} disabledWhy={wrong ?? "Name the folder."} onCancel={() => go(prefix ? { tab: "objects", p: prefix } : {})}
      onSubmit={() => { setBusy(true); setError(null); void putObject(name!, key, new Blob([]), "application/x-directory").then(() => (stale(), setFlash({ title: `Folder ${key} created.`, returned: "PutObject wrote a 0-byte object whose key ends in a slash." }), go({ tab: "objects", p: prefix || undefined })), (e) => (setError(e), setBusy(false))); }}>
      <FormSection title="Folder">
        <Field label="Folder name" error={wrong ?? undefined} help="The folder name becomes part of every key inside it, and a key is limited to 1,024 bytes. Folders can be created and deleted but cannot be renamed." hint={folder.trim() ? <>The key will be <Mono>{key}</Mono></> : undefined}>
          {(id) => <Input id={id} value={folder} onChange={(e) => setFolder(e.target.value)} placeholder="seasonal" className="font-mono" autoComplete="off" spellCheck={false} data-testid="folder-name" />}
        </Field>
      </FormSection>
      <FormSection title="Server-side encryption" optional description="Needed only if your bucket policy requires objects to be encrypted with a specific encryption key: then you must specify the same key when you create the folder, or folder creation fails.">
        <LocalStatus state="stored" reason="Objects are plain files under the data directory whatever key you name." />
      </FormSection>
    </FormPage>
  );
}

// The bucket objects page's dialogs: upload files into the folder on screen (each a PutObject through the console, at most
// 25 MiB), create a folder (an empty object whose key ends in `/`), delete what is selected (DeleteObjects; a folder is every
// object under it).
import { useQueryClient } from "@tanstack/react-query";
import { UploadIcon } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { bytes } from "@doze/ui/lib/format";
import { type Target, UPLOAD_MAX, callOp, deepKey, putObject } from "../deep/api";
import { Confirm, Refused } from "../deep/parts";
import type { Row } from "./objects";

/** The page's reads of the bucket, stale after a change. */
function useStale(t: Target) {
  const qc = useQueryClient();
  return () => void qc.invalidateQueries({ queryKey: deepKey(t) });
}

export function Upload({ open, onOpenChange, bucket, prefix, t }: { open: boolean; onOpenChange: (o: boolean) => void; bucket: string; prefix: string; t: Target }) {
  const [files, setFiles] = useState<File[]>([]);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState(0);
  const [error, setError] = useState<unknown>(null);
  const stale = useStale(t);
  const tooBig = files.filter((f) => f.size > UPLOAD_MAX);
  const change = (o: boolean) => {
    if (busy) return;
    if (!o) {
      setFiles([]);
      setError(null);
      setDone(0);
    }
    onOpenChange(o);
  };
  const run = async () => {
    setBusy(true);
    setError(null);
    let n = 0;
    try {
      for (const f of files) {
        await putObject(bucket, prefix + f.name, f, f.type);
        n += 1;
        setDone(n);
      }
      toast.success(n === 1 ? `${files[0]?.name} uploaded` : `${n} files uploaded`);
      setBusy(false);
      stale();
      change(false);
    } catch (e) {
      setError(e);
      setBusy(false);
      if (n > 0) stale();
    }
  };
  return (
    <Dialog open={open} onOpenChange={change}>
      <DialogContent className="sm:max-w-[600px]" data-testid="upload-dialog">
        <DialogHeader>
          <DialogTitle className="text-[21px] font-semibold">Upload</DialogTitle>
          <DialogDescription>
            Into <span className="font-mono text-foreground">s3://{bucket}/{prefix}</span>. Each file is one <span className="font-mono text-foreground">PutObject</span> as the caller <span className="font-mono text-foreground">console</span>, named by the file. At most {bytes(UPLOAD_MAX)} a file through the console; put a larger one with the AWS CLI or an SDK.
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-1.5">
          <Label htmlFor="upload-files">Files</Label>
          <Input id="upload-files" type="file" multiple onChange={(e) => (setFiles([...(e.target.files ?? [])]), setDone(0), setError(null))} className="bg-card" data-testid="upload-files" />
        </div>
        {files.length > 0 && (
          <ul className="grid max-h-56 gap-1 overflow-auto rounded-[10px] border bg-muted px-4 py-3 text-[13px]" data-testid="upload-list">
            {files.map((f, i) => (
              <li key={f.name} className="flex justify-between gap-3 font-mono">
                <span className="truncate">
                  {i < done ? "✓ " : ""}
                  {prefix}
                  {f.name}
                </span>
                <span className={f.size > UPLOAD_MAX ? "text-bad" : "text-muted-foreground"}>{bytes(f.size)}</span>
              </li>
            ))}
          </ul>
        )}
        {tooBig.length > 0 && <p className="text-[13px] text-bad">{tooBig.map((f) => f.name).join(", ")} {tooBig.length === 1 ? "is" : "are"} over {bytes(UPLOAD_MAX)}.</p>}
        <Refused operation="PutObject" error={error} testId="upload-error" />
        <DialogFooter>
          <Button type="button" variant="outline" className="bg-card" disabled={busy} onClick={() => change(false)}>
            Cancel
          </Button>
          <Button type="button" disabled={files.length === 0 || tooBig.length > 0 || busy} onClick={() => void run()} data-testid="upload-submit">
            <UploadIcon /> {busy ? `Uploading ${done + 1} of ${files.length}…` : files.length > 1 ? `Upload ${files.length} files` : "Upload"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** What is wrong with a folder's name, if anything. */
export function folderWrong(name: string): string | null {
  if (!name.trim()) return null;
  if (name.includes("/")) return "A folder's name cannot hold a /: make one folder, then open it to make the next.";
  if (new TextEncoder().encode(name).length > 1000) return "Too long: a key is at most 1,024 bytes.";
  return null;
}

export function NewFolder({ open, onOpenChange, bucket, prefix, t }: { open: boolean; onOpenChange: (o: boolean) => void; bucket: string; prefix: string; t: Target }) {
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const stale = useStale(t);
  const wrong = folderWrong(name);
  const change = (o: boolean) => {
    if (!o) {
      setName("");
      setError(null);
    }
    onOpenChange(o);
  };
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim() || wrong || busy) return;
    setBusy(true);
    try {
      await putObject(bucket, `${prefix}${name.trim()}/`, new Blob([]), "application/x-directory");
      toast.success(`${name.trim()}/ created`);
      stale();
      change(false);
    } catch (err) {
      setError(err);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog open={open} onOpenChange={change}>
      <DialogContent className="sm:max-w-[560px]" data-testid="folder-dialog">
        <form onSubmit={(e) => void submit(e)} className="grid gap-5">
          <DialogHeader>
            <DialogTitle className="text-[21px] font-semibold">Create folder</DialogTitle>
            <DialogDescription>
              S3 has no folders: this puts an empty object whose key ends in <span className="font-mono text-foreground">/</span>, which lists as a folder. Its objects are what share its prefix.
            </DialogDescription>
          </DialogHeader>
          <div className="grid gap-1.5">
            <Label htmlFor="folder-name">Name</Label>
            <Input id="folder-name" value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. banners" className="bg-card font-mono text-[13px]" autoComplete="off" spellCheck={false} data-testid="folder-name" />
            <p className="text-[12.5px] text-muted-foreground">
              The key will be <span className="font-mono text-foreground">{prefix}{name.trim() || "…"}/</span>
            </p>
            {wrong && <p className="text-[13px] text-bad">{wrong}</p>}
          </div>
          <Refused operation="PutObject" error={error} />
          <DialogFooter>
            <Button type="button" variant="outline" className="bg-card" onClick={() => change(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={!name.trim() || !!wrong || busy} data-testid="folder-submit">
              {busy ? "Creating…" : "Create folder"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

type Listing = { Contents?: { Key: string }[]; IsTruncated?: boolean; NextContinuationToken?: string };

/** Every key under `prefix`, page by page. */
async function keysUnder(t: Target, prefix: string): Promise<string[]> {
  const keys: string[] = [];
  let token: string | undefined;
  do {
    const page: Listing = await callOp<Listing>(t, "ListObjectsV2", { Prefix: prefix, MaxKeys: 1000, ...(token ? { ContinuationToken: token } : {}) });
    keys.push(...(page.Contents ?? []).map((c) => c.Key));
    token = page.IsTruncated ? page.NextContinuationToken : undefined;
  } while (token);
  return keys;
}

export function DeleteObjects({ open, onOpenChange, rows, t, onDone }: { open: boolean; onOpenChange: (o: boolean) => void; rows: Row[]; t: Target; onDone: () => void }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const stale = useStale(t);
  const folders = rows.filter((r) => r.folder);
  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      const keys = rows.filter((r) => !r.folder).map((r) => r.key);
      for (const f of folders) keys.push(...(await keysUnder(t, f.key)));
      let failed: { Key: string; Code: string; Message?: string }[] = [];
      for (let i = 0; i < keys.length; i += 1000) {
        const answer = await callOp<{ Errors?: { Key: string; Code: string; Message?: string }[] }>(t, "DeleteObjects", { Delete: { Objects: keys.slice(i, i + 1000).map((Key) => ({ Key })), Quiet: true } });
        failed = [...failed, ...(answer.Errors ?? [])];
      }
      stale();
      if (failed.length > 0) {
        setError(new Error(`${failed.length} of ${keys.length} could not be deleted: ${failed[0]?.Key} (${failed[0]?.Code}).`));
        return;
      }
      toast.success(keys.length === 1 ? "1 object deleted" : `${keys.length} objects deleted`);
      onDone();
      onOpenChange(false);
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Confirm open={open} onOpenChange={(o) => (o || setError(null), onOpenChange(o))} title={rows.length === 1 ? "Delete object" : `Delete ${rows.length} items`} verb="Delete" operation="DeleteObjects" pending={busy} error={error} run={() => void run()} testId="objects-delete-dialog">
      Delete{" "}
      {rows.slice(0, 5).map((r, i) => (
        <span key={r.key}>
          {i > 0 && ", "}
          <b className="font-mono font-semibold break-all text-bright">{r.key}</b>
        </span>
      ))}
      {rows.length > 5 ? ` and ${rows.length - 5} more` : ""}?{folders.length > 0 ? " A folder is deleted with every object under it." : ""}
    </Confirm>
  );
}

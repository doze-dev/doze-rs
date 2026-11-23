// The Actions menu of a bucket's objects: Download as, Share with a presigned URL, Calculate total size, Copy, Move, Rename
// object. A choice that needs a page of its own (copy, move, rename, a presigned link) goes to it; the two that are a small
// answer (a saved name, a total) are a dialog over the list.
import { ChevronDownIcon } from "lucide-react";
import { useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@doze/ui/components/ui/dropdown-menu";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { callOp, downloadObject, type Target } from "../deep/api";
import type { Row } from "./listing";
import { baseName, count, size } from "./words";

type Listing = { Contents?: { Key: string; Size?: number }[]; IsTruncated?: boolean; NextContinuationToken?: string };

/** The total size of what is chosen: each object's, and every object under each folder (ListObjectsV2, page by page). */
export async function totalSize(t: Target, rows: Row[]): Promise<{ objects: number; bytes: number }> {
  let objects = 0;
  let bytes = 0;
  for (const r of rows) {
    if (!r.folder) {
      objects += 1;
      bytes += r.size ?? 0;
      continue;
    }
    let token: string | undefined;
    do {
      const page: Listing = await callOp<Listing>(t, "ListObjectsV2", { Prefix: r.key, MaxKeys: 1000, ...(token ? { ContinuationToken: token } : {}) });
      for (const o of page.Contents ?? []) {
        if (o.Key === r.key) continue;
        objects += 1;
        bytes += o.Size ?? 0;
      }
      token = page.IsTruncated ? page.NextContinuationToken : undefined;
    } while (token);
  }
  return { objects, bytes };
}

/** `label`, `testId` and `extra` are for an object's own page: its menu is `Object actions`, and ends with the Delete of that object. */
export function ActionsMenu({ t, rows, one, go, label = "Actions", testId = "objects-actions", align = "start", extra }: { t: Target; rows: Row[]; one: Row | null; go: (action: string, mode?: string) => void; label?: string; testId?: string; align?: "start" | "end"; extra?: React.ReactNode }) {
  const [dialog, setDialog] = useState<"total" | "download" | null>(null);
  const [total, setTotal] = useState<{ objects: number; bytes: number } | null>(null);
  const [saveAs, setSaveAs] = useState("");
  const [error, setError] = useState<unknown>(null);
  const open = async (d: "total" | "download") => {
    setError(null);
    setTotal(null);
    if (d === "download" && one) setSaveAs(baseName(one.key));
    setDialog(d);
    if (d === "total") {
      try {
        setTotal(await totalSize(t, rows));
      } catch (e) {
        setError(e);
      }
    }
  };
  const failed = error ? errorWords(error) : null;
  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button type="button" variant="outline" className="bg-card" disabled={rows.length === 0} data-testid={testId}>
            {label} <ChevronDownIcon />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align={align} className="min-w-60">
          <DropdownMenuItem disabled={!one} onSelect={() => void open("download")} data-testid="action-download-as">Download as</DropdownMenuItem>
          <DropdownMenuItem disabled={!one} onSelect={() => go("presign")} data-testid="action-presign">Share with a presigned URL</DropdownMenuItem>
          <DropdownMenuItem onSelect={() => void open("total")} data-testid="action-total">Calculate total size</DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem disabled={!one} onSelect={() => go("object-copy", "copy")} data-testid="action-copy">Copy</DropdownMenuItem>
          <DropdownMenuItem disabled={!one} onSelect={() => go("object-copy", "move")} data-testid="action-move">Move</DropdownMenuItem>
          <DropdownMenuItem disabled={!one} onSelect={() => go("object-copy", "rename")} data-testid="action-rename">Rename object</DropdownMenuItem>
          {extra}
        </DropdownMenuContent>
      </DropdownMenu>
      <Dialog open={dialog === "total"} onOpenChange={(o) => !o && setDialog(null)}>
        <DialogContent className="sm:max-w-[480px]" data-testid="total-dialog">
          <DialogHeader>
            <DialogTitle className="text-[21px] font-semibold">Total size</DialogTitle>
            <DialogDescription>{rows.length === 1 ? baseName(rows[0]!.key) : `${count(rows.length, "item")} chosen`}</DialogDescription>
          </DialogHeader>
          {failed ? <Alert tone="bad" title="The size could not be worked out">{failed.message}</Alert> : total ? <p className="text-[15px] text-bright" data-testid="total-size"><b>{size(total.bytes)}</b> in {count(total.objects, "object")}.</p> : <p className="text-muted-foreground">Adding up…</p>}
          <DialogFooter><Button variant="outline" className="bg-card" onClick={() => setDialog(null)}>Close</Button></DialogFooter>
        </DialogContent>
      </Dialog>
      <Dialog open={dialog === "download"} onOpenChange={(o) => !o && setDialog(null)}>
        <DialogContent className="sm:max-w-[480px]" data-testid="download-as-dialog">
          <DialogHeader>
            <DialogTitle className="text-[21px] font-semibold">Download as</DialogTitle>
            <DialogDescription>Save {one ? baseName(one.key) : "the object"} under another name. The object itself is not renamed.</DialogDescription>
          </DialogHeader>
          <div className="grid gap-1.5">
            <Label htmlFor="save-as">File name</Label>
            <Input id="save-as" value={saveAs} onChange={(e) => setSaveAs(e.target.value)} className="font-mono" data-testid="save-as" />
          </div>
          {failed && <Alert tone="bad" title="The download failed">{failed.message}</Alert>}
          <DialogFooter>
            <Button variant="outline" className="bg-card" onClick={() => setDialog(null)}>Cancel</Button>
            <Button disabled={!saveAs.trim() || !one} data-testid="save-as-submit" onClick={() => one && void downloadObject(t.name, one.key, saveAs.trim()).then(() => setDialog(null), setError)}>Download</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}

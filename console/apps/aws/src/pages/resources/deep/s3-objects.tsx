// A bucket's objects: its folders (shared key prefixes) and objects one level at a time, a breadcrumb back up, pages by
// continuation token, an object's drawer, and upload, download, a new folder and delete. ListObjectsV2 with a delimiter of
// `/`, as the caller `console`.
import { getRouteApi, Link, useNavigate } from "@tanstack/react-router";
import { ChevronLeftIcon, ChevronRightIcon, DownloadIcon, FileIcon, FolderIcon, FolderPlusIcon, RefreshCwIcon, SearchIcon, Trash2Icon, UploadIcon } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { errorWords } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { Button } from "@doze/ui/components/ui/button";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Input } from "@doze/ui/components/ui/input";
import { bytes } from "@doze/ui/lib/format";
import { cn } from "@doze/ui/lib/utils";
import { downloadObject, useOpQuery } from "./api";
import type { DeepProps } from "./pages";
import { at, Card, Refused } from "./parts";
import { DeleteObjects, NewFolder, Upload } from "./s3-dialogs";
import { ObjectSheet } from "./s3-object-sheet";

/** One row: a folder (a common prefix) or an object. */
export type Row = { key: string; folder: boolean; size?: number; modified?: string | number; storage?: string };

type Listing = { Contents?: { Key: string; Size?: number; LastModified?: string | number; StorageClass?: string }[]; CommonPrefixes?: { Prefix: string }[]; IsTruncated?: boolean; NextContinuationToken?: string; KeyCount?: number };

/** The rows of a listing at `prefix`: its folders first, then its objects; the folder's own marker (`prefix` itself) left out. */
export function rowsOf(l: Listing, prefix: string): Row[] {
  const folders = (l.CommonPrefixes ?? []).map((p) => ({ key: p.Prefix, folder: true }));
  const objects = (l.Contents ?? []).filter((o) => o.Key !== prefix).map((o) => ({ key: o.Key, folder: false, size: o.Size, modified: o.LastModified, storage: o.StorageClass }));
  return [...folders, ...objects];
}

/** `a/b/` as the crumbs `a/` and `a/b/`, each with its label. */
export function crumbsOf(prefix: string): { label: string; prefix: string }[] {
  const parts = prefix.split("/").filter(Boolean);
  return parts.map((p, i) => ({ label: p, prefix: `${parts.slice(0, i + 1).join("/")}/` }));
}

const PAGE = 100;
const route = getRouteApi("/resources/$service/$kind/$name");

export function BucketObjects({ d }: DeepProps) {
  const t = { service: d.service, kind: d.kind, name: d.name };
  const { p } = route.useSearch();
  const prefix = p ?? "";
  const navigate = useNavigate();
  const go = (to: string) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: d.service, kind: d.kind, name: d.name }, search: { tab: "objects", ...(to ? { p: to } : {}) } });
  const [find, setFind] = useState("");
  const [tokens, setTokens] = useState<string[]>([]);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [open, setOpen] = useState<string | null>(null);
  const [dialog, setDialog] = useState<"upload" | "folder" | "delete" | null>(null);
  const token = tokens.at(-1);
  const input = { Prefix: prefix + find, Delimiter: "/", MaxKeys: PAGE, ...(token ? { ContinuationToken: token } : {}) };
  const list = useOpQuery<Listing>(t, "ListObjectsV2", input, ["list", prefix, find, token ?? ""]);
  const rows = list.data ? rowsOf(list.data, prefix) : [];
  const versioned = (d.fields.find((f) => f.label === "Versioning")?.value ?? "") !== "" && d.fields.find((f) => f.label === "Versioning")?.value !== "Disabled";
  const toggle = (key: string, on: boolean) => setPicked((s) => { const n = new Set(s); if (on) n.add(key); else n.delete(key); return n; });
  const chosen = rows.filter((r) => picked.has(r.key));
  const one = chosen.length === 1 && !chosen[0]?.folder ? chosen[0] : null;
  const columns: DataColumn<Row>[] = [
    { id: "pick", header: "", size: 44, enableSorting: false, cell: (c) => <Checkbox aria-label={`Select ${c.row.original.key}`} checked={picked.has(c.row.original.key)} onClick={(e) => e.stopPropagation()} onCheckedChange={(on) => toggle(c.row.original.key, on === true)} /> },
    {
      id: "name",
      header: "Name",
      accessorFn: (r) => `${r.folder ? 0 : 1}${r.key}`,
      cell: (c) => {
        const r = c.row.original;
        const Icon = r.folder ? FolderIcon : FileIcon;
        return (
          <span className="flex min-w-0 items-center gap-2">
            <Icon className={cn("size-4 flex-none", r.folder ? "text-link" : "text-muted-foreground")} aria-hidden />
            <span className="truncate font-mono text-[13px] text-link">{r.key.slice(prefix.length) || r.key}</span>
          </span>
        );
      },
    },
    { id: "modified", header: "Last modified", size: 200, accessorFn: (r) => r.modified ?? "", cell: (c) => (at(c.row.original.modified)) },
    { id: "size", header: "Size", size: 110, meta: { numeric: true }, accessorFn: (r) => r.size ?? -1, cell: (c) => (c.row.original.folder ? "–" : bytes(c.row.original.size ?? 0)) },
    { id: "class", header: "Storage class", size: 150, accessorFn: (r) => r.storage ?? "", cell: (c) => (c.row.original.folder ? "–" : (c.row.original.storage ?? "STANDARD").replace(/_/g, " ").toLowerCase()) },
  ];
  const download = async (key: string) => {
    try {
      await downloadObject(d.name, key);
    } catch (e) {
      toast.error("The download failed", { description: errorWords(e).message });
    }
  };
  const reset = () => (setTokens([]), setPicked(new Set()));
  return (
    <div className="grid gap-6">
      <Card
        title={
          <nav aria-label="Folder" className="flex flex-wrap items-center gap-1.5 text-[17px]" data-testid="object-crumbs">
            <Link to="/resources/$service/$kind/$name" params={{ service: d.service, kind: d.kind, name: d.name }} search={{ tab: "objects" }} onClick={reset} className={cn("font-mono hover:underline", prefix ? "text-link" : "text-bright")}>
              {d.name}
            </Link>
            {crumbsOf(prefix).map((c) => (
              <span key={c.prefix} className="flex items-center gap-1.5">
                <ChevronRightIcon className="size-4 text-muted-foreground" aria-hidden />
                <Link to="/resources/$service/$kind/$name" params={{ service: d.service, kind: d.kind, name: d.name }} search={{ tab: "objects", p: c.prefix }} onClick={reset} className={cn("font-mono hover:underline", c.prefix === prefix ? "text-bright" : "text-link")}>
                  {c.label}
                </Link>
              </span>
            ))}
          </nav>
        }
        lede={<>A folder is a shared key prefix: <span className="font-mono">banners/hero.jpg</span> is in the folder <span className="font-mono">banners/</span>. Read with <span className="font-mono">ListObjectsV2</span>, {PAGE} at a time.</>}
        actions={
          <>
            <Button variant="outline" size="sm" className="bg-card" disabled={!one} onClick={() => one && void download(one.key)} data-testid="download-picked">
              <DownloadIcon /> Download
            </Button>
            <Button variant="outline" size="sm" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={chosen.length === 0} onClick={() => setDialog("delete")} data-testid="delete-objects-open">
              <Trash2Icon /> Delete
            </Button>
            <Button variant="outline" size="sm" className="bg-card" onClick={() => setDialog("folder")} data-testid="folder-open">
              <FolderPlusIcon /> Create folder
            </Button>
            <Button size="sm" onClick={() => setDialog("upload")} data-testid="upload-open">
              <UploadIcon /> Upload
            </Button>
          </>
        }
        testId="objects-card"
      >
        <div className="grid gap-4">
          <div className="flex flex-wrap items-center gap-3">
            <div className="relative w-full max-w-sm">
              <SearchIcon className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" aria-hidden />
              <Input aria-label="Find objects by prefix" value={find} onChange={(e) => (setFind(e.target.value), reset())} placeholder={`Find in ${prefix || "this bucket"} by prefix`} className="bg-card pl-9 font-mono text-[13px]" data-testid="object-find" />
            </div>
            <span className="flex-1" />
            <span className="text-[13px] text-muted-foreground">{chosen.length > 0 ? `${chosen.length} selected` : list.data ? `${rows.length} shown${list.data.IsTruncated ? ", more on the next page" : ""}` : ""}</span>
            <Button variant="outline" size="sm" className="bg-card" onClick={() => void list.refetch()} disabled={list.isFetching} aria-label="Refresh">
              <RefreshCwIcon className={cn(list.isFetching && "animate-spin")} />
            </Button>
          </div>
          <Refused operation="ListObjectsV2" error={list.error} />
          <DataTable aria-label="Objects" data={rows} columns={columns} rowId={(r) => r.key} onRowClick={(r) => (r.folder ? (go(r.key), reset(), setFind("")) : setOpen(r.key))} maxHeight={560} empty={list.isPending ? "Reading…" : find ? `No key in ${prefix || "this bucket"} begins with ${find}.` : prefix ? "This folder is empty." : "This bucket holds no object yet. Upload one, or put one with the AWS CLI."} />
          <div className="flex items-center justify-end gap-2 text-[13px] text-muted-foreground">
            <span>Page {tokens.length + 1}</span>
            <Button variant="outline" size="sm" className="bg-card" aria-label="Previous page" disabled={tokens.length === 0} onClick={() => (setTokens((s) => s.slice(0, -1)), setPicked(new Set()))} data-testid="objects-prev">
              <ChevronLeftIcon />
            </Button>
            <Button variant="outline" size="sm" className="bg-card" aria-label="Next page" disabled={!list.data?.IsTruncated} onClick={() => list.data?.NextContinuationToken && (setTokens((s) => [...s, list.data.NextContinuationToken as string]), setPicked(new Set()))} data-testid="objects-next">
              <ChevronRightIcon />
            </Button>
          </div>
        </div>
      </Card>
      <ObjectSheet bucket={d.name} t={t} objectKey={open} versioned={versioned} onClose={() => setOpen(null)} onDownload={(k) => void download(k)} onDeleted={() => (setOpen(null), reset())} />
      <Upload open={dialog === "upload"} onOpenChange={(o) => setDialog(o ? "upload" : null)} bucket={d.name} prefix={prefix} t={t} />
      <NewFolder open={dialog === "folder"} onOpenChange={(o) => setDialog(o ? "folder" : null)} bucket={d.name} prefix={prefix} t={t} />
      <DeleteObjects open={dialog === "delete"} onOpenChange={(o) => setDialog(o ? "delete" : null)} rows={chosen} t={t} onDone={reset} />
    </div>
  );
}

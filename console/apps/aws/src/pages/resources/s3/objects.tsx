// A bucket's Objects tab: its folders (shared key prefixes) and objects one level at a time, found by prefix, listed as versions
// on request, and acted on (download, delete, copy, a presigned link) from here. ListObjectsV2 or ListObjectVersions with a
// delimiter of `/`, as the caller `console`. An upload, a new folder, a delete and an object's own page are pages of their own.
import { ChevronLeftIcon, ChevronRightIcon, DownloadIcon, FileIcon, FolderIcon, FolderPlusIcon, SearchIcon, Trash2Icon, UploadIcon } from "lucide-react";
import { useState } from "react";
import { Link } from "@tanstack/react-router";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { type DataColumn } from "@doze/ui/components/app/data-table";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { Badge } from "@doze/ui/components/ui/badge";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Switch } from "@doze/ui/components/ui/switch";
import { cn } from "@doze/ui/lib/utils";
import { downloadObject, useOpQuery } from "../deep/api";
import { Card } from "../deep/parts";
import { bucketOf, type Search, useBucketNav } from "./api";
import { chosenRows, crumbsOf, hiddenKeys, hiddenNote, type Listing, oneObject, type Row, rowId, rowsOf, selToText, type Versions, versionRowsOf } from "./listing";
import { ActionsMenu } from "./objects-menu";
import { shortDate, size, storageWords } from "./words";

const PAGE = 100;

export function ObjectsTab({ name, search }: { name: string; search: Search }) {
  const t = bucketOf(name);
  const go = useBucketNav(name);
  const prefix = search.p ?? "";
  const versions = search.versions === "yes";
  const [find, setFind] = useState("");
  const [applied, setApplied] = useState("");
  const [pages, setPages] = useState<{ token: string; version?: string }[]>([]);
  const [picked, setPicked] = useState<ReadonlySet<string>>(new Set());
  const [error, setError] = useState<unknown>(null);
  const at = pages.at(-1);
  const base = { Prefix: prefix + applied, Delimiter: "/", MaxKeys: PAGE };
  const key = ["list", prefix, applied, at?.token ?? "", at?.version ?? ""];
  const plain = useOpQuery<Listing>(t, "ListObjectsV2", { ...base, ...(at ? { ContinuationToken: at.token } : {}) }, key, !versions);
  const vers = useOpQuery<Versions>(t, "ListObjectVersions", { ...base, ...(at ? { KeyMarker: at.token, VersionIdMarker: at.version } : {}) }, [...key, "versions"], versions);
  const q = versions ? vers : plain;
  const data = versions ? vers.data : plain.data;
  const rows = data ? (versions ? versionRowsOf(vers.data!, base.Prefix) : rowsOf(plain.data!, base.Prefix)) : [];
  const next = versions ? (vers.data?.IsTruncated ? { token: vers.data.NextKeyMarker ?? "", version: vers.data.NextVersionIdMarker } : null) : plain.data?.IsTruncated && plain.data.NextContinuationToken ? { token: plain.data.NextContinuationToken } : null;
  const chosen = chosenRows(rows, picked);
  const one = oneObject(chosen);
  const reset = () => (setPages([]), setPicked(new Set()));
  const here = (p: Partial<Search>) => go({ tab: "objects", ...(prefix ? { p: prefix } : {}), ...(versions ? { versions: "yes" as const } : {}), ...p });
  const open = (r: Row) => (r.folder ? (reset(), setFind(""), setApplied(""), go({ tab: "objects", p: r.key, ...(versions ? { versions: "yes" as const } : {}) })) : go({ do: "object", key: r.key, ...(r.versionId ? { ver: r.versionId } : {}) }));
  const columns: DataColumn<Row>[] = [
    {
      id: "name",
      header: "Name",
      accessorFn: (r) => `${r.folder ? 0 : 1}${r.key}`,
      min: 200,
      cell: (c) => {
        const r = c.row.original;
        const Icon = r.folder ? FolderIcon : FileIcon;
        return (
          <span className="flex min-w-0 items-center gap-2">
            <Icon className={cn("size-4 flex-none", r.folder ? "text-link" : "text-muted-foreground")} aria-hidden />
            <button type="button" className="truncate rounded-sm text-left font-mono text-[13px] text-link outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring/50" onClick={() => open(r)} data-testid="object-open">
              {r.key.slice(base.Prefix.length - applied.length) || r.key}
            </button>
            {r.marker && <Badge variant="outline" className="border-warn/40 bg-warn/10 font-normal text-warn">Delete marker</Badge>}
          </span>
        );
      },
    },
    ...(versions ? [{ id: "version", header: "Version ID", size: 330, min: 300, accessorFn: (r: Row) => r.versionId ?? "", meta: { mono: true }, cell: (c: { row: { original: Row } }) => c.row.original.versionId ?? "-" } satisfies DataColumn<Row>] : []),
    { id: "modified", header: "Last modified", size: 260, min: 200, accessorFn: (r) => r.modified ?? "", cell: (c) => (c.row.original.folder ? "-" : shortDate(c.row.original.modified)) },
    { id: "size", header: "Size", size: 110, meta: { numeric: true }, accessorFn: (r) => r.size ?? -1, cell: (c) => (c.row.original.folder || c.row.original.marker ? "-" : size(c.row.original.size)) },
    { id: "class", header: "Storage class", size: 140, min: 110, accessorFn: (r) => r.storage ?? "", cell: (c) => (c.row.original.folder || c.row.original.marker ? "-" : storageWords(c.row.original.storage)) },
  ];
  const download = async (k: string) => {
    try {
      await downloadObject(name, k);
    } catch (e) {
      setError(e);
    }
  };
  const failed = error ? errorWords(error) : null;
  const empty = (
    <div className="grid justify-items-center gap-2 px-6 py-8 text-center" data-testid="objects-none">
      <h3 className="text-[16px] font-semibold text-bright">No objects</h3>
      <p className="max-w-[34rem] text-[13.5px] text-muted-foreground">
        {applied ? <>{versions ? "ListObjectVersions" : "ListObjectsV2"} with the prefix <b className="font-mono text-foreground">{base.Prefix}</b> returned no keys from {name}. Upload an object with that prefix, or create a folder of that name to start the prefix.</> : prefix ? "This folder is empty." : `${name} holds no object yet. Upload one, or put one with the AWS CLI.`}
      </p>
      <Button onClick={() => go({ do: "upload", ...(prefix ? { p: prefix } : {}) })} data-testid="empty-upload"><UploadIcon /> Upload</Button>
    </div>
  );
  return (
    <div className="grid gap-6" data-testid="objects-tab">
      <Card
        title={`Objects (${rows.length})`}
        lede={applied ? <>Showing keys that begin with <span className="font-mono">{base.Prefix}</span>.</> : versions ? "With Show versions on, every version of a key is a row, and a delete marker is a row too." : <>A folder is a shared key prefix: the object <span className="font-mono">banners/hero.jpg</span> appears inside the folder <span className="font-mono">banners/</span>.</>}
        testId="objects-card"
      >
        {prefix && (
          <nav aria-label="Folder" className="mb-4 flex flex-wrap items-center gap-1.5 text-[13.5px]" data-testid="object-crumbs">
            <Link to="/resources/$service/$kind/$name" params={{ service: "s3", kind: "buckets", name }} search={{ tab: "objects", ...(versions ? { versions: "yes" as const } : {}) }} className="font-mono text-link hover:underline">{name}</Link>
            {crumbsOf(prefix).map((c) => (
              <span key={c.prefix} className="flex items-center gap-1.5">
                <ChevronRightIcon className="size-4 text-muted-foreground" aria-hidden />
                <Link to="/resources/$service/$kind/$name" params={{ service: "s3", kind: "buckets", name }} search={{ tab: "objects", p: c.prefix, ...(versions ? { versions: "yes" as const } : {}) }} className={cn("font-mono hover:underline", c.prefix === prefix ? "text-bright" : "text-link")}>{c.label}</Link>
              </span>
            ))}
          </nav>
        )}
        <div className="mb-4 flex flex-wrap items-center gap-2">
          <Button variant="outline" className="bg-card" disabled={!one} onClick={() => one && void download(one.key)} data-testid="download-picked"><DownloadIcon /> Download</Button>
          <Button variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={chosen.length === 0 || (versions && chosen.some((r) => r.folder))} onClick={() => go({ do: versions ? "object-delete-version" : "object-delete", sel: selToText(chosen), ...(prefix ? { p: prefix } : {}) })} data-testid="delete-objects-open"><Trash2Icon /> Delete</Button>
          <ActionsMenu t={t} rows={chosen} one={one} go={(action, mode) => go({ do: action, ...(one ? { key: one.key } : {}), ...(mode ? { mode } : {}), ...(action === "presign" ? { do: "object", dialog: "presign" } : {}) })} />
          <Button variant="outline" className="bg-card" onClick={() => go({ do: "folder-create", ...(prefix ? { p: prefix } : {}) })} data-testid="folder-open"><FolderPlusIcon /> Create folder</Button>
          <Button onClick={() => go({ do: "upload", ...(prefix ? { p: prefix } : {}) })} data-testid="upload-open"><UploadIcon /> Upload</Button>
        </div>
        {failed && <Alert tone="bad" title="The listing was refused" className="mb-4">{failed.message}</Alert>}
        {q.error ? (
          <Alert tone="bad" title={`${versions ? "ListObjectVersions" : "ListObjectsV2"} was refused`} testId="op-error">{errorWords(q.error).message}</Alert>
        ) : (
          <SelectableTable<Row> aria-label="Objects" data={rows} columns={columns} rowId={rowId} selected={picked} onSelectedChange={setPicked} noun="object" maxHeight={560} toolbar={
          <form className="flex flex-wrap items-center gap-3" onSubmit={(e) => (e.preventDefault(), reset(), setApplied(find))}>
            <div className="relative w-full max-w-sm">
              <SearchIcon className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" aria-hidden />
              <Input aria-label="Find objects by prefix" value={find} onChange={(e) => setFind(e.target.value)} placeholder="Find objects by prefix" className="bg-card pl-9 font-mono text-[13px]" data-testid="object-find" />
            </div>
            <Button type="submit" variant="outline" className="bg-card" data-testid="object-search">Search</Button>
            <Label className="ml-2 flex items-center gap-2.5 font-normal text-foreground">
              <Switch checked={versions} onCheckedChange={(on) => (reset(), here({ versions: on ? "yes" : undefined }))} data-testid="show-versions" /> Show versions
            </Label>
          </form>
            } empty={q.isPending ? "Reading…" : empty} />
        )}
        {versions && !q.error && hiddenNote(hiddenKeys(rows)) && <Alert tone="info" title={hiddenNote(hiddenKeys(rows)) ?? ""} className="mt-4" testId="hidden-by-marker" />}
        <div className="mt-3 flex items-center justify-between gap-2 text-[13px] text-muted-foreground">
          <span data-testid="page-line">{rows.length === 0 ? "0 objects" : `1 to ${rows.length} of ${rows.length}${next ? "+" : ""} ${rows.length === 1 ? "object" : "objects"}`}{pages.length > 0 ? ` · page ${pages.length + 1}` : ""}</span>
          <span className="flex gap-2">
            <Button variant="outline" size="icon-sm" className="bg-card" aria-label="Previous page" disabled={pages.length === 0} onClick={() => (setPages((s) => s.slice(0, -1)), setPicked(new Set()))} data-testid="objects-prev"><ChevronLeftIcon /></Button>
            <Button variant="outline" size="icon-sm" className="bg-card" aria-label="Next page" disabled={!next} onClick={() => next && (setPages((s) => [...s, next]), setPicked(new Set()))} data-testid="objects-next"><ChevronRightIcon /></Button>
          </span>
        </div>
      </Card>
    </div>
  );
}

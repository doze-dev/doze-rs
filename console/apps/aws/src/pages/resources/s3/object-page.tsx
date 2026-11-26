// One object: its overview (what HeadObject says, its addresses, its tags and metadata) and its versions, with Download and the
// Object actions (a presigned link, copy, move, rename). The presigned link is a dialog over this page.
import { DownloadIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { copyText } from "@doze/ui/components/app/copy";
import { DataTable } from "@doze/ui/components/app/data-table";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { DropdownMenuItem } from "@doze/ui/components/ui/dropdown-menu";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Tabs, TabsList, TabsTrigger } from "@doze/ui/components/ui/tabs";
import { useMutation } from "@tanstack/react-query";
import { client } from "../../../api/client";
import { useAbout } from "../../../api/queries";
import type { Presigned } from "../../../api/types/Presigned";
import { downloadObject, useOpQuery } from "../deep/api";
import { Card } from "../deep/parts";
import type { OwnProps } from "../deep/registry";
import { RecentCalls } from "../../../kit/recent-calls";
import { bucketOf, crumbs, useBucket, useBucketNav, useSetting } from "./api";
import { notificationRules, SSE_WORDS, isSse } from "./config";
import { rowId, type Versions, versionRowsOf, type Row, selToText } from "./listing";
import { ActionsMenu } from "./objects-menu";
import { ActionPage, FlashBanner, Mono, Reading, Rows, useFlash } from "./parts";
import { baseName, bucketArn, clock, count, longDate, notifiedWords, s3Uri, shortDate, size, storageWords } from "./words";

type Head = { ContentType?: string; ContentLength?: number; ETag?: string; LastModified?: string | number; StorageClass?: string; VersionId?: string; CacheControl?: string; ContentEncoding?: string; ContentDisposition?: string; ContentLanguage?: string; ServerSideEncryption?: string; Metadata?: Record<string, string> };

const UNITS = { minutes: 60, hours: 3600 } as const;

export function PresignDialog({ name, objectKey, versionId, open, onClose }: { name: string; objectKey: string; versionId?: string; open: boolean; onClose: () => void }) {
  const about = useAbout();
  const [amount, setAmount] = useState("12");
  const [unit, setUnit] = useState<keyof typeof UNITS>("hours");
  const seconds = Number(amount) * UNITS[unit];
  const bad = !(seconds >= 1 && seconds <= 12 * 3600);
  const make = useMutation({
    mutationFn: () => client.api<Presigned>(`/api/v1/resources/s3/buckets/${encodeURIComponent(name)}/presign`, { method: "POST", body: JSON.stringify({ key: objectKey, expires: seconds, version_id: versionId ?? null }), headers: { "Content-Type": "application/json" } }),
    onSuccess: (p) => void copyText(p.url, "Presigned URL copied"),
  });
  const said = make.error ? errorWords(make.error) : null;
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-[600px]" data-testid="presign-dialog">
        <DialogHeader>
          <DialogTitle className="text-[21px] font-semibold">Share with a presigned URL</DialogTitle>
          <DialogDescription>Create a link to <Mono>{baseName(objectKey)}</Mono> that downloads the object without sign-in until it expires.</DialogDescription>
        </DialogHeader>
        <div className="grid gap-1.5">
          <Label htmlFor="presign-amount">How long the presigned URL stays valid</Label>
          <div className="flex gap-2">
            <Input id="presign-amount" value={amount} onChange={(e) => setAmount(e.target.value)} inputMode="numeric" className="w-28 font-mono" data-testid="presign-amount" />
            <select value={unit} onChange={(e) => setUnit(e.target.value as keyof typeof UNITS)} className="h-9 rounded-md border bg-card px-3 text-[14px]" aria-label="Unit" data-testid="presign-unit"><option value="minutes">minutes</option><option value="hours">hours</option></select>
          </div>
          <p className="text-[13px] text-muted-foreground">When you use the console, the maximum expiration time for a presigned URL is 12 hours from the time of creation. Expiry is enforced here; {about.data?.auth === "verify" ? "this doze-aws verifies signatures, and the console holds no access key it issued, so the link will be refused." : "the signature itself is not verified, because this doze-aws trusts every key."} When the URL is created it is copied to your clipboard, and a button copies it again.</p>
        </div>
        {said && <Alert tone="bad" title={`The link was not made: ${said.code?.split(" · ")[0]}`}>{said.message}</Alert>}
        {make.data && (
          <div className="grid gap-2" data-testid="presigned">
            <pre className="max-h-40 overflow-auto rounded-[var(--radius-field)] border bg-well p-3 font-mono text-[12px] break-all whitespace-pre-wrap" data-testid="presigned-url">{make.data.url}</pre>
            <div className="flex items-center gap-3"><CopyButton text={make.data.url} label="Copy URL" what="Presigned URL copied" /><span className="text-[13px] text-muted-foreground">Expires {longDate(make.data.expires_at)}.</span></div>
          </div>
        )}
        <DialogFooter>
          <Button variant="outline" className="bg-card" onClick={onClose}>Cancel</Button>
          <Button disabled={bad || make.isPending} onClick={() => make.mutate()} data-testid="presign-submit">{make.isPending ? "Creating…" : "Create presigned URL"}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function ObjectPage({ name, search }: OwnProps) {
  const key = search.key ?? "";
  const go = useBucketNav(name!);
  const [flash, dismiss] = useFlash();
  const t = bucketOf(name!);
  const bucket = useBucket(name!);
  const about = useAbout();
  const [tab, setTab] = useState<"overview" | "versions">(search.tab === "versions" ? "versions" : "overview");
  const [picked, setPicked] = useState<ReadonlySet<string>>(new Set());
  const [error, setError] = useState<unknown>(null);
  const head = useOpQuery<Head>(t, "HeadObject", { Key: key, ...(search.ver ? { VersionId: search.ver } : {}) }, ["head", key, search.ver ?? ""]);
  const tags = useOpQuery<{ TagSet?: { Key: string; Value: string }[] }>(t, "GetObjectTagging", { Key: key }, ["tags", key]);
  const vq = useOpQuery<Versions>(t, "ListObjectVersions", { Prefix: key, MaxKeys: 1000 }, ["versions", key]);
  const notes = useSetting(name!, "GetBucketNotificationConfiguration", []);
  const h = head.data;
  const versions = vq.data ? versionRowsOf(vq.data, "").filter((r) => r.key === key) : [];
  const notified = notificationRules(notes.data).filter((r) => r.events.some((e) => e.startsWith("s3:ObjectCreated:"))).map((r) => r.name);
  const objectUrl = `${about.data?.endpoint ?? ""}/${name}/${key.split("/").map(encodeURIComponent).join("/")}`;
  const sel = versions.filter((v) => picked.has(rowId(v)));
  const dl = async (versionId?: string) => downloadObject(name!, key, undefined, versionId).catch(setError);
  const here: Row = { key, folder: false, size: h?.ContentLength };
  const menu = (
    <ActionsMenu t={t} rows={h ? [here] : []} one={h ? here : null} label="Object actions" testId="object-actions" align="end"
      go={(action, mode) => go(action === "presign" ? { do: "object", key, dialog: "presign" } : { do: action, key, ...(mode ? { mode } : {}) })}
      extra={<DropdownMenuItem onSelect={() => go({ do: "object-delete", sel: selToText([{ key, folder: false }]) })} data-testid="action-delete">Delete</DropdownMenuItem>} />
  );
  const sentence = h ? `${baseName(key)} is ${size(h.ContentLength)}, last modified ${clock(h.LastModified)} UTC. ${versions.length > 1 ? `It has ${count(versions.length, "version")}${notified.length ? ` and ${notifiedWords(notified)}` : ""}.` : notified.length ? `${notifiedWords(notified)}.` : ""}`.trim() : null;
  const meta = (m: Record<string, string> | undefined) => [...(h?.ContentType ? [["System defined", "Content-Type", h.ContentType]] : []), ...(h?.CacheControl ? [["System defined", "Cache-Control", h.CacheControl]] : []), ...(h?.ContentDisposition ? [["System defined", "Content-Disposition", h.ContentDisposition]] : []), ...(h?.ContentEncoding ? [["System defined", "Content-Encoding", h.ContentEncoding]] : []), ...Object.entries(m ?? {}).map(([k, v]) => ["User defined", `x-amz-meta-${k}`, v])] as [string, string, string][];
  return (
    <ActionPage title={baseName(key)} mono crumbs={[...crumbs(name!), { label: baseName(key), to: `/resources/s3/buckets/${encodeURIComponent(name!)}` }, ...(tab === "versions" ? [{ label: `${baseName(key)}: Versions`, to: `/resources/s3/buckets/${encodeURIComponent(name!)}` }] : [])]} testId="object-page" banner={flash ? <FlashBanner flash={flash} dismiss={dismiss} /> : undefined}
      actions={<><Button variant="outline" className="bg-card" onClick={() => void dl()} data-testid="object-download"><DownloadIcon /> Download</Button>{menu}</>}
      sub={sentence ? <span data-testid="object-sentence">{sentence}</span> : undefined}>
      {error ? <Alert tone="bad" title="The download failed">{errorWords(error).message}</Alert> : null}
      <Tabs value={tab} onValueChange={(v) => setTab(v as "overview" | "versions")} className="gap-0">
        <TabsList variant="line" className="h-11 w-full justify-start gap-2 rounded-none border-b p-0" aria-label="Object pages">
          <TabsTrigger value="overview" className="h-11 flex-none px-3 text-[14px]" data-testid="tab-overview">Object overview</TabsTrigger>
          <TabsTrigger value="versions" className="h-11 flex-none px-3 text-[14px]" data-testid="tab-versions">Versions <span className="rounded-full bg-muted px-1.5 text-[11.5px] text-muted-foreground tabular-nums">{versions.length}</span></TabsTrigger>
        </TabsList>
      </Tabs>
      {tab === "overview" ? (
        <Reading q={head} what="The object">
          <Card title="Object overview" testId="overview">
            <Rows rows={[["Key", <Mono key="k">{key}</Mono>], ["Last modified", longDate(h?.LastModified)], ["Size", size(h?.ContentLength)], ["ETag", <Mono key="e">{(h?.ETag ?? "").replace(/"/g, "")}</Mono>], ["S3 URI", <span key="u" className="flex items-center gap-2"><Mono>{s3Uri(name!, key)}</Mono><CopyButton text={s3Uri(name!, key)} what="S3 URI copied" iconOnly variant="ghost" size="icon-sm" /></span>], ["Amazon Resource Name (ARN)", <Mono key="a">{`${bucketArn(name!)}/${key}`}</Mono>], ["Object URL", <Mono key="o">{objectUrl}</Mono>]]} />
          </Card>
          <Card title="Properties" className="mt-6">
            <Rows rows={[["Storage class", storageWords(h?.StorageClass)], ["Server-side encryption settings", h?.ServerSideEncryption && isSse(h.ServerSideEncryption) ? SSE_WORDS[h.ServerSideEncryption] : (h?.ServerSideEncryption ?? "None")]]} />
          </Card>
          <div className="mt-6 grid gap-6 lg:grid-cols-2" data-testid="tags-and-metadata">
            <Card title="Tags" testId="object-tags">
              {(tags.data?.TagSet ?? []).length > 0 ? <DataTable aria-label="Tags" data={tags.data!.TagSet!} rowId={(x) => x.Key} maxHeight={240} columns={[{ id: "k", header: "Key", accessorKey: "Key", size: 180, meta: { mono: true } }, { id: "v", header: "Value", accessorKey: "Value", meta: { mono: true } }]} /> : <p className="text-[13.5px] text-muted-foreground" data-testid="object-tags-none">{tags.isPending ? "Reading…" : "This object has no tags."}</p>}
            </Card>
            <Card title="Metadata" testId="object-metadata">
              <DataTable aria-label="Metadata" data={meta(h?.Metadata)} rowId={(m) => m[1]} maxHeight={280} empty="No metadata." columns={[{ id: "t", header: "Type", size: 145, accessorFn: (m) => m[0] }, { id: "k", header: "Key", size: 175, accessorFn: (m) => m[1], meta: { mono: true } }, { id: "v", header: "Value", accessorFn: (m) => m[2], meta: { mono: true } }]} />
            </Card>
          </div>
          {bucket.detail && <RecentCalls calls={bucket.detail.calls.filter((c) => c.resource === `${name}/${key}`)} noun="object" scope="this object" className="mt-6" />}
        </Reading>
      ) : (
        <Reading q={vq} what="The versions">
          <Card title={`Versions (${versions.length})`} lede="Each time the key is written, S3 stores another version with its own version ID." testId="versions">
            <SelectableTable<Row> aria-label="Versions" data={versions} rowId={rowId} selected={picked} onSelectedChange={setPicked} noun="version" maxHeight={420}
              toolbar={<>
                <Button variant="outline" className="bg-card" disabled={sel.length !== 1 || Boolean(sel[0]?.marker)} onClick={() => void dl(sel[0]!.versionId)} data-testid="version-download"><DownloadIcon /> Download</Button>
                <Button variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={sel.length === 0} onClick={() => go({ do: "object-delete-version", key, sel: selToText(sel) })} data-testid="version-delete"><Trash2Icon /> Delete</Button>
              </>}
              columns={[{ id: "v", header: "Version ID", accessorFn: (r) => r.versionId ?? "", meta: { mono: true, wrap: true }, min: 220, cell: (c) => <span className="font-mono text-[13px]">{c.row.original.versionId}{c.row.original.marker ? " (delete marker)" : c.row.original.latest ? " (current)" : ""}</span> }, { id: "m", header: "Last modified", size: 300, accessorFn: (r) => r.modified ?? "", cell: (c) => shortDate(c.row.original.modified) }, { id: "s", header: "Size", size: 110, meta: { numeric: true }, accessorFn: (r) => r.size ?? -1, cell: (c) => (c.row.original.marker ? "-" : size(c.row.original.size)) }, { id: "c", header: "Storage class", size: 140, accessorFn: (r) => r.storage ?? "", cell: (c) => (c.row.original.marker ? "-" : storageWords(c.row.original.storage)) }]} />
            <p className="mt-4 text-[13px] text-muted-foreground">A delete with no version ID never removes data from a versioned bucket: S3 adds a delete marker that becomes the current version. Only a delete that names a version ID removes that version for good.</p>
          </Card>
        </Reading>
      )}
      <PresignDialog name={name!} objectKey={key} versionId={search.ver} open={search.dialog === "presign"} onClose={() => go({ do: "object", key, ...(search.ver ? { ver: search.ver } : {}) })} />
    </ActionPage>
  );
}

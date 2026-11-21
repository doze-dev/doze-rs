// One object, in a drawer over the bucket's objects: what HeadObject says of it (its metadata), its tags (GetObjectTagging),
// and when the bucket keeps versions, its versions (ListObjectVersions); download and delete.
import { DownloadIcon, Trash2Icon } from "lucide-react";
import { toast } from "sonner";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { Button } from "@doze/ui/components/ui/button";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@doze/ui/components/ui/sheet";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { bytes } from "@doze/ui/lib/format";
import { timeMs } from "../model";
import { type Target, useOp, useOpQuery } from "../deep/api";
import { at, Confirm, Refused, useOpen } from "../deep/parts";

type Head = { ContentType?: string; ContentLength?: number; ETag?: string; LastModified?: string | number; StorageClass?: string; VersionId?: string; CacheControl?: string; ContentEncoding?: string; ServerSideEncryption?: string; Metadata?: Record<string, string> };
type Version = { Key: string; VersionId: string; IsLatest?: boolean; LastModified?: string | number; Size?: number };
type Versions = { Versions?: Version[]; DeleteMarkers?: { Key: string; VersionId: string; IsLatest?: boolean; LastModified?: string | number }[] };

function Rows({ rows, testId }: { rows: [string, React.ReactNode][]; testId?: string }) {
  return (
    <dl className="grid grid-cols-[minmax(8rem,11rem)_minmax(0,1fr)] text-[13.5px]" data-testid={testId}>
      {rows.map(([k, v]) => (
        <div key={k} className="contents">
          <dt className="border-b py-2 text-muted-foreground">{k}</dt>
          <dd className="min-w-0 border-b py-2 break-all text-bright">{v ?? <span className="text-muted-foreground">–</span>}</dd>
        </div>
      ))}
    </dl>
  );
}

function Body({ t, objectKey, versioned, onDownload, onDelete }: { t: Target; objectKey: string; versioned: boolean; onDownload: () => void; onDelete: () => void }) {
  const head = useOpQuery<Head>(t, "HeadObject", { Key: objectKey }, ["head", objectKey]);
  const tags = useOpQuery<{ TagSet?: { Key: string; Value: string }[] }>(t, "GetObjectTagging", { Key: objectKey }, ["tags", objectKey]);
  const versions = useOpQuery<Versions>(t, "ListObjectVersions", { Prefix: objectKey, MaxKeys: 100 }, ["versions", objectKey], versioned);
  const h = head.data;
  const mono = (v?: string) => (v ? <span className="font-mono text-[13px]">{v}</span> : undefined);
  const all = [...(versions.data?.Versions ?? []).map((v) => ({ ...v, marker: false })), ...(versions.data?.DeleteMarkers ?? []).map((v) => ({ ...v, Size: undefined, marker: true }))].filter((v) => v.Key === objectKey).sort((a, b) => (timeMs(String(b.LastModified ?? "")) ?? 0) - (timeMs(String(a.LastModified ?? "")) ?? 0));
  return (
    <div className="grid gap-6 px-7 py-6">
      <div className="flex flex-wrap gap-2">
        <Button variant="outline" className="bg-card" onClick={onDownload} data-testid="object-download">
          <DownloadIcon /> Download
        </Button>
        <CopyButton text={`s3://${t.name}/${objectKey}`} what="S3 URI copied" className="bg-card" />
        <span className="flex-1" />
        <Button variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" onClick={onDelete} data-testid="object-delete">
          <Trash2Icon /> Delete
        </Button>
      </div>
      <section className="grid gap-2">
        <h3 className="text-[13.5px] text-muted-foreground">Object (HeadObject)</h3>
        <Refused operation="HeadObject" error={head.error} />
        {!h && !head.error ? (
          <Skeleton className="h-40" />
        ) : h ? (
          <Rows
            testId="object-head"
            rows={[
              ["Size", h.ContentLength !== undefined ? bytes(h.ContentLength) : undefined],
              ["Type", mono(h.ContentType)],
              ["Last modified", h.LastModified ? at(h.LastModified) : undefined],
              ["ETag", mono(h.ETag)],
              ["Storage class", h.StorageClass ?? "STANDARD"],
              ["Encryption", mono(h.ServerSideEncryption)],
              ["Cache-Control", mono(h.CacheControl)],
              ["Content-Encoding", mono(h.ContentEncoding)],
              ["Version ID", mono(h.VersionId)],
            ]}
          />
        ) : null}
      </section>
      <section className="grid gap-2">
        <h3 className="text-[13.5px] text-muted-foreground">User metadata</h3>
        {h && Object.keys(h.Metadata ?? {}).length > 0 ? <Rows rows={Object.entries(h.Metadata ?? {}).map(([k, v]) => [`x-amz-meta-${k}`, mono(v)])} /> : <p className="text-[13.5px] text-muted-foreground">None.</p>}
      </section>
      <section className="grid gap-2">
        <h3 className="text-[13.5px] text-muted-foreground">Tags (GetObjectTagging)</h3>
        <Refused operation="GetObjectTagging" error={tags.error} />
        {(tags.data?.TagSet ?? []).length > 0 ? <Rows testId="object-tags" rows={(tags.data?.TagSet ?? []).map((x) => [x.Key, mono(x.Value)])} /> : <p className="text-[13.5px] text-muted-foreground" data-testid="object-tags-none">{tags.isPending ? "Reading…" : "None."}</p>}
      </section>
      <section className="grid gap-2">
        <h3 className="text-[13.5px] text-muted-foreground">Versions{versioned ? " (ListObjectVersions)" : ""}</h3>
        {!versioned ? (
          <p className="text-[13.5px] text-muted-foreground">Versioning is off for this bucket: an object has one version, and a put replaces it.</p>
        ) : (
          <>
            <Refused operation="ListObjectVersions" error={versions.error} />
            <Rows testId="object-versions" rows={all.map((v) => [v.IsLatest ? `Latest · ${at(v.LastModified)}` : at(v.LastModified), <span key={v.VersionId} className="font-mono text-[12.5px]">{v.marker ? "delete marker " : ""}{v.VersionId}{v.Size !== undefined ? ` · ${bytes(v.Size)}` : ""}</span>])} />
          </>
        )}
      </section>
    </div>
  );
}

export function ObjectSheet({ bucket, t, objectKey, versioned, onClose, onDownload, onDeleted }: { bucket: string; t: Target; objectKey: string | null; versioned: boolean; onClose: () => void; onDownload: (k: string) => void; onDeleted: () => void }) {
  const del = useOp(t, "DeleteObject");
  const [confirm, setConfirm] = useOpen(() => del.reset());
  return (
    <Sheet open={objectKey !== null} onOpenChange={(o) => !o && onClose()}>
      <SheetContent className="w-full gap-0 overflow-y-auto sm:max-w-[560px]" data-testid="object-sheet">
        {objectKey && (
          <>
            <SheetHeader className="border-b px-7 pt-7 pb-5">
              <SheetTitle className="font-mono text-[18px] font-semibold break-all">{objectKey.split("/").filter(Boolean).pop()}</SheetTitle>
              <SheetDescription className="font-mono text-[12.5px] break-all">
                s3://{bucket}/{objectKey}
              </SheetDescription>
            </SheetHeader>
            <Body t={t} objectKey={objectKey} versioned={versioned} onDownload={() => onDownload(objectKey)} onDelete={() => setConfirm(true)} />
            <Confirm
              open={confirm}
              onOpenChange={setConfirm}
              title="Delete object"
              verb="Delete"
              operation="DeleteObject"
              pending={del.isPending}
              error={del.error}
              run={() => del.mutate({ Key: objectKey }, { onSuccess: () => (setConfirm(false), toast.success(`${objectKey} deleted`), onDeleted()) })}
              testId="object-delete-dialog"
            >
              Delete <b className="font-mono font-semibold break-all text-bright">{objectKey}</b>? {versioned ? "The bucket keeps versions: S3 adds a delete marker, and the older versions stay." : "Versioning is off: the object is gone for good."}
            </Confirm>
          </>
        )}
      </SheetContent>
    </Sheet>
  );
}

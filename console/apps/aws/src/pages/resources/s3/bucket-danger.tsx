// Delete bucket, Empty bucket and the Status of an emptying: pages of their own, each asking for the bucket's name. S3 deletes
// only an empty bucket, so Delete says when it is not and sends the person to Empty; Empty removes every version and delete marker.
import { useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { useQuery } from "@tanstack/react-query";
import { BlockersList, useBlockers, useDeleteClearing } from "../../../kit";
import { callOp } from "../deep/api";
import type { OwnProps } from "../deep/registry";
import { bucketOf, crumbs, useBucket, useListNav } from "./api";
import { type Emptied, emptiedOf, setEmptied, setFlash } from "./flash";
import { ActionPage, Footer, Mono, Rows } from "./parts";
import { Card } from "../deep/parts";
import { count, size, versioningWords } from "./words";

type Page = { Versions?: { Key: string; VersionId?: string; Size?: number }[]; DeleteMarkers?: { Key: string; VersionId?: string }[]; IsTruncated?: boolean };
const CAP = 20;

/** How much a bucket holds: its objects (all versions), delete markers and bytes; `more` when it is more than was read. */
export function useHolding(name: string) {
  return useQuery({
    queryKey: ["s3", "holding", name],
    gcTime: 0,
    queryFn: async () => {
      const t = bucketOf(name);
      let objects = 0, markers = 0, bytes = 0, more = false;
      let km: string | undefined, vm: string | undefined;
      for (let i = 0; i < CAP; i++) {
        const p = await callOp<Page & { NextKeyMarker?: string; NextVersionIdMarker?: string }>(t, "ListObjectVersions", { MaxKeys: 1000, ...(km ? { KeyMarker: km, VersionIdMarker: vm } : {}) });
        objects += p.Versions?.length ?? 0;
        markers += p.DeleteMarkers?.length ?? 0;
        bytes += (p.Versions ?? []).reduce((a, v) => a + (v.Size ?? 0), 0);
        if (!p.IsTruncated) return { objects, markers, bytes, more };
        km = p.NextKeyMarker;
        vm = p.NextVersionIdMarker;
      }
      more = true;
      return { objects, markers, bytes, more };
    },
    retry: false,
  });
}

/** Removes every version and delete marker, a thousand at a time. */
export async function emptyBucket(name: string): Promise<Emptied> {
  const t = bucketOf(name);
  const out: Emptied = { deleted: 0, bytes: 0, failed: [] };
  for (;;) {
    const p = await callOp<Page>(t, "ListObjectVersions", { MaxKeys: 1000 });
    const all = [...(p.Versions ?? []), ...(p.DeleteMarkers ?? [])];
    if (all.length === 0) return out;
    const answer = await callOp<{ Errors?: { Key: string; Code: string; Message?: string }[] }>(t, "DeleteObjects", { Delete: { Objects: all.map((o) => ({ Key: o.Key, ...(o.VersionId ? { VersionId: o.VersionId } : {}) })), Quiet: true } });
    const errors = answer.Errors ?? [];
    out.failed.push(...errors.map((e) => ({ key: e.Key, code: e.Code, message: e.Message })));
    out.deleted += all.length - errors.length;
    out.bytes += (p.Versions ?? []).reduce((a, v) => a + (v.Size ?? 0), 0);
    if (errors.length === all.length) return out;
  }
}

function Confirm({ id, words, label, value, set }: { id: string; words: string; label: string; value: string; set: (v: string) => void }) {
  return (
    <div className="grid gap-1.5">
      <Label htmlFor={id}>{label}</Label>
      <Input id={id} value={value} onChange={(e) => set(e.target.value)} placeholder={words} autoComplete="off" spellCheck={false} className="max-w-md font-mono" data-testid="confirm-input" />
    </div>
  );
}

export function BucketDelete({ search }: OwnProps) {
  const name = search.name ?? "";
  const list = useListNav();
  const blockers = useBlockers(bucketOf(name));
  const del = useDeleteClearing(bucketOf(name));
  const [typed, setTyped] = useState("");
  const blocking = blockers.data?.blockers.filter((b) => b.blocks) ?? [];
  const said = del.error ? errorWords(del.error) : null;
  const run = () => del.mutate({ clear: false }, { onSuccess: () => (setFlash({ title: `Bucket ${name} deleted.`, returned: "DeleteBucket answered 204. Its name is free to use again.", left: "To confirm a deletion, search the list for the name you deleted. If S3 does not list it, the bucket is gone." }), list({})) });
  return (
    <ActionPage title="Delete bucket" sub={<>Delete the bucket <Mono>{name}</Mono>. A bucket must be empty before it can be deleted.</>} crumbs={[...crumbs(null), { label: "Delete bucket", to: "/resources/s3/buckets" }]} testId="bucket-delete-page"
      footer={<Footer onCancel={() => list({})}><Button variant="destructive" disabled={typed !== name || del.isPending} onClick={run} data-testid="delete-submit">{del.isPending ? "Deleting…" : "Delete bucket"}</Button></Footer>}>
      {blocking.length > 0 && (
        <Alert tone="warn" title="This bucket is not empty" testId="not-empty" action={<Button variant="outline" size="sm" className="bg-card" onClick={() => list({ do: "empty", name })} data-testid="go-empty">Empty bucket</Button>}>
          S3 deletes a bucket only when every object, version and delete marker is gone. Empty it first.
        </Alert>
      )}
      {said && <Alert tone="bad" title={`DeleteBucket was refused: ${said.code?.split(" · ")[0]}`} testId="delete-refused">{said.message}</Alert>}
      <Alert tone="warn" title="Deleting a bucket cannot be undone.">Bucket names are unique. If you delete your bucket, another AWS user can use the name. If you want to continue to use the same bucket name, don&apos;t delete your bucket. Instead, empty and keep the bucket.</Alert>
      <Card title="Delete bucket">
        <BlockersList data={blockers.data} loading={blockers.isPending} className="mb-4" />
        <Confirm id="confirm" words={name} label="To confirm deletion, enter the name of the bucket" value={typed} set={setTyped} />
        <p className="mt-3 text-[13px] text-muted-foreground">Every object, including all versions and delete markers, must be gone before S3 deletes a bucket. If any are left, this page shows a &quot;This bucket is not empty&quot; alert with an Empty bucket button.</p>
      </Card>
    </ActionPage>
  );
}

export function BucketEmpty({ search }: OwnProps) {
  const name = search.name ?? "";
  const list = useListNav();
  const bucket = useBucket(name);
  const holding = useHolding(name);
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const h = holding.data;
  const versioned = bucket.versioning !== undefined && bucket.versioning !== "Disabled";
  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      setEmptied(name, await emptyBucket(name));
      list({ do: "empty-status", name });
    } catch (e) {
      setError(e);
      setBusy(false);
    }
  };
  const said = error ? errorWords(error) : null;
  return (
    <ActionPage title="Empty bucket" sub={<>Permanently delete all objects in <Mono>{name}</Mono>. The bucket itself stays.</>} crumbs={[...crumbs(null), { label: "Empty bucket", to: "/resources/s3/buckets" }]} testId="bucket-empty-page"
      footer={<Footer onCancel={() => list({})}><Button variant="destructive" disabled={typed !== name || busy} onClick={() => void run()} data-testid="empty-submit">{busy ? "Emptying…" : "Empty"}</Button></Footer>}>
      <p className="text-[15px] text-foreground" data-testid="holding">
        {h ? <><Mono>{name}</Mono> holds <b>{count(h.objects, "object")}{h.more ? " or more" : ""}</b> ({size(h.bytes)}){h.markers > 0 ? ` and ${count(h.markers, "delete marker")}` : ""}. {versioned ? `Versioning is ${versioningWords(bucket.versioning!)}, so every version of every object and every delete marker is removed.` : "It has no versioning, so each object is removed once and nothing is kept as a version."}</> : "Counting what it holds…"}
      </p>
      {said && <Alert tone="bad" title={`Emptying stopped: ${said.code?.split(" · ")[0]}`}>{said.message}</Alert>}
      <Alert tone="warn" title="Emptying the bucket cannot be undone.">Objects added to the bucket while the empty bucket action is in progress will be deleted.</Alert>
      <Card title="Empty bucket">
        <Confirm id="confirm" words={name} label="To confirm that you want to empty the bucket, enter the bucket name" value={typed} set={setTyped} />
        <p className="mt-3 text-[13px] text-muted-foreground">In a bucket with versioning enabled or suspended, emptying deletes all versions of every object. {versioned ? `${name} has versioning ${versioningWords(bucket.versioning!)}.` : `${name} does not.`}</p>
        <p className="mt-2 text-[13px] text-muted-foreground">To empty a large bucket, a lifecycle rule that expires objects is the recommended way. Expiration is asynchronous, so the bucket can take some days to empty.</p>
      </Card>
    </ActionPage>
  );
}

export function EmptyStatus({ search }: OwnProps) {
  const name = search.name ?? "";
  const list = useListNav();
  const r = emptiedOf(name);
  return (
    <ActionPage title="Empty bucket: Status" sub={<>A summary of the successful and failed object deletions in <Mono>{name}</Mono>.</>} crumbs={[...crumbs(null), { label: "Empty bucket: Status", to: "/resources/s3/buckets" }]} actions={<Button onClick={() => list({})} data-testid="status-exit">Exit</Button>} testId="empty-status-page">
      <Card title="Summary">
        {r ? (
          <Rows rows={[["Successful deletions", <span key="s" data-testid="deleted">{count(r.deleted, "object")} <span className="ml-2 text-muted-foreground">{size(r.bytes)}</span></span>], ["Failed deletions", <span key="f" data-testid="failed">{count(r.failed.length, "object")}</span>], ...r.failed.slice(0, 5).map((f, i) => [`Failed ${i + 1}`, <span key={f.key}><Mono>{f.key}</Mono> {f.code}</span>] as [string, React.ReactNode])]} />
        ) : (
          <p className="text-[13.5px] text-muted-foreground">This page has no result to show: the emptying it reports was not made in this window.</p>
        )}
      </Card>
    </ActionPage>
  );
}

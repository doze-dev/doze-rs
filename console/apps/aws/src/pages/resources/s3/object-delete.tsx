// Delete objects, and Permanently delete object versions: the objects (or versions) named, what the delete does in this bucket's
// terms (a delete marker, or gone for good), the word to type, and then DeleteObjects. A folder is every object under it.
import { useQueries } from "@tanstack/react-query";
import { useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { DataTable } from "@doze/ui/components/app/data-table";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { callOp, type Target } from "../deep/api";
import { Card } from "../deep/parts";
import type { OwnProps } from "../deep/registry";
import { bucketOf, crumbs, useBucket, useBucketNav, useSetting, useStale } from "./api";
import { notificationRules } from "./config";
import { setFlash } from "./flash";
import { selFromText } from "./listing";
import { ActionPage, Footer, Mono } from "./parts";
import { baseName, count, PERMANENT, s3Uri, shortDate, size } from "./words";

type Head = { ContentLength?: number; LastModified?: string | number };
type Item = { key: string; versionId?: string };
type Listing = { Contents?: { Key: string }[]; Versions?: { Key: string; VersionId?: string; LastModified?: string | number; Size?: number }[]; DeleteMarkers?: { Key: string; VersionId?: string }[]; IsTruncated?: boolean; NextContinuationToken?: string };

/** Every object under a folder (the folder's own marker included), page by page. */
async function under(t: Target, prefix: string): Promise<string[]> {
  const keys: string[] = [];
  let token: string | undefined;
  do {
    const p: Listing = await callOp<Listing>(t, "ListObjectsV2", { Prefix: prefix, MaxKeys: 1000, ...(token ? { ContinuationToken: token } : {}) });
    keys.push(...(p.Contents ?? []).map((c) => c.Key));
    token = p.IsTruncated ? p.NextContinuationToken : undefined;
  } while (token);
  return keys;
}

export function ObjectDelete({ name, search, permanent }: OwnProps & { permanent?: boolean }) {
  const go = useBucketNav(name!);
  const stale = useStale(name!);
  const t = bucketOf(name!);
  const bucket = useBucket(name!);
  const notes = useSetting(name!, "GetBucketNotificationConfiguration", []);
  const items: Item[] = selFromText(search.sel);
  const versionMode = permanent === true;
  const heads = useQueries({ queries: items.slice(0, 50).map((i) => ({ queryKey: ["delete-head", name, i.key, i.versionId ?? ""], retry: false, enabled: !i.key.endsWith("/"), queryFn: () => callOp<Head>(t, "HeadObject", { Key: i.key, ...(i.versionId ? { VersionId: i.versionId } : {}) }) })) });
  const siblings = useSetting<Listing>(name!, "ListObjectVersions", [], { Prefix: items[0]?.key ?? "", MaxKeys: 1000 }, versionMode);
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const versioned = bucket.versioning !== undefined && bucket.versioning !== "Disabled";
  const hard = versionMode || !versioned;
  const word = hard ? PERMANENT : "delete";
  const removed = notificationRules(notes.data).filter((r) => r.events.some((e) => e.startsWith("s3:ObjectRemoved:")));
  const others = versionMode ? Math.max(0, (siblings.data?.Versions?.filter((v) => v.Key === items[0]?.key).length ?? 0) + (siblings.data?.DeleteMarkers?.filter((v) => v.Key === items[0]?.key).length ?? 0) - items.length) : 0;
  const back = () => go(search.p ? { tab: "objects", p: search.p } : versionMode && items[0] ? { do: "object", key: items[0].key, tab: "versions" } : {});
  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      const doomed: Item[] = [];
      for (const i of items) {
        if (i.key.endsWith("/") && !i.versionId) doomed.push(...(await under(t, i.key)).map((key) => ({ key })));
        else doomed.push(i);
      }
      let failed = 0;
      let first = "";
      for (let n = 0; n < doomed.length; n += 1000) {
        const a = await callOp<{ Errors?: { Key: string; Code: string }[] }>(t, "DeleteObjects", { Delete: { Objects: doomed.slice(n, n + 1000).map((d) => ({ Key: d.key, ...(d.versionId ? { VersionId: d.versionId } : {}) })), Quiet: true } });
        failed += a.Errors?.length ?? 0;
        first ||= a.Errors?.[0] ? `${a.Errors[0].Key} (${a.Errors[0].Code})` : "";
      }
      if (failed > 0) throw new Error(`${failed} of ${doomed.length} could not be deleted: ${first}.`);
      stale();
      setFlash({ title: hard ? `${count(doomed.length, "object")} permanently deleted.` : `${count(doomed.length, "object")} deleted.`, returned: hard ? "DeleteObjects named the key and the version ID." : "Each key now has a delete marker as its current version; the older versions stay." });
      back();
    } catch (e) {
      setError(e);
      setBusy(false);
    }
  };
  const said = error ? errorWords(error) : null;
  const bucketPath = `/resources/s3/buckets/${encodeURIComponent(name!)}`;
  const rows = items.map((i, n) => ({ ...i, head: heads[n]?.data, listed: siblings.data?.Versions?.find((v) => v.Key === i.key && v.VersionId === i.versionId) }));
  return (
    <ActionPage title={versionMode ? "Delete objects" : "Delete objects"} crumbs={[...crumbs(name!), ...(versionMode && items[0] ? [{ label: baseName(items[0].key), to: bucketPath }, { label: "Versions", to: bucketPath }] : []), { label: versionMode ? "Permanently delete object versions" : "Delete objects", to: bucketPath }]} testId={versionMode ? "object-delete-version-page" : "object-delete-page"}
      sub={versionMode ? <>Permanently delete {count(items.length, "version")} of <Mono>{baseName(items[0]?.key ?? "")}</Mono>.</> : <>Delete {count(items.length, "object")} from <Mono>{s3Uri(name!)}</Mono>.</>}
      footer={<Footer onCancel={back}><Button variant="destructive" disabled={typed !== word || busy || items.length === 0} onClick={() => void run()} data-testid="delete-objects-submit">{busy ? "Deleting…" : "Delete objects"}</Button></Footer>}>
      {versionMode ? (
        <Alert tone="warn" title="When you permanently delete an object version, the action cannot be undone.">{others > 0 ? `The other ${others === 1 ? "version stays" : `${others} versions stay`}.` : "No other version of the key is left."}</Alert>
      ) : versioned ? (
        <Alert tone="info" title="Versioning is on, so this does not remove the data.">A delete without a version ID makes S3 insert a delete marker that becomes the current version. A GET without a version ID then returns 404, and every older version stays until you delete it by version ID.</Alert>
      ) : (
        <Alert tone="warn" title="Versioning is off, so the objects are gone for good.">The data of each object is removed and cannot be recovered.</Alert>
      )}
      {said && <Alert tone="bad" title="Not deleted" testId="delete-refused">{said.message}</Alert>}
      <Card title="Specified objects">
        <DataTable aria-label="Specified objects" data={rows} rowId={(r) => `${r.key}|${r.versionId ?? ""}`} maxHeight={280} columns={[{ id: "n", header: "Name", accessorFn: (r) => r.key, meta: { mono: true }, min: 200 }, ...(versionMode ? [{ id: "v", header: "Version ID", size: 240, accessorFn: (r: (typeof rows)[number]) => r.versionId ?? "", meta: { mono: true } }] : []), { id: "m", header: "Last modified", size: 260, accessorFn: (r) => String(r.head?.LastModified ?? r.listed?.LastModified ?? ""), cell: (c) => (c.row.original.key.endsWith("/") ? "-" : shortDate(c.row.original.head?.LastModified ?? c.row.original.listed?.LastModified)) }, { id: "s", header: "Size", size: 110, meta: { numeric: true }, accessorFn: (r) => r.head?.ContentLength ?? r.listed?.Size ?? -1, cell: (c) => (c.row.original.key.endsWith("/") ? "-" : size(c.row.original.head?.ContentLength ?? c.row.original.listed?.Size)) }]} />
      </Card>
      <Card title={hard ? "Permanently delete objects?" : "Delete objects?"}>
        <div className="grid gap-1.5">
          <Label htmlFor="confirm">To confirm {hard ? "permanent " : ""}deletion of the {versionMode ? "versions" : "objects"} listed under Specified objects, enter <b>{word}</b></Label>
          <Input id="confirm" value={typed} onChange={(e) => setTyped(e.target.value)} placeholder={word} autoComplete="off" className="max-w-md font-mono" data-testid="confirm-input" />
        </div>
        <p className="mt-3 text-[13px] text-muted-foreground">
          {versionMode ? "The call names the key and the version ID, which is what makes it permanent. " : versioned ? "In a bucket without versioning, or when you delete a specific version, the confirmation word is permanently delete and the data is gone. This bucket is the first kind: delete only hides the object. " : ""}
          {removed.length > 0 ? `An ObjectRemoved notification rule exists on this bucket: ${removed.map((r) => r.name).join(", ")} is told of each delete.` : `No ObjectRemoved notification rule exists on this bucket, so nothing is told of the delete.`}
        </p>
      </Card>
    </ActionPage>
  );
}

export const ObjectDeleteVersion = (p: OwnProps) => <ObjectDelete {...p} permanent />;

// A bucket's listing, pure: the rows of ListObjectsV2 and ListObjectVersions (folders first), the crumbs of a prefix, and what
// is wrong with a folder's name.
export type Row = {
  key: string;
  folder: boolean;
  /** Set when the listing is of versions (an object has a row for each). */
  versionId?: string;
  marker?: boolean;
  latest?: boolean;
  size?: number;
  modified?: string | number;
  storage?: string;
};

type Item = { Key: string; Size?: number; LastModified?: string | number; StorageClass?: string; VersionId?: string; IsLatest?: boolean };
export type Listing = { Contents?: Item[]; CommonPrefixes?: { Prefix: string }[]; IsTruncated?: boolean; NextContinuationToken?: string; KeyCount?: number };
export type Versions = { Versions?: Item[]; DeleteMarkers?: Item[]; CommonPrefixes?: { Prefix: string }[]; IsTruncated?: boolean; NextKeyMarker?: string; NextVersionIdMarker?: string };

/** The id of a row: a key, and for a version its id too. */
export const rowId = (r: Row): string => (r.versionId === undefined ? r.key : `${r.key}\u0000${r.versionId}`);

/** The rows of a listing at `prefix`: its folders first, then its objects; the folder's own marker (`prefix` itself) left out. */
export function rowsOf(l: Listing, prefix: string): Row[] {
  const folders = (l.CommonPrefixes ?? []).map((p) => ({ key: p.Prefix, folder: true }));
  const objects = (l.Contents ?? []).filter((o) => o.Key !== prefix).map((o) => ({ key: o.Key, folder: false, size: o.Size, modified: o.LastModified, storage: o.StorageClass }));
  return [...folders, ...objects];
}

/** The rows of a versions listing: folders, then for each key its delete marker and versions, the newest first. */
export function versionRowsOf(l: Versions, prefix: string): Row[] {
  const folders = (l.CommonPrefixes ?? []).map((p) => ({ key: p.Prefix, folder: true }));
  const versions: Row[] = (l.Versions ?? []).filter((v) => v.Key !== prefix).map((v) => ({ key: v.Key, folder: false, versionId: v.VersionId ?? "null", latest: v.IsLatest, size: v.Size, modified: v.LastModified, storage: v.StorageClass }));
  const markers: Row[] = (l.DeleteMarkers ?? []).map((v) => ({ key: v.Key, folder: false, versionId: v.VersionId ?? "null", marker: true, latest: v.IsLatest, modified: v.LastModified }));
  const when = (r: Row) => (typeof r.modified === "number" ? r.modified : Date.parse(String(r.modified ?? "")) || 0);
  const objects = [...versions, ...markers].sort((a, b) => (a.key === b.key ? when(b) - when(a) : a.key < b.key ? -1 : 1));
  return [...folders, ...objects];
}

/** `a/b/` as the crumbs `a/` and `a/b/`, each with its label. */
export function crumbsOf(prefix: string): { label: string; prefix: string }[] {
  const parts = prefix.split("/").filter(Boolean);
  return parts.map((p, i) => ({ label: p, prefix: `${parts.slice(0, i + 1).join("/")}/` }));
}

/** What is wrong with a folder's name, if anything. */
export function folderWrong(name: string): string | null {
  if (!name.trim()) return null;
  if (name.includes("/")) return "A folder's name cannot hold a /: make one folder, then open it to make the next.";
  if (new TextEncoder().encode(name).length > 1000) return "Too long: a key is at most 1,024 bytes.";
  return null;
}

/** What `selected` ids are: the rows of `rows` that were chosen. */
export const chosenRows = (rows: Row[], selected: ReadonlySet<string>): Row[] => rows.filter((r) => selected.has(rowId(r)));

/** The one object (not a folder or a delete marker) a selection is, when it is exactly one. */
export function oneObject(rows: Row[]): Row | null {
  const [only] = rows;
  return rows.length === 1 && only && !only.folder && !only.marker ? only : null;
}

/** A selection as the address carries it (`?sel=`): the keys, and a version's id when the listing was of versions. */
export function selToText(rows: Row[]): string {
  return JSON.stringify(rows.map((r) => (r.versionId === undefined ? [r.key] : [r.key, r.versionId])));
}

/** What `?sel=` names: keys, each with its version id when it has one. */
export function selFromText(text: string | undefined): { key: string; versionId?: string }[] {
  if (!text) return [];
  try {
    const v: unknown = JSON.parse(text);
    if (!Array.isArray(v)) return [];
    return v.flatMap((e) => (Array.isArray(e) && typeof e[0] === "string" ? [{ key: e[0], ...(typeof e[1] === "string" ? { versionId: e[1] } : {}) }] : []));
  } catch {
    return [];
  }
}

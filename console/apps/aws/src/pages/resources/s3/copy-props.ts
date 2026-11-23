// What "Specify settings" of a copy sets, pure: the members of CopyObject that the page's choices become, and the settings a copy
// starts from (the source's own, so that choosing to specify changes only what the person changes).
import { uploadProps } from "./upload-props";

type Pair = { key: string; value: string };

export type CopyForm = {
  /** Whether the copy keeps the source's metadata (`COPY`) or takes the rows below in its place (`REPLACE`). */
  metadata: "copy" | "replace";
  metaRows: Pair[];
  storageClass: string;
  /** The source's class: a class is sent only when the person changed it. */
  sourceClass: string;
  /** A canned ACL, or "" for none (the destination's own ownership rules). */
  acl: string;
  tags: Pair[];
  encryption: "bucket" | "AES256" | "aws:kms";
  kmsKey: string;
  checksum: string;
};

/** The canned ACLs a copy may name, with the words under each. `public` ones grant to everyone or to every AWS account. */
export const ACLS: { value: string; title: string; public: boolean }[] = [
  { value: "private", title: "Private: only the owner has access", public: false },
  { value: "bucket-owner-read", title: "Bucket owner read", public: false },
  { value: "bucket-owner-full-control", title: "Bucket owner full control", public: false },
  { value: "authenticated-read", title: "Authenticated users read: any AWS account", public: true },
  { value: "public-read", title: "Public read: anyone", public: true },
];

/** The ACLs a destination takes, and why the others are not offered: with ownership enforced (ACLs disabled) only the owner's full
 * control is accepted; with Block public ACLs on, a public one is refused. */
export function aclChoices(enforced: boolean, blockPublicAcls: boolean): { value: string; title: string; why?: string }[] {
  return ACLS.map((a) => {
    if (enforced && a.value !== "bucket-owner-full-control") return { value: a.value, title: a.title, why: "ACLs are disabled on the destination (Bucket owner enforced)." };
    if (a.public && blockPublicAcls) return { value: a.value, title: a.title, why: "The destination blocks public ACLs." };
    return { value: a.value, title: a.title };
  });
}

const SYSTEM_HEADERS: [string, keyof Head][] = [["Content-Type", "ContentType"], ["Cache-Control", "CacheControl"], ["Content-Disposition", "ContentDisposition"], ["Content-Encoding", "ContentEncoding"], ["Content-Language", "ContentLanguage"]];
type Head = { ContentType?: string; CacheControl?: string; ContentDisposition?: string; ContentEncoding?: string; ContentLanguage?: string; Metadata?: Record<string, string>; StorageClass?: string };

/** The source's metadata as rows to edit: its system headers by name, then its own keys as `x-amz-meta-<key>`. */
export function sourceMetadata(h: Head | undefined): Pair[] {
  if (!h) return [];
  const system = SYSTEM_HEADERS.flatMap(([key, member]) => (h[member] ? [{ key, value: String(h[member]) }] : []));
  return [...system, ...Object.entries(h.Metadata ?? {}).map(([k, v]) => ({ key: `x-amz-meta-${k}`, value: v }))];
}

/** The members of CopyObject for "Specify settings". The metadata directive is the person's choice; the tags always replace (the
 * rows start as the source's, so a copy with no edit keeps them); the rest is sent only when it differs from the default. */
export function specifyProps(f: CopyForm): Record<string, unknown> {
  const replace = f.metadata === "replace";
  const typed = f.metaRows.find((r) => r.key.trim().toLowerCase() === "content-type" && r.value !== "");
  const shared = uploadProps({ storageClass: "STANDARD", encryption: f.encryption, kmsKey: f.kmsKey, checksum: f.checksum, tags: f.tags, metadata: replace ? f.metaRows.filter((r) => r.key.trim().toLowerCase() !== "content-type") : [] });
  return {
    MetadataDirective: replace ? "REPLACE" : "COPY",
    TaggingDirective: "REPLACE",
    ...shared,
    ...(replace && typed ? { ContentType: typed.value } : {}),
    ...(f.storageClass !== f.sourceClass ? { StorageClass: f.storageClass } : {}),
    ...(f.acl ? { ACL: f.acl } : {}),
  };
}

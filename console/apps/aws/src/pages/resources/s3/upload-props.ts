// What an upload sets beyond the file, pure: the properties of the board's Upload page as the members of PutObject that the
// console's upload address takes (`?props=`, crates/aws-console/src/api/objects.rs), and the result of each file kept for the
// Status page.
export type UploadForm = { storageClass: string; encryption: "bucket" | "AES256" | "aws:kms"; kmsKey: string; checksum: string; tags: { key: string; value: string }[]; metadata: { key: string; value: string }[] };

export const STORAGE_CLASSES = ["STANDARD", "INTELLIGENT_TIERING", "STANDARD_IA", "ONEZONE_IA", "GLACIER_IR", "GLACIER", "DEEP_ARCHIVE"];
export const CHECKSUMS = ["CRC32", "CRC32C", "CRC64NVME", "SHA1", "SHA256"];
const SYSTEM: Record<string, string> = { "cache-control": "CacheControl", "content-disposition": "ContentDisposition", "content-encoding": "ContentEncoding", "content-language": "ContentLanguage" };

/** The members `?props=` carries for this form: only what differs from the defaults, so what is left at its default is left out. */
export function uploadProps(f: UploadForm): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  if (f.storageClass !== "STANDARD") out.StorageClass = f.storageClass;
  if (f.encryption !== "bucket") {
    out.ServerSideEncryption = f.encryption;
    if (f.encryption === "aws:kms" && f.kmsKey) out.SSEKMSKeyId = f.kmsKey;
  }
  if (f.checksum) out.ChecksumAlgorithm = f.checksum;
  const tags = f.tags.filter((t) => t.key);
  if (tags.length > 0) out.Tagging = tags.map((t) => `${encodeURIComponent(t.key)}=${encodeURIComponent(t.value)}`).join("&");
  const user: Record<string, string> = {};
  for (const m of f.metadata.filter((m) => m.key)) {
    const k = m.key.trim().toLowerCase();
    if (SYSTEM[k]) out[SYSTEM[k]] = m.value;
    else user[k.replace(/^x-amz-meta-/, "")] = m.value;
  }
  if (Object.keys(user).length > 0) out.Metadata = user;
  return out;
}

export const NO_PROPS: UploadForm = { storageClass: "STANDARD", encryption: "bucket", kmsKey: "", checksum: "", tags: [], metadata: [] };

/** One file's result, as the Status page lists it. */
export type Sent = { key: string; type: string; size: number; status: number; message?: string };

const sent = new Map<string, Sent[]>();
export const setSent = (bucket: string, rows: Sent[]): void => void sent.set(bucket, rows);
export const sentOf = (bucket: string): Sent[] | undefined => sent.get(bucket);

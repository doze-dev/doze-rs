// The S3 pages' calls: a bucket's settings read through the operations its tabs list (crates/aws-console/src/resources/s3_ops.rs),
// and the address helpers that move between the pages of one bucket.
import { useNavigate } from "@tanstack/react-router";
import { ApiFailure } from "@doze/ui/lib/api";
import { useResourceDetail } from "../../../api/resources";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import type { OwnSearch } from "../search";
import { type Target, useOp, useOpRead } from "../deep/api";
import { versioningOf, type Versioning } from "./words";

export const bucketOf = (name: string): Target => ({ service: "s3", kind: "buckets", name });

/** The refusals that mean a setting was never set (S3 answers an error, not an empty configuration). */
export const NONE = {
  policy: ["NoSuchBucketPolicy"],
  cors: ["NoSuchCORSConfiguration"],
  encryption: ["ServerSideEncryptionConfigurationNotFoundError"],
  lifecycle: ["NoSuchLifecycleConfiguration"],
  website: ["NoSuchWebsiteConfiguration"],
  bpa: ["NoSuchPublicAccessBlockConfiguration"],
  ownership: ["OwnershipControlsNotFoundError"],
  lock: ["ObjectLockConfigurationNotFoundError"],
};

/** One read of a bucket setting. `absent` is the refusal that says it was never set; `failed` is any other. */
export function useSetting<T>(name: string, operation: string, none: string[], input: object = {}, enabled = true) {
  const q = useOpRead<T>(bucketOf(name), operation, input, enabled);
  const absent = q.error instanceof ApiFailure && none.includes(q.error.code);
  return { ...q, absent, failed: q.error && !absent ? q.error : null };
}

/** A bucket's own facts: what the page header says, from the descriptor's detail. */
export function useBucket(name: string) {
  const q = useResourceDetail("s3", "buckets", name);
  const d: ResourceDetail | undefined = q.data;
  const field = (label: string) => d?.fields.find((f) => f.label === label)?.value ?? undefined;
  const versioning: Versioning | undefined = d ? versioningOf(field("Versioning") === "Disabled" ? undefined : field("Versioning")) : undefined;
  return { ...q, detail: d, region: field("Region") ?? "us-east-1", created: field("Created"), versioning };
}

/** Writes one of the bucket's settings; its reads are stale after. */
export const usePut = (name: string, operation: string) => useOp<unknown>(bucketOf(name), operation);

export type Search = OwnSearch & { tab?: string; p?: string };
const KEYS = ["do", "name", "key", "ver", "sel", "dialog", "versions", "mode", "tab", "p"] as const;

/** Moves between the pages of one bucket (`?do=`, `?tab=`, `?key=`); what is not named is not kept. */
export function useBucketNav(name: string) {
  const navigate = useNavigate();
  return (search: Search = {}, replace = false) => {
    const clean = Object.fromEntries(KEYS.flatMap((k) => (search[k] ? [[k, search[k]]] : [])));
    void navigate({ to: "/resources/$service/$kind/$name", params: { service: "s3", kind: "buckets", name }, search: clean, replace });
  };
}

/** Moves between the pages of the list of buckets (`?do=create`). */
export function useListNav() {
  const navigate = useNavigate();
  return (search: OwnSearch = {}) => {
    const clean = Object.fromEntries((["do", "name"] as const).flatMap((k) => (search[k] ? [[k, search[k]]] : [])));
    void navigate({ to: "/resources/$service/$second", params: { service: "s3", second: "buckets" }, search: clean });
  };
}

/** The crumbs every bucket page starts with: Resources, the group, S3, the bucket. */
export function crumbs(name: string | null, group = "Storage and data"): { label: string; to: string }[] {
  const base = [{ label: "Resources", to: "/resources" }, { label: group, to: "/resources" }, { label: "S3", to: "/resources/s3/buckets" }];
  return name ? [...base, { label: name, to: `/resources/s3/buckets/${encodeURIComponent(name)}` }] : base;
}

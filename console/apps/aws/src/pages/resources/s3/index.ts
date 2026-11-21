// S3's pages, drawn by the console in place of the generic ones: the list of buckets, a bucket (Objects, Properties, Permissions,
// Management), and the full-page forms and results of `?do=` (a create, a delete, an upload, a setting's edit).
import { lazy, type ReactNode } from "react";
import { type OwnPage, type OwnProps, registerOwn } from "../deep/registry";

type Page = (p: OwnProps) => ReactNode;
const page = <K extends string>(load: () => Promise<Record<K, Page>>, key: K): OwnPage => lazy(() => load().then((m) => ({ default: m[key] })));

registerOwn("s3", "buckets", {
  list: page(() => import("./buckets-list"), "BucketsList"),
  one: page(() => import("./bucket-page"), "BucketPage"),
  actions: {
    create: page(() => import("./bucket-create"), "BucketCreate"),
    delete: page(() => import("./bucket-danger"), "BucketDelete"),
    empty: page(() => import("./bucket-danger"), "BucketEmpty"),
    "empty-status": page(() => import("./bucket-danger"), "EmptyStatus"),
    "versioning-edit": page(() => import("./edits-a"), "VersioningEdit"),
    "ownership-edit": page(() => import("./edits-a"), "OwnershipEdit"),
    "bpa-edit": page(() => import("./edits-a"), "BpaEdit"),
    "encryption-edit": page(() => import("./edits-a"), "EncryptionEdit"),
    "website-edit": page(() => import("./edits-a"), "WebsiteEdit"),
    "cors-edit": page(() => import("./edits-b"), "CorsEdit"),
    "policy-edit": page(() => import("./edits-b"), "PolicyEdit"),
    "tags-edit": page(() => import("./edits-b"), "TagsEdit"),
    "notification-create": page(() => import("./edits-c"), "NotificationCreate"),
    "lifecycle-create": page(() => import("./edits-c"), "LifecycleCreate"),
    upload: page(() => import("./upload"), "Upload"),
    "upload-status": page(() => import("./upload"), "UploadStatus"),
    "folder-create": page(() => import("./upload"), "FolderCreate"),
    object: page(() => import("./object-page"), "ObjectPage"),
    "object-copy": page(() => import("./object-copy"), "ObjectCopy"),
    "object-delete": page(() => import("./object-delete"), "ObjectDelete"),
    "object-delete-version": page(() => import("./object-delete"), "ObjectDeleteVersion"),
  },
});

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
  },
});

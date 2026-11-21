// S3's pages of a bucket: its objects.
import { lazy } from "react";
import { register } from "../deep/registry";

register("s3", {
  objects: { page: lazy(() => import("./objects").then((m) => ({ default: m.BucketObjects }))) },
});

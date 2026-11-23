// KMS's pages: its keys, drawn by the console's app in place of the generic list and detail pages.
import { lazy } from "react";
import { registerKind } from "../deep/registry";

registerKind("kms", "keys", {
  list: lazy(() => import("./list").then((m) => ({ default: m.KmsList }))),
  detail: lazy(() => import("./detail").then((m) => ({ default: m.KmsDetail }))),
});

// Secrets Manager's pages: its secrets, drawn by the console's app in place of the generic list and detail pages.
import { lazy } from "react";
import { registerKind } from "../deep/registry";

registerKind("secretsmanager", "secrets", {
  list: lazy(() => import("./list").then((m) => ({ default: m.SecretsList }))),
  detail: lazy(() => import("./detail").then((m) => ({ default: m.SecretDetail }))),
});

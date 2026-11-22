// Parameter Store's pages: its parameters, drawn by the console's app in place of the generic list and detail pages.
import { lazy } from "react";
import { registerKind } from "../deep/registry";

registerKind("ssm", "parameters", {
  list: lazy(() => import("./list").then((m) => ({ default: m.SsmList }))),
  detail: lazy(() => import("./detail").then((m) => ({ default: m.SsmDetail }))),
});

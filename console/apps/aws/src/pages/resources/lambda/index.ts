// Lambda's pages: its functions (list, create, one function with its tabs and forms) and its layers, drawn by the console's app in
// place of the generic list and detail pages.
import { lazy } from "react";
import { registerKind } from "../deep/registry";

registerKind("lambda", "functions", {
  list: lazy(() => import("./list").then((m) => ({ default: m.LambdaList }))),
  detail: lazy(() => import("./detail").then((m) => ({ default: m.FunctionDetail }))),
});

registerKind("lambda", "layers", {
  list: lazy(() => import("./layers").then((m) => ({ default: m.LayersList }))),
  detail: lazy(() => import("./layers").then((m) => ({ default: m.LayerDetail }))),
});

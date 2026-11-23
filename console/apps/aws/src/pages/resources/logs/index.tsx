// CloudWatch Logs' pages: the log groups' list and one group's page (drawn by the console's app in place of the generic ones), and a
// stream's events, which is a page of its own (`deep/routes.tsx`).
import { lazy } from "react";
import { registerKind } from "../deep/registry";

registerKind("logs", "groups", {
  list: lazy(() => import("./list").then((m) => ({ default: m.LogsList }))),
  detail: lazy(() => import("./detail").then((m) => ({ default: m.GroupDetail }))),
});

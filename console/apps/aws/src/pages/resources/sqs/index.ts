// SQS's pages: its queues, drawn by the console's app in place of the generic list and detail pages.
import { lazy } from "react";
import { registerKind } from "../deep/registry";

registerKind("sqs", "queues", {
  list: lazy(() => import("./list").then((m) => ({ default: m.SqsList }))),
  detail: lazy(() => import("./detail").then((m) => ({ default: m.SqsDetail }))),
});

// SNS's pages: its topics and subscriptions, drawn by the console's app in place of the generic list and detail pages.
import { lazy } from "react";
import { registerKind } from "../deep/registry";

registerKind("sns", "topics", {
  list: lazy(() => import("./topics-list").then((m) => ({ default: m.TopicsList }))),
  detail: lazy(() => import("./topic-detail").then((m) => ({ default: m.TopicDetail }))),
});
registerKind("sns", "subscriptions", {
  list: lazy(() => import("./subs-list").then((m) => ({ default: m.SubsList }))),
  detail: lazy(() => import("./sub-detail").then((m) => ({ default: m.SubDetail }))),
});

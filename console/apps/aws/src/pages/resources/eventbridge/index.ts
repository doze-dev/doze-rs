// EventBridge's pages: its event buses, rules, archives, replays and API destinations, drawn by the console's app in place of the generic list and
// detail pages.
import { lazy } from "react";
import { registerKind } from "../deep/registry";

registerKind("eventbridge", "buses", {
  list: lazy(() => import("./buses-list").then((m) => ({ default: m.BusesList }))),
  detail: lazy(() => import("./bus-detail").then((m) => ({ default: m.BusDetail }))),
});
registerKind("eventbridge", "rules", {
  list: lazy(() => import("./rules-list").then((m) => ({ default: m.RulesList }))),
  detail: lazy(() => import("./rule-detail").then((m) => ({ default: m.RuleDetail }))),
});
registerKind("eventbridge", "archives", { list: lazy(() => import("./archives-list").then((m) => ({ default: m.ArchivesList }))) });
registerKind("eventbridge", "replays", { list: lazy(() => import("./replays-list").then((m) => ({ default: m.ReplaysList }))) });
registerKind("eventbridge", "api-destinations", { list: lazy(() => import("./dests-list").then((m) => ({ default: m.DestsList }))) });

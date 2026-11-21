// EventBridge's pages: its event buses, rules, archives, replays and API destinations, drawn by the console's app in place of the generic list and
// detail pages.
import { lazy } from "react";
import { registerKind } from "../deep/registry";

registerKind("eventbridge", "buses", {
  list: lazy(() => import("./buses-list").then((m) => ({ default: m.BusesList }))),
  detail: lazy(() => import("./bus-detail").then((m) => ({ default: m.BusDetail }))),
});

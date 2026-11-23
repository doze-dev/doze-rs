// CloudWatch's pages, drawn by the console's app in place of the generic ones: its own page (the overview), the alarms (list, one,
// and the create and edit wizard), the metrics (browse and graph) and the dashboards (list, one, and the page of a metric widget).
import { lazy } from "react";
import { registerKind, registerOwn, registerService, type OwnPage, type OwnProps } from "../deep/registry";

registerService("cloudwatch", lazy(() => import("./overview").then((m) => ({ default: m.Overview }))));

registerKind("cloudwatch", "alarms", {
  list: lazy(() => import("./alarms-list").then((m) => ({ default: m.AlarmsList }))),
  detail: lazy(() => import("./alarm-detail").then((m) => ({ default: m.AlarmDetail }))),
});

registerKind("cloudwatch", "metrics", {
  list: lazy(() => import("./metrics-page").then((m) => ({ default: m.MetricsPage }))),
});

const widget = lazy(() => import("./widget-page").then((m) => ({ default: ({ name, search }: OwnProps) => <m.WidgetPage name={name ?? ""} mode={search.mode} /> })));
registerOwn("cloudwatch", "dashboards", {
  list: lazy(() => import("./dashboards-list").then((m) => ({ default: m.DashboardsList }))) as unknown as OwnPage,
  one: lazy(() => import("./dashboard-view").then((m) => ({ default: m.DashboardPage }))) as unknown as OwnPage,
  actions: { widget: widget as OwnPage },
});

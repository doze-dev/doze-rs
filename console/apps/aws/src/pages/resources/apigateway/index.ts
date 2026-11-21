// API Gateway's pages: the list of APIs (REST and HTTP together), a REST API's page, an HTTP API's page, and the usage plans, drawn by the
// console's app in place of the generic list and detail pages.
import { lazy } from "react";
import { registerKind } from "../deep/registry";

const list = lazy(() => import("./list").then((m) => ({ default: m.ApisList })));
registerKind("apigateway", "restapis", { list, detail: lazy(() => import("./rest-detail").then((m) => ({ default: m.RestDetail }))) });
registerKind("apigateway", "apis", { list, detail: lazy(() => import("./http-detail").then((m) => ({ default: m.HttpDetail }))) });
registerKind("apigateway", "usageplans", { list: lazy(() => import("./plans-list").then((m) => ({ default: m.PlansList }))), detail: lazy(() => import("./plan-detail").then((m) => ({ default: m.PlanDetail }))) });

// IAM's pages: its users, groups, roles and policies, drawn by the console's app in place of the generic list and detail pages.
import { lazy } from "react";
import { registerKind, registerService } from "../deep/registry";

registerKind("iam", "users", {
  list: lazy(() => import("./users-list").then((m) => ({ default: m.UsersList }))),
  detail: lazy(() => import("./user-detail").then((m) => ({ default: m.UserDetail }))),
});
registerKind("iam", "groups", {
  list: lazy(() => import("./groups-list").then((m) => ({ default: m.GroupsList }))),
  detail: lazy(() => import("./group-detail").then((m) => ({ default: m.GroupDetail }))),
});
registerKind("iam", "roles", {
  list: lazy(() => import("./roles-list").then((m) => ({ default: m.RolesList }))),
  detail: lazy(() => import("./role-detail").then((m) => ({ default: m.RoleDetail }))),
});
registerKind("iam", "policies", {
  list: lazy(() => import("./policies-list").then((m) => ({ default: m.PoliciesList }))),
  detail: lazy(() => import("./policy-detail").then((m) => ({ default: m.PolicyDetail }))),
});
registerService("iam", lazy(() => import("./dashboard").then((m) => ({ default: m.Dashboard }))));
registerService("sts", lazy(() => import("./sts-switch").then((m) => ({ default: m.SwitchRole }))));

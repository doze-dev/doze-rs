// IAM's pages: its users, groups, roles and policies, drawn by the console's app in place of the generic list and detail pages.
import { lazy } from "react";
import { registerKind } from "../deep/registry";

registerKind("iam", "users", {
  list: lazy(() => import("./users-list").then((m) => ({ default: m.UsersList }))),
  detail: lazy(() => import("./user-detail").then((m) => ({ default: m.UserDetail }))),
});

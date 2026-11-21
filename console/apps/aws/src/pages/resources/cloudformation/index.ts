// CloudFormation's pages, drawn by the console's app in place of the generic list and detail: the stacks (list, one, and the
// full-page flows of `?do=`: create, update, a change set) and the exports.
import { lazy, type ReactNode } from "react";
import { type OwnPage, type OwnProps, registerOwn } from "../deep/registry";

type Page = (p: OwnProps) => ReactNode;
const page = <K extends string>(load: () => Promise<Record<K, Page>>, key: K): OwnPage => lazy(() => load().then((m) => ({ default: m[key] })));

registerOwn("cloudformation", "stacks", {
  list: page(() => import("./list"), "StacksListPage"),
  one: page(() => import("./detail"), "StackPage"),
  actions: {
    create: page(() => import("./wizard"), "CreateStackPage"),
    update: page(() => import("./wizard"), "UpdateStackPage"),
    changeset: page(() => import("./changeset-page"), "ChangeSetPage"),
  },
});
registerOwn("cloudformation", "exports", {
  list: page(() => import("./exports"), "ExportsListPage"),
  one: page(() => import("./exports"), "ExportPage"),
});

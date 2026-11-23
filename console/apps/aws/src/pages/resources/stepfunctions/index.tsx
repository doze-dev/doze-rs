// Step Functions' pages, drawn by the console in place of the generic ones: the list of state machines, a machine (Executions,
// Definition, Versions, Aliases, Tags), Workflow Studio (`?do=create`, `?do=edit`), Create alias and Manage tags, and the list of
// activities with its Create activity page. An execution is a page of its own (`../deep/routes.tsx`).
import { lazy, type ReactNode } from "react";
import { type OwnPage, type OwnProps, registerOwn } from "../deep/registry";

type Page = (p: OwnProps) => ReactNode;
const page = <K extends string>(load: () => Promise<Record<K, Page>>, key: K): OwnPage => lazy(() => load().then((m) => ({ default: m[key] })));

registerOwn("stepfunctions", "machines", {
  list: page(() => import("./list"), "MachinesList"),
  one: lazy(() => import("./detail").then((m) => ({ default: m.MachineDetail }))) as unknown as OwnPage,
  actions: {
    create: page(() => import("./studio-pages"), "StudioCreate"),
    edit: page(() => import("./studio-pages"), "StudioEdit"),
    "alias-create": page(() => import("./alias-create"), "AliasCreate"),
    tags: page(() => import("./tags-page"), "TagsPage"),
  },
});

registerOwn("stepfunctions", "activities", {
  list: page(() => import("./activities"), "ActivitiesList"),
  actions: { create: page(() => import("./activities"), "ActivityCreate") },
});

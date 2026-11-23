// DynamoDB's pages, drawn by the console in place of the generic ones: the list of tables, a table (Overview, Indexes, Monitor, Exports
// and streams, Additional settings), and the full pages of `?do=` (Create table, Create global secondary index, Edit read/write
// capacity, Turn on TTL, Explore table items, Create item, Edit item, the PartiQL editor).
import { lazy, type ReactNode } from "react";
import { type OwnPage, type OwnProps, registerOwn } from "../deep/registry";

type Page = (p: OwnProps) => ReactNode;
const page = <K extends string>(load: () => Promise<Record<K, Page>>, key: K): OwnPage => lazy(() => load().then((m) => ({ default: m[key] })));

registerOwn("dynamodb", "tables", {
  list: page(() => import("./list"), "TablesList"),
  one: page(() => import("./detail"), "TablePage"),
  actions: {
    create: page(() => import("./create"), "TableCreate"),
    partiql: page(() => import("./partiql"), "Partiql"),
    explore: page(() => import("./explore"), "Explore"),
    "item-create": page(() => import("./item-pages"), "ItemCreate"),
    "item-edit": page(() => import("./item-pages"), "ItemEdit"),
    "index-create": page(() => import("./index-create"), "IndexCreate"),
    "capacity-edit": page(() => import("./settings-forms"), "CapacityEdit"),
    "ttl-edit": page(() => import("./settings-forms"), "TtlEdit"),
  },
});

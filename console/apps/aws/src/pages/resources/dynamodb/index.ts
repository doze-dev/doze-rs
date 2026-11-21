// DynamoDB's pages of a table: its items.
import { lazy } from "react";
import { register } from "../deep/registry";

register("dynamodb", {
  items: { page: lazy(() => import("./items").then((m) => ({ default: m.TableItems }))) },
});

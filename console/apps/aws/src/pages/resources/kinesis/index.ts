// Kinesis's pages: the data streams' list and one stream's page, drawn by the console's app in place of the generic ones.
import { lazy } from "react";
import { registerKind } from "../deep/registry";

registerKind("kinesis", "streams", {
  list: lazy(() => import("./list").then((m) => ({ default: m.KinesisList }))),
  detail: lazy(() => import("./detail").then((m) => ({ default: m.KinesisDetail }))),
});

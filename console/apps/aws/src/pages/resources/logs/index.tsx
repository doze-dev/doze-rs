// CloudWatch Logs' pages of a log group: its streams, search, Live Tail.
import { lazy } from "react";
import { type DeepProps, register } from "../deep/registry";

register("logs", {
  streams: { page: lazy(() => import("./streams-tab").then((m) => ({ default: ({ d }: DeepProps) => <m.GroupStreams group={d.name} /> }))) },
  search: { page: lazy(() => import("./search-tab").then((m) => ({ default: ({ d }: DeepProps) => <m.GroupSearch group={d.name} /> }))) },
  tail: { page: lazy(() => import("./tail-tab").then((m) => ({ default: ({ d }: DeepProps) => <m.GroupTail group={d.name} /> }))) },
});

// SQS's pages of a queue: its messages, its dead-letter redrive, its settings.
import { lazy } from "react";
import { register } from "../deep/registry";

register("sqs", {
  messages: { page: lazy(() => import("./messages").then((m) => ({ default: m.QueueMessages }))) },
  redrive: {
    page: lazy(() => import("./redrive").then((m) => ({ default: m.QueueRedrive }))),
    when: (d) => d.subs.some((s) => s.title.includes("dead-letter queue") && s.rows.length > 0),
  },
  settings: { page: lazy(() => import("./settings").then((m) => ({ default: m.QueueSettings }))) },
});

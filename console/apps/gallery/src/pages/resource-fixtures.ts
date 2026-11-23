// A queue's detail as the console's API answers it (contract §6): the generated fields and the kit's additions.
import type { ResourceDetail } from "@aws-kit";

const base: Pick<ResourceDetail, "service" | "service_title" | "group" | "kind" | "kind_title" | "one" | "operations" | "subs" | "calls" | "reveal" | "delete" | "deep" | "sentence" | "badges" | "wired" | "tags" | "verbs"> = { service: "sqs", service_title: "SQS", group: "Messaging and events", kind: "queues", kind_title: "Queues", one: "queue", operations: ["GetQueueAttributes"], subs: [], calls: [], reveal: null, delete: null, deep: [], sentence: null, badges: [], wired: [], tags: false, verbs: [] };

export const QUEUE: ResourceDetail = {
  ...base,
  name: "pick-lists",
  id: "http://127.0.0.1:14590/123456789012/pick-lists",
  fields: [
    { label: "ARN", value: "arn:aws:sqs:us-east-1:123456789012:pick-lists", format: "mono", hidden: false, link: null },
    { label: "URL", value: "http://127.0.0.1:14590/123456789012/pick-lists", format: "mono", hidden: false, link: null },
    { label: "Visibility timeout", value: "30", format: "seconds", hidden: false, link: null },
    { label: "Message retention period", value: "345600", format: "seconds", hidden: false, link: null },
    { label: "Dead-letter queue", value: "pick-lists-dlq", format: "mono", hidden: false, link: { service: "sqs", kind: "queues", name: "pick-lists-dlq" } },
    { label: "Approx. messages", value: "2 available, 1 in flight, 0 delayed", format: "text", hidden: false, link: null },
  ],
  sentence: "pick-lists has 2 messages waiting and 1 in flight. A message received more than 5 times moves to pick-lists-dlq, which holds 1 now.",
  badges: [{ text: "Standard", tone: "neutral" }],
  wired: [
    { label: "Fed by", to: { service: "events", kind: "rules", name: "payment-settled" }, note: "The rule sends matching events here." },
    { label: "Dead letters to", to: { service: "sqs", kind: "queues", name: "pick-lists-dlq" }, note: null },
    { label: "Encrypted with", to: { service: "kms", kind: "keys", name: "alias/larder-app" }, note: null },
  ],
  tags: true,
  verbs: [
    { id: "edit", label: "Edit", danger: false, op: null, needs: null },
    { id: "delete", label: "Delete", danger: true, op: "DeleteQueue", needs: "confirm" },
    { id: "purge", label: "Purge", danger: false, op: "PurgeQueue", needs: "confirm" },
    { id: "send", label: "Send and receive messages", danger: false, op: null, needs: null },
    { id: "redrive", label: "Start DLQ redrive", danger: false, op: "StartMessageMoveTask", needs: "input" },
  ],
};

/** A kind with nothing of its own: no sentence, badges, wired or verbs, and a field with a very long value. */
export const BARE: ResourceDetail = {
  ...base,
  service: "ssm",
  service_title: "Systems Manager",
  group: "Config and secrets",
  kind: "parameters",
  kind_title: "Parameters",
  one: "parameter",
  name: "/larder/prod/a-parameter-whose-name-is-long-enough-to-wrap-the-title-of-the-page-on-a-narrow-window",
  id: "/larder/prod/db-url",
  fields: [
    { label: "Type", value: "SecureString", format: "text", hidden: false, link: null },
    { label: "Value", value: null, format: "text", hidden: true, link: null },
    { label: "Description", value: "A connection string. ".repeat(12), format: "text", hidden: false, link: null },
    { label: "Version", value: "3", format: "number", hidden: false, link: null },
  ],
};

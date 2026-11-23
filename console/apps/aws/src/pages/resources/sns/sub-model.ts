// A subscription as the console's lists and details give it: its row's name is its ARN, or, for one waiting for its endpoint, what
// tells it apart (`pending|<topic ARN>|<protocol>|<endpoint>`, crates/aws-console/src/resources/pending.rs). Pure.
import type { ResourceRow } from "../../../api/types/ResourceRow";
import { protocolLabel } from "./protocols";

const PENDING = "pending|";

export type SubRow = {
  /** The row's name in the console's addresses. */
  name: string;
  pending: boolean;
  topicArn: string;
  topic: string;
  protocol: string;
  endpoint: string;
  /** The subscription's own id: the last part of its ARN; none while it is pending. */
  id: string | null;
};

export const isPending = (name: string): boolean => name.startsWith(PENDING);

/** The segments after the last colon (`orders-created` for an ARN of one). */
export const tail = (arn: string): string => arn.slice(arn.lastIndexOf(":") + 1);

/** `5f2a91c4…`: the first eight characters of the id, or `PendingConfirmation`. */
export const shortId = (id: string | null): string => (id === null ? "PendingConfirmation" : `${id.slice(0, 8)}…`);

/** `arn:aws:sqs:us-east-1:…:invoice-queue`: an ARN with the account left out, as the board draws it. */
export function shortArn(arn: string): string {
  const p = arn.split(":");
  if (p.length < 6 || p[0] !== "arn") return arn;
  p[4] = "…";
  return p.join(":");
}

/** The topic's ARN of a confirmed subscription's ARN: everything before the id. */
export const topicOfSubscription = (arn: string): string => arn.slice(0, arn.lastIndexOf(":"));

/** One row of the subscriptions list (cells: Topic, Protocol, Endpoint). */
export function subRow(r: ResourceRow): SubRow {
  const pending = isPending(r.name);
  const parts = pending ? r.name.slice(PENDING.length).split("|") : [];
  const topicArn = pending ? (parts[0] ?? "") : topicOfSubscription(r.name);
  return { name: r.name, pending, topicArn, topic: tail(topicArn), protocol: r.cells[1] ?? parts[1] ?? "", endpoint: r.cells[2] ?? parts.slice(2).join("|"), id: pending ? null : tail(r.name) };
}

/** One row of a topic's own Subscriptions list: the row's cells are Subscription (the ARN's tail, or `PendingConfirmation`), Protocol and Endpoint, and
 * its link is the row's name. */
export function topicSubRow(r: { cells: (string | null)[]; link: { name: string } | null }, topicArn: string): SubRow {
  const name = r.link?.name ?? "";
  const pending = isPending(name) || r.cells[0] === "PendingConfirmation";
  return { name, pending, topicArn, topic: tail(topicArn), protocol: r.cells[1] ?? "", endpoint: r.cells[2] ?? "", id: pending ? null : (r.cells[0] ?? null) };
}

/** What the endpoint is shown as: a queue's or function's name for SQS and Lambda, else as it is. */
export const endpointName = (protocol: string, endpoint: string): string => (protocol === "sqs" || protocol === "lambda" ? tail(endpoint) : endpoint);

/** `Amazon SQS`: the protocol as the tables say it. */
export const protocolName = (s: Pick<SubRow, "protocol">): string => protocolLabel(s.protocol);

/** The status column: `Confirmed` or `Pending confirmation`. */
export const statusOf = (s: Pick<SubRow, "pending">): "Confirmed" | "Pending confirmation" => (s.pending ? "Pending confirmation" : "Confirmed");

/** What can be said of why a pending subscription is pending, by its protocol. */
export function pendingWhy(protocol: string): string {
  if (protocol === "email" || protocol === "email-json") return "Stored: doze-aws sends no email";
  if (protocol === "http" || protocol === "https") return "Waiting for the endpoint to confirm";
  return "Waiting for confirmation";
}

/** Confirmed first, then the ones that wait (a webhook before an address, which never confirms here), each as the service listed them. */
export function orderSubs<T extends Pick<SubRow, "pending" | "protocol">>(rows: readonly T[]): T[] {
  const rank = (s: T): number => (s.pending ? (s.protocol.startsWith("email") ? 2 : 1) : 0);
  return [...rows].sort((a, b) => rank(a) - rank(b));
}

/** What the palette calls a subscription (its name is an ARN, or `pending|...`): the topic it is on, and its id's start or, while it waits, its protocol and endpoint. */
export function subscriptionLabel(name: string): string {
  const s = subRow({ name, cells: [], link: null } as unknown as ResourceRow);
  return s.pending ? `${s.topic} · ${s.protocol} ${s.endpoint}`.trim() : `${s.topic} · ${shortId(s.id)}`;
}

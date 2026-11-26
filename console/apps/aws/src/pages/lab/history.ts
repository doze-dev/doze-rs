// The calls sent from the Lab, kept in this browser: the last fifty for each region, each with what is needed to send it again.
// Pure: `kept.ts` keeps the list and `sent-list.tsx` draws it.
import type { LabAnswer } from "../../api/types/LabAnswer";

export const MOST_SENT = 50;
/** An input longer than this (as JSON) is not kept: the list stays small, and a call that big is not one to replay by hand. */
export const MOST_INPUT = 16_384;

export type Sent = {
  id: string;
  /** When it was sent (ms). */
  at: number;
  /** The API on the bus (`sqs`, `states`) and the service by its `--services` name (`stepfunctions`). */
  target: string;
  service: string;
  /** The service's title (`SQS`). */
  title: string;
  op: string;
  /** What was sent; `null` when it was too long to keep. */
  input: Record<string, unknown> | null;
  /** The answer: ok, or the error's code. */
  ok: boolean;
  code: string | null;
  status: number | null;
  took_us: number;
  /** The input in one line. */
  summary: string;
};

/** What was asked of the Lab: the target it was sent to and the operation with its input. */
export type Ask = { target: string; service: string; title: string; op: string; input: Record<string, unknown> };

/** The last part of a long value (`http://host/111122223333/orders` is `orders`, an ARN is its name), so a line says which thing, not where it lives. */
export function tail(v: string): string {
  const cut = v.split(/[/:]/).filter(Boolean);
  return v.length > 28 && cut.length > 1 ? (cut[cut.length - 1] ?? v) : v;
}

function short(v: unknown): string {
  if (typeof v === "string") {
    const t = tail(v);
    return JSON.stringify(t.length > 28 ? `${t.slice(0, 27)}…` : t);
  }
  if (Array.isArray(v)) return `[${v.length}]`;
  if (v !== null && typeof v === "object") return "{…}";
  return String(v);
}

/** An input in one line, `QueueUrl: "orders", MessageBody: "hello"`, cut at `most` characters. */
export function summarise(input: Record<string, unknown>, most = 96): string {
  const line = Object.entries(input)
    .map(([k, v]) => `${k}: ${short(v)}`)
    .join(", ");
  if (line === "") return "No input";
  return line.length > most ? `${line.slice(0, most - 1)}…` : line;
}

/** The entry for a call and the answer it got. `id` and `at` are given so a test can fix them. */
export function sentFrom(ask: Ask, answer: LabAnswer, id: string, at: number): Sent {
  const keep = JSON.stringify(ask.input).length <= MOST_INPUT;
  return {
    id,
    at,
    target: ask.target,
    service: ask.service,
    title: ask.title,
    op: ask.op,
    input: keep ? ask.input : null,
    ok: answer.error === null,
    code: answer.error?.code ?? null,
    status: answer.error?.status ?? null,
    took_us: Number(answer.took_us),
    summary: summarise(ask.input),
  };
}

/** The list with `s` first, at most fifty. */
export const addSent = (list: Sent[], s: Sent): Sent[] => [s, ...list].slice(0, MOST_SENT);

const text = (v: unknown, most = 200): v is string => typeof v === "string" && v.length > 0 && v.length <= most;

/** One entry as it was kept, or `null` when it is not whole (an older or edited value). */
export function parseSent(v: unknown): Sent | null {
  if (v === null || typeof v !== "object") return null;
  const o = v as Record<string, unknown>;
  const input = o.input === null ? null : typeof o.input === "object" && !Array.isArray(o.input) ? (o.input as Record<string, unknown>) : undefined;
  if (!text(o.id) || typeof o.at !== "number" || !text(o.target) || !text(o.service) || !text(o.title) || !text(o.op) || input === undefined) return null;
  if (typeof o.ok !== "boolean" || typeof o.took_us !== "number" || typeof o.summary !== "string") return null;
  return {
    id: o.id,
    at: o.at,
    target: o.target,
    service: o.service,
    title: o.title,
    op: o.op,
    input,
    ok: o.ok,
    code: typeof o.code === "string" ? o.code : null,
    status: typeof o.status === "number" ? o.status : null,
    took_us: o.took_us,
    summary: o.summary,
  };
}

/** A fresh id for an entry. */
export const newId = (at: number): string => `${at.toString(36)}-${Math.random().toString(36).slice(2, 8)}`;

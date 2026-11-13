// The typed search params of the Groups pages: filters and overlays live in the address, and a hand-edited address can
// only ever put a known value on screen (the rest is dropped).
import { oneOf, shortText } from "../../lib/search";

export const STATES = ["stable", "rebalancing", "empty"] as const;
export type GroupState = (typeof STATES)[number];

export type GroupsSearch = { q?: string; state?: GroupState[] };
export function groupsSearch(s: Record<string, unknown>): GroupsSearch {
  const raw = Array.isArray(s.state) ? s.state : s.state === undefined ? [] : [s.state];
  const state = STATES.filter((v) => raw.includes(v));
  return { q: shortText(s.q, 249), state: state.length ? state : undefined };
}

/** The dialogs of a group, on every tab: `?dialog=reset`. */
export const DIALOGS = ["reset", "delete", "delete-offsets"] as const;
export type GroupSearch = { dialog?: (typeof DIALOGS)[number] };
export function groupSearch(s: Record<string, unknown>): GroupSearch {
  return { dialog: oneOf(DIALOGS, s.dialog) };
}

const flag = (v: unknown): 1 | undefined => (v === 1 || v === "1" ? 1 : undefined);

export type MembersSearch = { member?: string };
export function membersSearch(s: Record<string, unknown>): MembersSearch {
  return { member: shortText(s.member, 512) };
}

export type LagSearch = { expiring?: 1 };
export function lagSearch(s: Record<string, unknown>): LagSearch {
  return { expiring: flag(s.expiring) };
}

export type StorySearch = { generation?: number; rule?: 1 };
export function storySearch(s: Record<string, unknown>): StorySearch {
  const g = typeof s.generation === "number" ? s.generation : typeof s.generation === "string" && /^-?\d{1,10}$/.test(s.generation) ? Number(s.generation) : undefined;
  return { generation: g !== undefined && Number.isSafeInteger(g) ? g : undefined, rule: flag(s.rule) };
}

export const KINDS = ["join", "commits", "heartbeats"] as const;
export type GroupWireSearch = { kind?: (typeof KINDS)[number]; refused?: 1; q?: string };
export function groupWireSearch(s: Record<string, unknown>): GroupWireSearch {
  return { kind: oneOf(KINDS, s.kind), refused: flag(s.refused), q: shortText(s.q, 249) };
}

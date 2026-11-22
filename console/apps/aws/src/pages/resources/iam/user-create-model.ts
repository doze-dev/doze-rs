// The Create user wizard's form and the calls it makes: CreateUser (with the tags and the boundary), then one AddUserToGroup per group
// and one AttachUserPolicy per policy. Pure.
import { filled, type KeyValueRow } from "@doze/ui/components/app/key-values";
import type { PreviewRequest } from "../../../kit";
import { nameProblem, takenProblem } from "./model";

export type How = "group" | "copy" | "policies";
export type UserForm = { name: string; how: How; groups: ReadonlySet<string>; copyFrom: string | null; policies: ReadonlySet<string>; boundary: string | null; tags: KeyValueRow[] };
export const blankUser: UserForm = { name: "", how: "group", groups: new Set(), copyFrom: null, policies: new Set(), boundary: null, tags: [] };

/** The groups and policies the user gets: the ones chosen, or the ones the user copied from has. */
export type Copied = { groups: string[]; policies: string[] };

export function permissionsOf(f: UserForm, copied: Copied): Copied {
  if (f.how === "group") return { groups: [...f.groups], policies: [] };
  if (f.how === "copy") return copied;
  return { groups: [], policies: [...f.policies] };
}

export const createInput = (f: UserForm): Record<string, unknown> => {
  const tags = filled(f.tags).map((t) => ({ Key: t.key, Value: t.value }));
  return { UserName: f.name || "(a name)", ...(f.boundary ? { PermissionsBoundary: f.boundary } : {}), ...(tags.length > 0 ? { Tags: tags } : {}) };
};

const call = (operation: string, input: Record<string, unknown>): PreviewRequest => ({ service: "iam", operation, input });

/** The calls in the order they are made: the user first. */
export function userCalls(f: UserForm, copied: Copied, withCreate = true): PreviewRequest[] {
  const p = permissionsOf(f, copied);
  const name = f.name || "(a name)";
  return [
    ...(withCreate ? [call("CreateUser", createInput(f))] : []),
    ...p.groups.map((GroupName) => call("AddUserToGroup", { UserName: name, GroupName })),
    ...p.policies.map((PolicyArn) => call("AttachUserPolicy", { UserName: name, PolicyArn })),
  ];
}

/** What is wrong on a step, `null` when it can go on. */
export const stepProblem = (f: UserForm, step: number, taken: readonly string[]): string | null => (step === 0 ? (nameProblem(f.name, "user") ?? takenProblem("user", f.name, taken)) : null);

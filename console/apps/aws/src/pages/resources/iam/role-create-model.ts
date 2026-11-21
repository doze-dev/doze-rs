// The Create role wizard's form and its calls: CreateRole with the trust policy (and the description, tags and boundary), then one
// AttachRolePolicy per policy chosen. Pure.
import { filled, type KeyValueRow } from "@doze/ui/components/app/key-values";
import type { PreviewRequest } from "../../../kit";
import { blankTrust, nameProblem, trustDocument, trustProblem, type TrustChoice } from "./model";

export type RoleForm = { trust: TrustChoice; policies: ReadonlySet<string>; boundary: string | null; name: string; description: string; tags: KeyValueRow[] };
export const blankRole: RoleForm = { trust: blankTrust, policies: new Set(), boundary: null, name: "", description: "", tags: [] };

export const createInput = (f: RoleForm, account: string): Record<string, unknown> => {
  const tags = filled(f.tags).map((t) => ({ Key: t.key, Value: t.value }));
  return { RoleName: f.name || "(a name)", AssumeRolePolicyDocument: trustDocument(f.trust, account), ...(f.description.trim() ? { Description: f.description.trim() } : {}), ...(f.boundary ? { PermissionsBoundary: f.boundary } : {}), ...(tags.length > 0 ? { Tags: tags } : {}) };
};

const call = (operation: string, input: Record<string, unknown>): PreviewRequest => ({ service: "iam", operation, input });

/** The calls in the order they are made; `withCreate` false for the step that only adds the attachments. */
export function roleCalls(f: RoleForm, account: string, withCreate = true): PreviewRequest[] {
  return [...(withCreate ? [call("CreateRole", createInput(f, account))] : []), ...[...f.policies].map((PolicyArn) => call("AttachRolePolicy", { RoleName: f.name || "(a name)", PolicyArn }))];
}

/** What is wrong with a step, `null` when it can go on. */
export function stepProblem(f: RoleForm, step: number, taken: readonly string[]): string | null {
  if (step === 0) return trustProblem(f.trust);
  if (step === 2) return nameProblem(f.name, "role") ?? (taken.includes(f.name) ? `A role named ${f.name} already exists.` : null);
  return null;
}

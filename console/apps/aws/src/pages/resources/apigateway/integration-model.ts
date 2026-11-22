// A REST method's integration request, edited (apigateway__rest-integration-request): the timeout, the mapping templates, the parameters mapped from the
// method request and what happens to a body no template matches. UpdateIntegration takes them as patches of the integration's own fields
// (`/timeoutInMillis`, `/requestTemplates/<content type>`, `/requestParameters/integration.request.<where>.<name>`, `/passthroughBehavior`).
import type { Patch } from "./api";
import type { Integration } from "./types";

export const DEFAULT_TIMEOUT = 29_000;
export const TIMEOUT_LEAST = 50;
export const PASSTHROUGH = ["WHEN_NO_MATCH", "WHEN_NO_TEMPLATES", "NEVER"] as const;
export type Passthrough = (typeof PASSTHROUGH)[number];
export type Where = "querystring" | "path" | "header";

export type TemplateRow = { id: number; type: string; text: string };
export type ParamRow = { id: number; where: Where; name: string; from: string };
export type IntegrationEdit = { defaultTimeout: boolean; timeout: string; passthrough: Passthrough; templates: TemplateRow[]; params: ParamRow[] };

let next = 0;
export const newId = (): number => ++next;

const PREFIX = "integration.request.";
const WHERES: readonly Where[] = ["querystring", "path", "header"];

/** Whether the integration is one whose request is shaped by templates and mappings (a proxy passes the request on as it is). */
export const shapes = (i: Pick<Integration, "type">): boolean => i.type !== "AWS_PROXY" && i.type !== "HTTP_PROXY";

/** The form's start: what the integration has. */
export function fromIntegration(i: Integration): IntegrationEdit {
  const timeout = i.timeoutInMillis ?? DEFAULT_TIMEOUT;
  const params: ParamRow[] = Object.entries(i.requestParameters ?? {}).flatMap(([key, from]) => {
    const where = WHERES.find((w) => key.startsWith(`${PREFIX}${w}.`));
    return where ? [{ id: newId(), where, name: key.slice(`${PREFIX}${where}.`.length), from }] : [];
  });
  const passthrough = (PASSTHROUGH as readonly string[]).includes(i.passthroughBehavior ?? "") ? (i.passthroughBehavior as Passthrough) : "WHEN_NO_MATCH";
  return { defaultTimeout: timeout === DEFAULT_TIMEOUT, timeout: String(timeout), passthrough, templates: Object.entries(i.requestTemplates ?? {}).map(([type, text]) => ({ id: newId(), type, text })), params };
}

/** What is wrong with the form; `null` when it can be saved. */
export function editProblem(e: IntegrationEdit, proxy: boolean): string | null {
  if (!e.defaultTimeout) {
    const n = Number(e.timeout);
    if (!/^\d+$/.test(e.timeout.trim()) || n < TIMEOUT_LEAST || n > DEFAULT_TIMEOUT) return `A custom timeout is ${TIMEOUT_LEAST} to ${DEFAULT_TIMEOUT.toLocaleString("en-US")} milliseconds.`;
  }
  if (proxy) return null;
  const types = e.templates.map((t) => t.type.trim());
  if (types.some((t) => t === "")) return "Give each template its content type, such as application/json.";
  if (new Set(types).size !== types.length) return "Each content type has one template.";
  if (e.params.some((p) => p.name.trim() === "" || p.from.trim() === "")) return "Give each parameter its name and where it is mapped from, such as method.request.querystring.page.";
  const keys = e.params.map((p) => `${p.where}.${p.name.trim()}`);
  return new Set(keys).size === keys.length ? null : "A parameter is mapped once.";
}

/** A JSON Pointer's one part: `~` and `/` are written as `~0` and `~1`. */
export const escapePointer = (part: string): string => part.replace(/~/g, "~0").replace(/\//g, "~1");

/** The patches of one map field (templates, parameters): what is new is added, what changed is replaced, what is gone is removed. */
function mapPatches(field: string, before: Readonly<Record<string, string>>, after: Readonly<Record<string, string>>): Patch[] {
  const out: Patch[] = [];
  for (const [k, v] of Object.entries(after)) {
    if (!(k in before)) out.push({ op: "add", path: `/${field}/${escapePointer(k)}`, value: v });
    else if (before[k] !== v) out.push({ op: "replace", path: `/${field}/${escapePointer(k)}`, value: v });
  }
  for (const k of Object.keys(before)) if (!(k in after)) out.push({ op: "remove", path: `/${field}/${escapePointer(k)}` });
  return out;
}

/** The patches that take `i` to the form: only what changed. A proxy integration has its timeout edited and no more. */
export function integrationPatches(i: Integration, e: IntegrationEdit): Patch[] {
  const out: Patch[] = [];
  const timeout = e.defaultTimeout ? DEFAULT_TIMEOUT : Number(e.timeout);
  if (timeout !== (i.timeoutInMillis ?? DEFAULT_TIMEOUT)) out.push({ op: "replace", path: "/timeoutInMillis", value: String(timeout) });
  if (!shapes(i)) return out;
  if (e.passthrough !== (i.passthroughBehavior ?? "WHEN_NO_MATCH")) out.push({ op: "replace", path: "/passthroughBehavior", value: e.passthrough });
  out.push(...mapPatches("requestTemplates", i.requestTemplates ?? {}, Object.fromEntries(e.templates.map((t) => [t.type.trim(), t.text]))));
  // Only the keys of the three mapped places are the form's: any other key of requestParameters is left as it is.
  const mine = (k: string) => WHERES.some((w) => k.startsWith(`${PREFIX}${w}.`));
  out.push(...mapPatches("requestParameters", Object.fromEntries(Object.entries(i.requestParameters ?? {}).filter(([k]) => mine(k))), Object.fromEntries(e.params.map((p) => [`${PREFIX}${p.where}.${p.name.trim()}`, p.from.trim()]))));
  return out;
}

// A usage plan's form and the calls it makes: CreateUsagePlan with its throttle and quota, a stage added to or taken off a plan (a patch of
// `/apiStages`, valued `<api id>:<stage>`), and a key made and added in one step.
import type { Patch } from "./api";

/** `throttle` and `quota` are the toggles of the form: a plan may have either, both or neither, and the values wait, filled in, while one is off. */
export type PlanForm = { name: string; description: string; throttle: boolean; rate: string; burst: string; quota: boolean; limit: string; period: "DAY" | "WEEK" | "MONTH" };
export const emptyPlan: PlanForm = { name: "", description: "", throttle: false, rate: "100", burst: "200", quota: false, limit: "10000", period: "MONTH" };

const num = (s: string) => /^\d+(\.\d+)?$/.test(s.trim());
const whole = (s: string) => /^\d+$/.test(s.trim());

export function planProblem(f: PlanForm): string | null {
  if (f.name.trim() === "") return "Enter a name for the plan.";
  if (f.throttle && (f.rate.trim() === "" || f.burst.trim() === "")) return "Throttling needs both a rate and a burst.";
  if (f.throttle && !(num(f.rate) && whole(f.burst))) return "The rate is a number of requests a second, the burst a whole number.";
  if (f.quota && !whole(f.limit)) return "The quota is a whole number of requests.";
  return null;
}

/** Throttle and quota are left out when their toggles are off: a plan may have either, both or neither. */
export function planInput(f: PlanForm): Record<string, unknown> {
  return {
    name: f.name || "<name>",
    ...(f.description ? { description: f.description } : {}),
    ...(f.throttle ? { throttle: { rateLimit: Number(f.rate), burstLimit: Number(f.burst) } } : {}),
    ...(f.quota ? { quota: { limit: Number(f.limit), period: f.period } } : {}),
  };
}

/** The REST API the Add API stage dialog offers first: the first with a stage the plan does not cover yet (an API with no stage, or all covered, has nothing to add);
 * the first API when none is known to have one. `stages`: each API's stages as far as read. */
export function firstWithFreeStage(apis: readonly { id: string }[], stages: Readonly<Record<string, readonly { stageName: string }[] | undefined>>, covered: readonly { apiId?: string; stage?: string }[]): string {
  const free = (id: string) => (stages[id] ?? []).some((s) => !covered.some((c) => c.apiId === id && c.stage === s.stageName));
  return (apis.find((a) => free(a.id)) ?? apis[0])?.id ?? "";
}

export const stageValue = (api: string, stage: string): string => `${api}:${stage}`;
export const addStagePatch = (api: string, stage: string): Patch[] => [{ op: "add", path: "/apiStages", value: stageValue(api, stage) }];
export const removeStagePatch = (api: string, stage: string): Patch[] => [{ op: "remove", path: "/apiStages", value: stageValue(api, stage) }];

export const keyProblem = (name: string): string | null => (name.trim() === "" ? "Enter a name for the key." : null);
